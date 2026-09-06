#![deny(unsafe_code)]

use aviqtl_app::{
    ApplicationModel, LifecycleStep, ProjectDefaults, ProjectSession, ProjectSettingsInput,
    SaveDecision, SceneSettingsInput,
    selection::SelectionBox,
    settings::SettingsStore,
    timeline_interaction::{TimelineDragKind, TimelineDragRequest},
};
use aviqtl_preview::{MediaPreview, PreviewPlanner, PreviewSurface};
use slint::wgpu_29::wgpu;
use slint::{
    CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

slint::include_modules!();

const VALIDATION_PROJECT: &[u8] = br#"{
    "version": 3,
    "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
    "scenes": [
        {"id": 1, "name": "Root", "duration": 1200, "gridMode": "Frame", "gridInterval": 10},
        {"id": 2, "name": "Scene 2", "duration": 600}
    ],
    "clips": [
        {"id": 1, "sceneId": 1, "type": "video", "start": 30, "duration": 180, "layer": 0},
        {"id": 2, "sceneId": 1, "type": "text", "start": 120, "duration": 100, "layer": 1},
        {"id": 3, "sceneId": 1, "type": "audio", "start": 60, "duration": 240, "layer": 3}
    ]
}"#;

#[derive(Clone, PartialEq, Eq)]
struct PreviewSourceKey {
    project_instance_id: u64,
    document_revision: u64,
    project_path: Option<PathBuf>,
}

#[derive(Clone, PartialEq, Eq)]
struct PreviewFrameKey {
    source: PreviewSourceKey,
    scene_id: i32,
    frame: i32,
}

struct PreviewRuntime {
    surface: PreviewSurface,
    decoder: MediaPreview,
    planner: Option<PreviewPlanner>,
    source_key: Option<PreviewSourceKey>,
    requested_frame: Option<PreviewFrameKey>,
}

impl PreviewRuntime {
    fn new(surface: PreviewSurface) -> Self {
        Self {
            surface,
            decoder: MediaPreview::new(Arc::new(|| {})),
            planner: None,
            source_key: None,
            requested_frame: None,
        }
    }

    fn update(&mut self, model: &ApplicationModel, main: &MainWindow) -> bool {
        let Some(project_instance_id) = model.current_project_instance_id() else {
            if self.source_key.is_some() || self.requested_frame.is_some() || self.planner.is_some()
            {
                self.decoder.reset();
                self.planner = None;
                self.source_key = None;
                self.requested_frame = None;
            }
            return false;
        };
        let Some(workspace) = model.current_workspace() else {
            return false;
        };
        let source_key = PreviewSourceKey {
            project_instance_id,
            document_revision: workspace.document_revision(),
            project_path: workspace.project().path.clone(),
        };
        if self.source_key.as_ref() != Some(&source_key) {
            if let Some(planner) = self.planner.as_mut() {
                planner.rebuild(workspace.document(), workspace.project().path.as_deref());
            } else {
                self.planner = Some(PreviewPlanner::new(
                    workspace.document(),
                    workspace.project().path.as_deref(),
                ));
            }
            self.source_key = Some(source_key.clone());
            self.requested_frame = None;
        }
        let frame_key = PreviewFrameKey {
            source: source_key,
            scene_id: workspace.selected_scene(),
            frame: workspace.playhead(),
        };
        if self.requested_frame.as_ref() != Some(&frame_key) {
            if let Some(planned) = self.planner.as_mut().and_then(|planner| {
                planner.build(
                    workspace.document(),
                    workspace.selected_scene(),
                    workspace.playhead(),
                )
            }) {
                for warning in planned.warnings {
                    eprintln!("Preview planning warning: {warning}");
                }
                self.decoder.request(planned.scene);
            }
            self.requested_frame = Some(frame_key);
        }
        let Some(batch) = self.decoder.poll() else {
            return false;
        };
        for error in &batch.errors {
            eprintln!("Preview decode warning: {error}");
        }
        let target_replaced = self.surface.compose(&batch.scene, batch.generation);
        if target_replaced {
            match slint::Image::try_from(self.surface.texture().clone()) {
                Ok(image) => main.set_preview_image(image),
                Err(error) => eprintln!("Preview texture import failed: {error}"),
            }
        }
        true
    }
}

struct LifecycleUi {
    launcher: slint::Weak<ProjectLauncherWindow>,
    recovery: slint::Weak<ProjectRecoveryWindow>,
    main: slint::Weak<MainWindow>,
    timeline: slint::Weak<TimelineWindow>,
    object_settings: slint::Weak<ObjectSettingsWindow>,
    project_settings: slint::Weak<ProjectSettingsWindow>,
    scene_settings: slint::Weak<SceneSettingsWindow>,
    system_settings: slint::Weak<SystemSettingsWindow>,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    quit_confirmed: Cell<bool>,
}

struct WindowRefs<'a> {
    launcher: &'a ProjectLauncherWindow,
    recovery: &'a ProjectRecoveryWindow,
    main: &'a MainWindow,
    timeline: &'a TimelineWindow,
    object_settings: &'a ObjectSettingsWindow,
    project_settings: &'a ProjectSettingsWindow,
    scene_settings: &'a SceneSettingsWindow,
    system_settings: &'a SystemSettingsWindow,
}

impl LifecycleUi {
    fn handle(&self, mut step: LifecycleStep) {
        loop {
            if !matches!(step, LifecycleStep::ConfirmSave { .. })
                && let Some(main) = self.main.upgrade()
            {
                main.set_save_confirmation_visible(false);
            }

            match step {
                LifecycleStep::None | LifecycleStep::Cancelled => return,
                LifecycleStep::ConfirmSave { project_name, .. } => {
                    self.sync();
                    if let Some(main) = self.main.upgrade() {
                        main.set_save_confirmation_project(project_name.into());
                        main.set_save_confirmation_visible(true);
                        let _ = main.show();
                    }
                    return;
                }
                LifecycleStep::ChooseSavePath { suggested_path, .. } => {
                    let path = choose_project_save_path(&suggested_path);
                    step = self.model.borrow_mut().complete_save_path(path.as_deref());
                }
                LifecycleStep::ProjectSaved { .. } => {
                    self.sync();
                    return;
                }
                LifecycleStep::ProjectClosed { .. } => {
                    self.sync();
                    if self.model.borrow().launcher_visible() {
                        if let Some(main) = self.main.upgrade() {
                            let _ = main.hide();
                        }
                        if let Some(launcher) = self.launcher.upgrade() {
                            sync_launcher_defaults(&launcher, &self.settings.borrow());
                            let _ = launcher.show();
                        }
                        self.show_recoveries_if_available();
                    }
                    return;
                }
                LifecycleStep::QuitReady => {
                    self.quit_confirmed.set(true);
                    self.hide_all_windows();
                    return;
                }
                LifecycleStep::SaveFailed { message, .. } => {
                    self.sync();
                    show_error_dialog(&message);
                    return;
                }
            }
        }
    }

    fn open_project_dialog(&self, from_launcher: bool) {
        if self.model.borrow().lifecycle_pending() {
            return;
        }
        let Some(path) = choose_project_to_open() else {
            return;
        };
        let result = self.model.borrow_mut().open_project(&path);
        match result {
            Ok(_) => {
                self.sync();
                if let Some(main) = self.main.upgrade() {
                    let _ = main.show();
                }
                if from_launcher && let Some(launcher) = self.launcher.upgrade() {
                    let _ = launcher.hide();
                }
                if from_launcher && let Some(recovery) = self.recovery.upgrade() {
                    let _ = recovery.hide();
                }
            }
            Err(message) => show_error_dialog(&message),
        }
    }

    fn sync(&self) {
        if let (Some(main), Some(timeline)) = (self.main.upgrade(), self.timeline.upgrade()) {
            sync_windows(&main, &timeline, &self.model.borrow());
        }
        if let Some(recovery) = self.recovery.upgrade() {
            sync_recovery_window(&recovery, &self.model.borrow());
        }
    }

    fn show_recoveries_if_available(&self) {
        let entries = self.model.borrow().recovery_entries();
        if entries.is_empty() {
            return;
        }
        if let Some(recovery) = self.recovery.upgrade() {
            sync_recovery_entries(&recovery, entries);
            recovery.set_error_message(SharedString::new());
            let _ = recovery.show();
        }
    }

    fn hide_all_windows(&self) {
        if let Some(window) = self.timeline.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.object_settings.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.project_settings.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.scene_settings.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.system_settings.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.launcher.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.recovery.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.main.upgrade() {
            let _ = window.hide();
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("AviQtl Slint failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let validation_frames = parse_validation_frames()?;
    let gpu = AppGpu::new()?;
    slint::BackendSelector::new()
        .require_wgpu_29(slint::wgpu_29::WGPUConfiguration::Manual {
            instance: gpu.instance.clone(),
            adapter: gpu.adapter.clone(),
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
        })
        .select()?;

    let preview_surface = PreviewSurface::new(gpu.device.clone(), gpu.queue.clone());
    let preview_image = slint::Image::try_from(preview_surface.texture().clone())?;
    let texture_imported = preview_image
        .to_wgpu_29_texture()
        .is_some_and(|texture| texture == *preview_surface.texture());
    let preview = Rc::new(RefCell::new(PreviewRuntime::new(preview_surface)));

    let (settings_store, settings_status) = SettingsStore::load();
    eprintln!("{settings_status}");
    let settings = Rc::new(RefCell::new(settings_store));
    let mut application_model = ApplicationModel::default();
    apply_runtime_settings(&mut application_model, &settings.borrow());
    let model = Rc::new(RefCell::new(application_model));
    let launcher = ProjectLauncherWindow::new()?;
    let recovery = ProjectRecoveryWindow::new()?;
    let main = MainWindow::new()?;
    let timeline = TimelineWindow::new()?;
    let object_settings = ObjectSettingsWindow::new()?;
    let project_settings = ProjectSettingsWindow::new()?;
    let scene_settings = SceneSettingsWindow::new()?;
    let system_settings = SystemSettingsWindow::new()?;
    sync_launcher_defaults(&launcher, &settings.borrow());
    sync_system_settings(&system_settings, &settings.borrow());
    main.set_preview_image(preview_image);
    main.set_project_tabs(ModelRc::new(VecModel::<ProjectTabData>::default()));
    recovery.set_recoveries(ModelRc::new(VecModel::<RecoveryEntryData>::default()));
    timeline.set_scene_tabs(ModelRc::new(VecModel::<SceneTabData>::default()));
    timeline.set_clips(ModelRc::new(VecModel::<TimelineClipData>::default()));
    timeline.set_layers(ModelRc::new(VecModel::<LayerData>::default()));

    let stats = Rc::new(GpuValidation::new(
        gpu.device.clone(),
        gpu.errors.clone(),
        texture_imported,
    ));
    install_render_probe(&main, stats.clone(), WindowKind::Main)?;
    install_render_probe(&timeline, stats.clone(), WindowKind::Timeline)?;
    let lifecycle_ui = Rc::new(LifecycleUi {
        launcher: launcher.as_weak(),
        recovery: recovery.as_weak(),
        main: main.as_weak(),
        timeline: timeline.as_weak(),
        object_settings: object_settings.as_weak(),
        project_settings: project_settings.as_weak(),
        scene_settings: scene_settings.as_weak(),
        system_settings: system_settings.as_weak(),
        model: model.clone(),
        settings: settings.clone(),
        quit_confirmed: Cell::new(false),
    });
    install_callbacks(
        WindowRefs {
            launcher: &launcher,
            recovery: &recovery,
            main: &main,
            timeline: &timeline,
            object_settings: &object_settings,
            project_settings: &project_settings,
            scene_settings: &scene_settings,
            system_settings: &system_settings,
        },
        model.clone(),
        settings,
        lifecycle_ui,
    );

    if validation_frames.is_some() {
        let project =
            ProjectSession::from_json(VALIDATION_PROJECT).map_err(std::io::Error::other)?;
        model.borrow_mut().add_project_session(project);
        sync_windows(&main, &timeline, &model.borrow());
        main.show()?;
        timeline.show()?;
    } else {
        sync_recovery_window(&recovery, &model.borrow());
        launcher.show()?;
        if !model.borrow().recovery_entries().is_empty() {
            recovery.show()?;
        }
    }

    let rendered_ticks = Rc::new(Cell::new(0_u64));
    let animation_timer = Timer::default();
    let animation_preview = preview.clone();
    let animation_main = main.as_weak();
    let animation_timeline = timeline.as_weak();
    let animation_launcher = launcher.as_weak();
    let animation_recovery = recovery.as_weak();
    let animation_settings = object_settings.as_weak();
    let animation_project_settings = project_settings.as_weak();
    let animation_scene_settings = scene_settings.as_weak();
    let animation_system_settings = system_settings.as_weak();
    let animation_stats = stats.clone();
    let animation_model = model.clone();
    let timer_ticks = rendered_ticks.clone();
    animation_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        let ticks = timer_ticks.get() + 1;
        timer_ticks.set(ticks);
        let transport_changed = animation_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.update_transport(Instant::now()));
        if transport_changed
            && let (Some(main), Some(timeline)) =
                (animation_main.upgrade(), animation_timeline.upgrade())
        {
            sync_transport(&main, &timeline, &animation_model.borrow());
        }
        if let Some(main) = animation_main.upgrade() {
            let preview_updated = {
                let model = animation_model.borrow();
                animation_preview.borrow_mut().update(&model, &main)
            };
            if preview_updated {
                animation_stats
                    .preview_updates
                    .set(animation_stats.preview_updates.get() + 1);
                main.window().request_redraw();
            }
        }
        if validation_frames.is_some_and(|frames| ticks >= frames) {
            if let Some(window) = animation_timeline.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_settings.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_project_settings.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_scene_settings.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_system_settings.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_launcher.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_recovery.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_main.upgrade() {
                let _ = window.hide();
            }
        }
    });

    let recovery_timer = Timer::default();
    let recovery_model = model.clone();
    let recovery_window = recovery.as_weak();
    recovery_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        let errors = recovery_model.borrow_mut().update_recovery();
        if errors.is_empty() {
            return;
        }
        for error in &errors {
            eprintln!("Project recovery failed: {error}");
        }
        if let Some(window) = recovery_window.upgrade() {
            sync_recovery_window(&window, &recovery_model.borrow());
            window.set_error_message(SharedString::from(errors.join("\n")));
        }
    });

    slint::run_event_loop()?;
    if validation_frames.is_some() {
        stats.print_and_validate()?;
    }
    Ok(())
}

fn install_callbacks(
    windows: WindowRefs<'_>,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    lifecycle_ui: Rc<LifecycleUi>,
) {
    let WindowRefs {
        launcher,
        recovery,
        main,
        timeline,
        object_settings,
        project_settings,
        scene_settings,
        system_settings,
    } = windows;
    let create_model = model.clone();
    let create_main = main.as_weak();
    let create_timeline = timeline.as_weak();
    let create_launcher = launcher.as_weak();
    let create_recovery = recovery.as_weak();
    let create_settings = settings.clone();
    launcher.on_create_project(move |width, height, fps, sample_rate| {
        if create_model.borrow().lifecycle_pending() {
            return;
        }
        let mut defaults = project_defaults(&create_settings.borrow());
        defaults.width = match parse_required_i32(&width, "幅", 1, 8_000) {
            Ok(value) => value,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        defaults.height = match parse_required_i32(&height, "高さ", 1, 8_000) {
            Ok(value) => value,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        defaults.fps = match parse_required_f64(&fps, "FPS", 1.0, 240.0) {
            Ok(value) => value,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        defaults.sample_rate =
            match parse_required_i32(&sample_rate, "サンプリングレート", 8_000, 192_000) {
                Ok(value) => value,
                Err(message) => {
                    show_error_dialog(&message);
                    return;
                }
            };
        create_model.borrow_mut().create_project(defaults);
        sync_weak_windows(&create_main, &create_timeline, &create_model);
        if let Some(main) = create_main.upgrade() {
            let _ = main.show();
        }
        if let Some(window) = create_launcher.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = create_recovery.upgrade() {
            let _ = window.hide();
        }
    });

    let launcher_open_ui = lifecycle_ui.clone();
    launcher.on_open_project(move || launcher_open_ui.open_project_dialog(true));

    let new_launcher = launcher.as_weak();
    let new_ui = lifecycle_ui.clone();
    let new_model = model.clone();
    main.on_new_project(move || {
        if new_model.borrow().lifecycle_pending() {
            return;
        }
        if let Some(window) = new_launcher.upgrade() {
            let _ = window.show();
        }
        new_ui.show_recoveries_if_available();
    });

    let main_open_ui = lifecycle_ui.clone();
    main.on_open_project(move || main_open_ui.open_project_dialog(false));

    let save_ui = lifecycle_ui.clone();
    main.on_save_project(move || {
        let step = save_ui.model.borrow_mut().request_save_current_project();
        save_ui.handle(step);
    });

    let save_as_ui = lifecycle_ui.clone();
    main.on_save_project_as(move || {
        let step = save_as_ui
            .model
            .borrow_mut()
            .request_save_current_project_as();
        save_as_ui.handle(step);
    });

    let quit_ui = lifecycle_ui.clone();
    main.on_quit_requested(move || {
        let confirm_unsaved = quit_ui
            .settings
            .borrow()
            .bool_value("showConfirmOnClose", true);
        let step = quit_ui.model.borrow_mut().request_quit(confirm_unsaved);
        quit_ui.handle(step);
    });

    let close_window_ui = lifecycle_ui.clone();
    main.window().on_close_requested(move || {
        if close_window_ui.quit_confirmed.get() {
            return CloseRequestResponse::HideWindow;
        }
        let confirm_unsaved = close_window_ui
            .settings
            .borrow()
            .bool_value("showConfirmOnClose", true);
        let step = close_window_ui
            .model
            .borrow_mut()
            .request_quit(confirm_unsaved);
        let quit_ready = matches!(step, LifecycleStep::QuitReady);
        close_window_ui.handle(step);
        if quit_ready {
            CloseRequestResponse::HideWindow
        } else {
            CloseRequestResponse::KeepWindowShown
        }
    });

    let confirm_save_ui = lifecycle_ui.clone();
    main.on_confirm_save(move || {
        let step = confirm_save_ui
            .model
            .borrow_mut()
            .answer_save_confirmation(SaveDecision::Save);
        confirm_save_ui.handle(step);
    });
    let confirm_discard_ui = lifecycle_ui.clone();
    main.on_confirm_discard(move || {
        let step = confirm_discard_ui
            .model
            .borrow_mut()
            .answer_save_confirmation(SaveDecision::Discard);
        confirm_discard_ui.handle(step);
    });
    let confirm_cancel_ui = lifecycle_ui.clone();
    main.on_confirm_cancel(move || {
        let step = confirm_cancel_ui
            .model
            .borrow_mut()
            .answer_save_confirmation(SaveDecision::Cancel);
        confirm_cancel_ui.handle(step);
    });

    let select_model = model.clone();
    let select_main = main.as_weak();
    let select_timeline = timeline.as_weak();
    main.on_select_project(move |index| {
        if select_model
            .borrow_mut()
            .select_project(index.max(0) as usize)
        {
            sync_weak_windows(&select_main, &select_timeline, &select_model);
        }
    });

    let close_ui = lifecycle_ui.clone();
    main.on_close_project(move |index| {
        let step = close_ui
            .model
            .borrow_mut()
            .request_close_project(index.max(0) as usize);
        close_ui.handle(step);
    });

    let recover_ui = lifecycle_ui.clone();
    recovery.on_recover_project(move |id| {
        let recover_ui = recover_ui.clone();
        let id = id.to_string();
        Timer::single_shot(Duration::ZERO, move || {
            let result = recover_ui.model.borrow_mut().recover_project(&id);
            match result {
                Ok(_) => {
                    if let Some(window) = recover_ui.recovery.upgrade() {
                        let _ = window.hide();
                    }
                    recover_ui.sync();
                    if let Some(window) = recover_ui.main.upgrade() {
                        let _ = window.show();
                    }
                    if let Some(window) = recover_ui.launcher.upgrade() {
                        let _ = window.hide();
                    }
                }
                Err(message) => {
                    if let Some(window) = recover_ui.recovery.upgrade() {
                        window.set_error_message(SharedString::from(message));
                    }
                }
            }
        });
    });

    let discard_ui = lifecycle_ui;
    recovery.on_discard_recovery(move |id| {
        let discard_ui = discard_ui.clone();
        let id = id.to_string();
        Timer::single_shot(Duration::ZERO, move || {
            let result = discard_ui.model.borrow_mut().discard_recovery(&id);
            if let Some(window) = discard_ui.recovery.upgrade() {
                match result {
                    Ok(()) => {
                        sync_recovery_window(&window, &discard_ui.model.borrow());
                        if discard_ui.model.borrow().recovery_entries().is_empty() {
                            let _ = window.hide();
                        }
                    }
                    Err(message) => window.set_error_message(SharedString::from(message)),
                }
            }
        });
    });

    let recovery_window = recovery.as_weak();
    recovery.on_close_window(move || {
        if let Some(window) = recovery_window.upgrade() {
            window.set_error_message(SharedString::new());
            window.set_discard_confirmation_visible(false);
            let _ = window.hide();
        }
    });

    let timeline_window = timeline.as_weak();
    main.on_show_timeline(move || {
        if let Some(window) = timeline_window.upgrade() {
            let _ = window.show();
        }
    });
    let settings_window = object_settings.as_weak();
    main.on_show_object_settings(move || {
        if let Some(window) = settings_window.upgrade() {
            let _ = window.show();
        }
    });

    let project_settings_model = model.clone();
    let project_settings_window = project_settings.as_weak();
    main.on_show_project_settings(move || {
        let input = project_settings_model
            .borrow()
            .current_workspace()
            .map(|workspace| workspace.project_settings());
        if let (Some(window), Some(input)) = (project_settings_window.upgrade(), input) {
            sync_project_settings(&window, &input);
            let _ = window.show();
        }
    });

    let system_settings_store = settings.clone();
    let system_settings_window = system_settings.as_weak();
    main.on_show_system_settings(move || {
        if let Some(window) = system_settings_window.upgrade() {
            sync_system_settings(&window, &system_settings_store.borrow());
            let _ = window.show();
        }
    });

    let project_apply_model = model.clone();
    let project_apply_main = main.as_weak();
    let project_apply_timeline = timeline.as_weak();
    let project_apply_window = project_settings.as_weak();
    project_settings.on_apply_settings(move || {
        let Some(window) = project_apply_window.upgrade() else {
            return false;
        };
        let fps = match parse_required_f64(&window.get_project_fps(), "FPS", 1.0, 240.0) {
            Ok(fps) => fps,
            Err(message) => {
                show_error_dialog(&message);
                return false;
            }
        };
        let input = ProjectSettingsInput {
            width: window.get_project_width(),
            height: window.get_project_height(),
            fps,
            sample_rate: window.get_project_sample_rate(),
        };
        let applied = project_apply_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.update_project_settings(input));
        if applied {
            sync_weak_windows(
                &project_apply_main,
                &project_apply_timeline,
                &project_apply_model,
            );
        }
        applied
    });
    let project_close_window = project_settings.as_weak();
    project_settings.on_close_window(move || {
        if let Some(window) = project_close_window.upgrade() {
            let _ = window.hide();
        }
    });

    let scene_create_model = model.clone();
    let scene_create_settings = settings.clone();
    let scene_create_window = scene_settings.as_weak();
    timeline.on_show_scene_settings(move || {
        let input = {
            let application = scene_create_model.borrow();
            application.current_workspace().map(|workspace| {
                let project = workspace.project_settings();
                SceneSettingsInput {
                    name: format!("シーン {}", workspace.document().scenes.len() + 1),
                    width: project.width,
                    height: project.height,
                    fps: project.fps,
                    duration: scene_create_settings
                        .borrow()
                        .i32_value("defaultProjectFrames", 3_600)
                        .max(1),
                    grid_mode: "Auto".to_owned(),
                    grid_bpm: 120.0,
                    grid_offset: 0.0,
                    grid_interval: 10,
                    grid_subdivision: 4,
                    enable_snap: true,
                    magnetic_snap_range: 10,
                }
            })
        };
        if let (Some(window), Some(input)) = (scene_create_window.upgrade(), input) {
            sync_scene_settings(&window, true, -1, &input);
            let _ = window.show();
        }
    });

    let timeline_settings = object_settings.as_weak();
    timeline.on_show_object_settings(move || {
        if let Some(window) = timeline_settings.upgrade() {
            let _ = window.show();
        }
    });

    let timeline_action_model = model.clone();
    let timeline_action_main = main.as_weak();
    let timeline_action_window = timeline.as_weak();
    timeline.on_timeline_action(move |action| {
        if let Some(workspace) = timeline_action_model.borrow_mut().current_workspace_mut() {
            match action.as_str() {
                "undo" => {
                    workspace.undo();
                }
                "redo" => {
                    workspace.redo();
                }
                "paste" => {
                    let frame = workspace.playhead();
                    let layer = workspace.selected_layer();
                    workspace.paste_clips_at(frame, layer);
                }
                _ => {}
            }
        }
        sync_weak_windows(
            &timeline_action_main,
            &timeline_action_window,
            &timeline_action_model,
        );
    });

    let scene_model = model.clone();
    let scene_main = main.as_weak();
    let scene_timeline = timeline.as_weak();
    timeline.on_scene_selected(move |scene_id| {
        if let Some(workspace) = scene_model.borrow_mut().current_workspace_mut() {
            workspace.switch_scene(scene_id);
        }
        sync_weak_windows(&scene_main, &scene_timeline, &scene_model);
    });

    let close_scene_model = model.clone();
    let close_scene_main = main.as_weak();
    let close_scene_timeline = timeline.as_weak();
    timeline.on_scene_closed(move |scene_id| {
        if let Some(workspace) = close_scene_model.borrow_mut().current_workspace_mut() {
            workspace.remove_scene(scene_id);
        }
        sync_weak_windows(&close_scene_main, &close_scene_timeline, &close_scene_model);
    });

    let scene_edit_model = model.clone();
    let scene_edit_window = scene_settings.as_weak();
    timeline.on_scene_settings(move |scene_id| {
        let input = scene_edit_model
            .borrow()
            .current_workspace()
            .and_then(|workspace| workspace.scene_settings(scene_id));
        if let (Some(window), Some(input)) = (scene_edit_window.upgrade(), input) {
            sync_scene_settings(&window, false, scene_id, &input);
            let _ = window.show();
        }
    });

    let scene_apply_model = model.clone();
    let scene_apply_main = main.as_weak();
    let scene_apply_timeline = timeline.as_weak();
    let scene_apply_window = scene_settings.as_weak();
    let scene_apply_settings = settings.clone();
    scene_settings.on_apply_settings(move || {
        let Some(window) = scene_apply_window.upgrade() else {
            return false;
        };
        let fps = match parse_required_f64(&window.get_scene_fps(), "FPS", 1.0, 240.0) {
            Ok(fps) => fps,
            Err(message) => {
                show_error_dialog(&message);
                return false;
            }
        };
        let grid_mode = match window.get_grid_mode_index() {
            1 => "BPM",
            2 => "Frame",
            _ => "Auto",
        };
        let input = SceneSettingsInput {
            name: window.get_scene_name().to_string(),
            width: window.get_scene_width(),
            height: window.get_scene_height(),
            fps,
            duration: window.get_scene_duration(),
            grid_mode: grid_mode.to_owned(),
            grid_bpm: parse_finite_f64(&window.get_grid_bpm(), 120.0),
            grid_offset: parse_finite_f64(&window.get_grid_offset(), 0.0),
            grid_interval: parse_i32_unbounded(&window.get_grid_interval(), 10),
            grid_subdivision: parse_i32_unbounded(&window.get_grid_subdivision(), 4),
            enable_snap: window.get_enable_snap(),
            magnetic_snap_range: window.get_magnetic_snap_range(),
        };
        let applied = if window.get_creation_mode() {
            let defaults = project_defaults(&scene_apply_settings.borrow());
            scene_apply_model
                .borrow_mut()
                .current_workspace_mut()
                .and_then(|workspace| workspace.create_scene(defaults, input))
                .is_some()
        } else {
            let scene_id = window.get_target_scene_id();
            scene_apply_model
                .borrow_mut()
                .current_workspace_mut()
                .is_some_and(|workspace| workspace.update_scene_settings(scene_id, input))
        };
        if applied {
            sync_weak_windows(&scene_apply_main, &scene_apply_timeline, &scene_apply_model);
        }
        applied
    });
    let scene_close_window = scene_settings.as_weak();
    scene_settings.on_close_window(move || {
        if let Some(window) = scene_close_window.upgrade() {
            let _ = window.hide();
        }
    });

    let system_reload_store = settings.clone();
    let system_reload_window = system_settings.as_weak();
    system_settings.on_reload_settings(move || {
        if let Some(window) = system_reload_window.upgrade() {
            sync_system_settings(&window, &system_reload_store.borrow());
        }
    });
    let system_apply_store = settings.clone();
    let system_apply_model = model.clone();
    let system_apply_launcher = launcher.as_weak();
    let system_apply_window = system_settings.as_weak();
    system_settings.on_apply_settings(move || {
        let Some(window) = system_apply_window.upgrade() else {
            return false;
        };
        match apply_system_settings(&window, &system_apply_store, &system_apply_model) {
            Ok(()) => {
                sync_system_settings(&window, &system_apply_store.borrow());
                if let Some(launcher) = system_apply_launcher.upgrade() {
                    sync_launcher_defaults(&launcher, &system_apply_store.borrow());
                }
                true
            }
            Err(message) => {
                show_error_dialog(&message);
                false
            }
        }
    });
    let system_close_window = system_settings.as_weak();
    system_settings.on_close_window(move || {
        if let Some(window) = system_close_window.upgrade() {
            let _ = window.hide();
        }
    });

    let clip_select_model = model.clone();
    let clip_select_main = main.as_weak();
    let clip_select_timeline = timeline.as_weak();
    timeline.on_clip_selected(move |clip_id, additive| {
        if let Some(workspace) = clip_select_model.borrow_mut().current_workspace_mut() {
            workspace.click_clip(clip_id, additive);
        }
        sync_weak_windows(&clip_select_main, &clip_select_timeline, &clip_select_model);
    });

    let clip_drag_start_model = model.clone();
    let clip_drag_start_main = main.as_weak();
    let clip_drag_start_timeline = timeline.as_weak();
    timeline.on_clip_drag_started(move |clip_id, additive| {
        if let Some(workspace) = clip_drag_start_model.borrow_mut().current_workspace_mut() {
            workspace.prepare_clip_drag(clip_id, additive);
        }
        sync_weak_windows(
            &clip_drag_start_main,
            &clip_drag_start_timeline,
            &clip_drag_start_model,
        );
    });

    let clip_context_model = model.clone();
    let clip_context_main = main.as_weak();
    let clip_context_timeline = timeline.as_weak();
    timeline.on_clip_context_selected(move |clip_id| {
        if let Some(workspace) = clip_context_model.borrow_mut().current_workspace_mut() {
            workspace.context_click_clip(clip_id);
        }
        sync_weak_windows(
            &clip_context_main,
            &clip_context_timeline,
            &clip_context_model,
        );
    });

    let clip_command_model = model.clone();
    let clip_command_main = main.as_weak();
    let clip_command_timeline = timeline.as_weak();
    let clip_command_settings = object_settings.as_weak();
    timeline.on_clip_command(move |action, clip_id| {
        if let Some(workspace) = clip_command_model.borrow_mut().current_workspace_mut() {
            workspace.context_click_clip(clip_id);
            match action.as_str() {
                "delete" => {
                    workspace.remove_selected_clips();
                }
                "split" => {
                    workspace.split_selected_clips_at(workspace.playhead());
                }
                "duplicate" => {
                    workspace.duplicate_selected_clips_at(
                        workspace.playhead(),
                        workspace.selected_layer(),
                    );
                }
                "cut" => {
                    workspace.cut_selected_clips();
                }
                "copy" => {
                    workspace.copy_selected_clips();
                }
                "clipping" => {
                    workspace.toggle_clip_by_upper_object(clip_id);
                }
                "add-effect" => {
                    if let Some(window) = clip_command_settings.upgrade() {
                        let _ = window.show();
                    }
                }
                _ => {}
            }
        }
        sync_weak_windows(
            &clip_command_main,
            &clip_command_timeline,
            &clip_command_model,
        );
    });

    let clip_drag_model = model.clone();
    let clip_drag_main = main.as_weak();
    let clip_drag_timeline = timeline.as_weak();
    timeline.on_clip_drag_finished(move |kind, clip_id, delta_x, delta_y, ignore_snap| {
        let kind = match kind.as_str() {
            "trim-start" => TimelineDragKind::TrimStart,
            "trim-end" => TimelineDragKind::TrimEnd,
            _ => TimelineDragKind::Move,
        };
        let pixels_per_frame = clip_drag_timeline
            .upgrade()
            .map_or(1.0, |window| window.get_pixels_per_frame());
        if let Some(workspace) = clip_drag_model.borrow_mut().current_workspace_mut() {
            workspace.drag_selected_clips(TimelineDragRequest {
                anchor_clip_id: clip_id,
                kind,
                delta_pixels: (delta_x, delta_y),
                pixels_per_frame,
                layer_height: 30.0,
                minimum_duration_frames: 5,
                ignore_snap,
            });
        }
        sync_weak_windows(&clip_drag_main, &clip_drag_timeline, &clip_drag_model);
    });

    let layer_model = model.clone();
    let layer_main = main.as_weak();
    let layer_timeline = timeline.as_weak();
    timeline.on_layer_activated(move |layer| {
        if let Some(workspace) = layer_model.borrow_mut().current_workspace_mut() {
            workspace.select_layer(layer);
            workspace.toggle_layer_visibility(layer);
        }
        sync_weak_windows(&layer_main, &layer_timeline, &layer_model);
    });

    let layer_command_model = model.clone();
    let layer_command_main = main.as_weak();
    let layer_command_timeline = timeline.as_weak();
    timeline.on_layer_command(move |action, layer| {
        if let Some(workspace) = layer_command_model.borrow_mut().current_workspace_mut() {
            match action.as_str() {
                "insert-above" => {
                    workspace.insert_layers(layer, 1, true);
                }
                "insert-below" => {
                    workspace.insert_layers(layer, 1, false);
                }
                "toggle-lock" => {
                    workspace.toggle_layer_lock(layer);
                }
                "toggle-visible" => {
                    workspace.toggle_layer_visibility(layer);
                }
                "show-all" => {
                    workspace.set_all_layers_visible(true);
                }
                "hide-all" => {
                    workspace.set_all_layers_visible(false);
                }
                _ => {}
            }
        }
        sync_weak_windows(
            &layer_command_main,
            &layer_command_timeline,
            &layer_command_model,
        );
    });

    let empty_model = model.clone();
    let empty_main = main.as_weak();
    let empty_timeline = timeline.as_weak();
    timeline.on_empty_clicked(move |frame, layer| {
        if let Some(workspace) = empty_model.borrow_mut().current_workspace_mut() {
            workspace.select_layer(layer);
            workspace.seek(frame);
        }
        sync_weak_windows(&empty_main, &empty_timeline, &empty_model);
    });

    let box_state = Rc::new(RefCell::new(None::<SelectionBox>));
    let box_start_state = box_state.clone();
    timeline.on_box_selection_started(move |frame, layer, additive| {
        *box_start_state.borrow_mut() = Some(SelectionBox {
            frame_a: frame,
            frame_b: frame,
            layer_a: layer,
            layer_b: layer,
            additive,
        });
    });
    let box_update_state = box_state.clone();
    let box_update_model = model.clone();
    let box_update_main = main.as_weak();
    let box_update_timeline = timeline.as_weak();
    timeline.on_box_selection_updated(move |frame, layer| {
        let selection_box = {
            let mut state = box_update_state.borrow_mut();
            let Some(selection_box) = state.as_mut() else {
                return;
            };
            selection_box.frame_b = frame;
            selection_box.layer_b = layer;
            *selection_box
        };
        if let Some(workspace) = box_update_model.borrow_mut().current_workspace_mut() {
            workspace.preview_box_selection(selection_box);
        }
        sync_weak_windows(&box_update_main, &box_update_timeline, &box_update_model);
    });
    let box_finish_state = box_state.clone();
    let box_finish_model = model.clone();
    let box_finish_main = main.as_weak();
    let box_finish_timeline = timeline.as_weak();
    timeline.on_box_selection_finished(move || {
        box_finish_state.borrow_mut().take();
        if let Some(workspace) = box_finish_model.borrow_mut().current_workspace_mut() {
            workspace.finish_box_selection();
        }
        sync_weak_windows(&box_finish_main, &box_finish_timeline, &box_finish_model);
    });
    let box_cancel_state = box_state;
    let box_cancel_model = model.clone();
    let box_cancel_main = main.as_weak();
    let box_cancel_timeline = timeline.as_weak();
    timeline.on_box_selection_cancelled(move || {
        box_cancel_state.borrow_mut().take();
        if let Some(workspace) = box_cancel_model.borrow_mut().current_workspace_mut() {
            workspace.cancel_box_selection();
        }
        sync_weak_windows(&box_cancel_main, &box_cancel_timeline, &box_cancel_model);
    });

    let undo_model = model.clone();
    let undo_main = main.as_weak();
    let undo_timeline = timeline.as_weak();
    main.on_undo(move || {
        if let Some(workspace) = undo_model.borrow_mut().current_workspace_mut() {
            workspace.undo();
        }
        sync_weak_windows(&undo_main, &undo_timeline, &undo_model);
    });
    let redo_model = model.clone();
    let redo_main = main.as_weak();
    let redo_timeline = timeline.as_weak();
    main.on_redo(move || {
        if let Some(workspace) = redo_model.borrow_mut().current_workspace_mut() {
            workspace.redo();
        }
        sync_weak_windows(&redo_main, &redo_timeline, &redo_model);
    });

    let seek_model = model.clone();
    let seek_main = main.as_weak();
    let seek_timeline = timeline.as_weak();
    main.on_seek(move |frame| {
        if let Some(workspace) = seek_model.borrow_mut().current_workspace_mut() {
            workspace.seek(frame.round() as i32);
        }
        sync_transport_weak(&seek_main, &seek_timeline, &seek_model);
    });
    let previous_model = model.clone();
    let previous_main = main.as_weak();
    let previous_timeline = timeline.as_weak();
    main.on_previous_frame(move || {
        if let Some(workspace) = previous_model.borrow_mut().current_workspace_mut() {
            workspace.step_playhead(-1);
        }
        sync_transport_weak(&previous_main, &previous_timeline, &previous_model);
    });
    let next_model = model.clone();
    let next_main = main.as_weak();
    let next_timeline = timeline.as_weak();
    main.on_next_frame(move || {
        if let Some(workspace) = next_model.borrow_mut().current_workspace_mut() {
            workspace.step_playhead(1);
        }
        sync_transport_weak(&next_main, &next_timeline, &next_model);
    });
    let playback_model = model.clone();
    let playback_main = main.as_weak();
    let playback_timeline = timeline.as_weak();
    main.on_toggle_playback(move || {
        if let Some(workspace) = playback_model.borrow_mut().current_workspace_mut() {
            workspace.toggle_playback();
        }
        sync_transport_weak(&playback_main, &playback_timeline, &playback_model);
    });
}

fn choose_project_to_open() -> Option<PathBuf> {
    project_file_dialog().pick_file()
}

fn choose_project_save_path(suggested_path: &Path) -> Option<PathBuf> {
    let mut dialog = project_file_dialog();
    if let Some(file_name) = suggested_path.file_name() {
        dialog = dialog.set_file_name(file_name.to_string_lossy().into_owned());
    }
    if let Some(parent) = suggested_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        dialog = dialog.set_directory(parent);
    }
    let mut path = dialog.save_file()?;
    if path.extension().is_none() {
        path.set_extension("aviqtl");
    }
    Some(path)
}

fn project_file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("AviQtl Plus Project files", &["aviqtl"])
        .add_filter("JSON files", &["json"])
}

fn show_error_dialog(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("AviQtl Plus")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn project_defaults(settings: &SettingsStore) -> ProjectDefaults {
    let fps = settings.f64_value("defaultProjectFps", 60.0);
    ProjectDefaults {
        width: settings
            .i32_value("defaultProjectWidth", 1_920)
            .clamp(1, 16_000),
        height: settings
            .i32_value("defaultProjectHeight", 1_080)
            .clamp(1, 16_000),
        fps: if fps.is_finite() {
            fps.clamp(1.0, 240.0)
        } else {
            60.0
        },
        sample_rate: settings
            .i32_value("defaultProjectSampleRate", 48_000)
            .clamp(8_000, 192_000),
        duration: settings
            .i32_value("defaultProjectFrames", 3_600)
            .clamp(1, 1_000_000),
        enable_snap: settings.bool_value("enableSnap", true),
        magnetic_snap_range: settings.i32_value("magneticSnapRange", 10).clamp(1, 100),
    }
}

fn sync_launcher_defaults(window: &ProjectLauncherWindow, settings: &SettingsStore) {
    let defaults = project_defaults(settings);
    window.set_default_width(SharedString::from(defaults.width.to_string()));
    window.set_default_height(SharedString::from(defaults.height.to_string()));
    window.set_default_fps(SharedString::from(defaults.fps.to_string()));
    window.set_default_sample_rate(SharedString::from(defaults.sample_rate.to_string()));
}

fn sync_project_settings(window: &ProjectSettingsWindow, input: &ProjectSettingsInput) {
    window.set_project_width(input.width);
    window.set_project_height(input.height);
    window.set_project_fps(SharedString::from(input.fps.to_string()));
    window.set_project_sample_rate(input.sample_rate);
}

fn sync_scene_settings(
    window: &SceneSettingsWindow,
    creation_mode: bool,
    scene_id: i32,
    input: &SceneSettingsInput,
) {
    window.set_creation_mode(creation_mode);
    window.set_target_scene_id(scene_id);
    window.set_scene_name(SharedString::from(input.name.clone()));
    window.set_scene_width(input.width);
    window.set_scene_height(input.height);
    window.set_scene_fps(SharedString::from(input.fps.to_string()));
    window.set_scene_duration(input.duration);
    window.set_grid_mode_index(match input.grid_mode.as_str() {
        "BPM" => 1,
        "Frame" => 2,
        _ => 0,
    });
    window.set_grid_bpm(SharedString::from(input.grid_bpm.to_string()));
    window.set_grid_offset(SharedString::from(input.grid_offset.to_string()));
    window.set_grid_interval(SharedString::from(input.grid_interval.to_string()));
    window.set_grid_subdivision(SharedString::from(input.grid_subdivision.to_string()));
    window.set_enable_snap(input.enable_snap);
    window.set_magnetic_snap_range(input.magnetic_snap_range);
}

fn sync_system_settings(window: &SystemSettingsWindow, settings: &SettingsStore) {
    let defaults = project_defaults(settings);
    window.set_confirm_unsaved(settings.bool_value("showConfirmOnClose", true));
    window.set_auto_backup(settings.bool_value("enableAutoBackup", true));
    window.set_backup_interval(settings.i32_value("backupInterval", 5).clamp(1, 60));
    window.set_undo_count(settings.i32_value("undoCount", 32).clamp(1, 1_000));
    window.set_default_project_width(defaults.width);
    window.set_default_project_height(defaults.height);
    window.set_default_project_fps(SharedString::from(defaults.fps.to_string()));
    window.set_default_project_frames(defaults.duration);
    window.set_default_project_sample_rate(defaults.sample_rate);
}

fn apply_runtime_settings(model: &mut ApplicationModel, settings: &SettingsStore) {
    let backup_minutes = settings.i32_value("backupInterval", 5).clamp(1, 24 * 60) as u64;
    model.set_recovery_interval(Duration::from_secs(backup_minutes * 60));
    model.set_auto_backup_enabled(settings.bool_value("enableAutoBackup", true));
    model.set_undo_limit(settings.i32_value("undoCount", 32).max(1) as usize);
}

fn apply_system_settings(
    window: &SystemSettingsWindow,
    settings: &Rc<RefCell<SettingsStore>>,
    model: &Rc<RefCell<ApplicationModel>>,
) -> Result<(), String> {
    let fps = parse_required_f64(
        &window.get_default_project_fps(),
        "既定のフレームレート",
        1.0,
        240.0,
    )?;
    let mut replacement = settings.borrow().snapshot();
    replacement.insert(
        "showConfirmOnClose".to_owned(),
        serde_json::json!(window.get_confirm_unsaved()),
    );
    replacement.insert(
        "enableAutoBackup".to_owned(),
        serde_json::json!(window.get_auto_backup()),
    );
    replacement.insert(
        "backupInterval".to_owned(),
        serde_json::json!(window.get_backup_interval().clamp(1, 60)),
    );
    replacement.insert(
        "undoCount".to_owned(),
        serde_json::json!(window.get_undo_count().clamp(1, 1_000)),
    );
    replacement.insert(
        "defaultProjectWidth".to_owned(),
        serde_json::json!(window.get_default_project_width().clamp(1, 16_000)),
    );
    replacement.insert(
        "defaultProjectHeight".to_owned(),
        serde_json::json!(window.get_default_project_height().clamp(1, 16_000)),
    );
    replacement.insert("defaultProjectFps".to_owned(), serde_json::json!(fps));
    replacement.insert(
        "defaultProjectFrames".to_owned(),
        serde_json::json!(window.get_default_project_frames().clamp(1, 1_000_000)),
    );
    replacement.insert(
        "defaultProjectSampleRate".to_owned(),
        serde_json::json!(
            window
                .get_default_project_sample_rate()
                .clamp(8_000, 192_000)
        ),
    );
    settings.borrow_mut().apply(replacement)?;
    apply_runtime_settings(&mut model.borrow_mut(), &settings.borrow());
    Ok(())
}

fn sync_weak_windows(
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
) {
    if let (Some(main), Some(timeline)) = (main.upgrade(), timeline.upgrade()) {
        sync_windows(&main, &timeline, &model.borrow());
    }
}

fn sync_transport_weak(
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
) {
    if let (Some(main), Some(timeline)) = (main.upgrade(), timeline.upgrade()) {
        sync_transport(&main, &timeline, &model.borrow());
    }
}

fn sync_windows(main: &MainWindow, timeline: &TimelineWindow, model: &ApplicationModel) {
    let tabs = model
        .tabs()
        .into_iter()
        .map(|tab| ProjectTabData {
            name: SharedString::from(tab.name),
            dirty: tab.dirty,
        })
        .collect::<Vec<_>>();
    update_vec_model(&main.get_project_tabs(), tabs);
    main.set_current_project(
        model
            .current_project_index()
            .map_or(-1, |index| index as i32),
    );

    let Some(workspace) = model.current_workspace() else {
        update_vec_model(&timeline.get_scene_tabs(), Vec::new());
        update_vec_model(&timeline.get_clips(), Vec::new());
        update_vec_model(&timeline.get_layers(), Vec::new());
        return;
    };
    let scenes = workspace
        .scene_tabs()
        .into_iter()
        .map(|scene| SceneTabData {
            id: scene.id,
            name: SharedString::from(scene.name),
            selected: scene.selected,
            root: scene.root,
        })
        .collect::<Vec<_>>();
    update_vec_model(&timeline.get_scene_tabs(), scenes);
    let clips = workspace
        .timeline_clips()
        .into_iter()
        .map(|clip| TimelineClipData {
            id: clip.id,
            label: SharedString::from(clip.label),
            start: clip.start,
            duration: clip.duration,
            layer: clip.layer,
            selected: clip.selected,
            primary: clip.primary,
        })
        .collect::<Vec<_>>();
    update_vec_model(&timeline.get_clips(), clips);
    let selected_layer = workspace.selected_layer();
    let scene = workspace.selected_scene_document();
    let layers = (0..128)
        .map(|index| LayerData {
            index,
            visible: scene.is_none_or(|scene| !scene.hidden_layers.contains(&index)),
            locked: scene.is_some_and(|scene| scene.locked_layers.contains(&index)),
            selected: index == selected_layer,
        })
        .collect::<Vec<_>>();
    update_vec_model(&timeline.get_layers(), layers);
    sync_transport(main, timeline, model);
}

fn sync_recovery_window(window: &ProjectRecoveryWindow, model: &ApplicationModel) {
    sync_recovery_entries(window, model.recovery_entries());
}

fn sync_recovery_entries(window: &ProjectRecoveryWindow, entries: Vec<aviqtl_app::RecoveryEntry>) {
    let entries = entries
        .into_iter()
        .map(|entry| {
            let valid = entry.is_valid();
            RecoveryEntryData {
                id: SharedString::from(entry.id),
                name: SharedString::from(if entry.display_name.is_empty() {
                    "Recovered project".to_owned()
                } else {
                    entry.display_name
                }),
                saved_at: SharedString::from(entry.saved_at),
                original_project_url: SharedString::from(entry.original_project_url),
                valid,
                error: SharedString::from(entry.error.unwrap_or_default()),
            }
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_recoveries(), entries);
}

fn update_vec_model<T: Clone + 'static>(model: &ModelRc<T>, rows: Vec<T>) {
    let model = model
        .as_any()
        .downcast_ref::<VecModel<T>>()
        .expect("UI list properties are initialized with VecModel");
    let common_rows = model.row_count().min(rows.len());
    for (index, row) in rows.iter().take(common_rows).cloned().enumerate() {
        model.set_row_data(index, row);
    }
    if rows.len() > common_rows {
        model.extend(rows.into_iter().skip(common_rows));
    } else {
        while model.row_count() > rows.len() {
            model.remove(rows.len());
        }
    }
}

fn sync_transport(main: &MainWindow, timeline: &TimelineWindow, model: &ApplicationModel) {
    let Some(workspace) = model.current_workspace() else {
        return;
    };
    let duration = workspace
        .selected_scene_document()
        .map_or(1, |scene| scene.duration.max(1));
    main.set_playhead(workspace.playhead() as f32);
    main.set_duration(duration as f32);
    main.set_playing(workspace.is_playing());
    main.set_status_text(SharedString::from(workspace.status()));
    timeline.set_playhead(workspace.playhead());
    timeline.set_duration(duration);
    timeline.set_selected_layer(workspace.selected_layer());
    timeline.set_action_status(SharedString::from(workspace.status()));
}

fn parse_required_i32(value: &str, label: &str, minimum: i32, maximum: i32) -> Result<i32, String> {
    let parsed = value
        .trim()
        .parse::<i32>()
        .map_err(|_| format!("{label}には整数を入力してください"))?;
    if (minimum..=maximum).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!(
            "{label}は{minimum}から{maximum}の範囲で入力してください"
        ))
    }
}

fn parse_required_f64(value: &str, label: &str, minimum: f64, maximum: f64) -> Result<f64, String> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("{label}には数値を入力してください"))?;
    if parsed.is_finite() && (minimum..=maximum).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!(
            "{label}は{minimum}から{maximum}の範囲で入力してください"
        ))
    }
}

fn parse_finite_f64(value: &str, fallback: f64) -> f64 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

fn parse_i32_unbounded(value: &str, fallback: i32) -> i32 {
    value.trim().parse::<i32>().unwrap_or(fallback)
}

fn parse_validation_frames() -> Result<Option<u64>, String> {
    let mut args = std::env::args().skip(1);
    let mut frames = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--validate-gpu" => frames = Some(120),
            "--frames" => {
                let value = args.next().ok_or("--frames requires a value")?;
                frames = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| "invalid --frames value")?
                        .max(1),
                );
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(frames)
}

struct AppGpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    errors: Arc<Mutex<Vec<String>>>,
}

impl AppGpu {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("aviqtl-slint-device"),
                ..Default::default()
            }))?;
        let errors = Arc::new(Mutex::new(Vec::new()));
        let error_log = errors.clone();
        device.on_uncaptured_error(Arc::new(move |error| {
            if let Ok(mut errors) = error_log.lock() {
                errors.push(error.to_string());
            }
        }));
        let info = adapter.get_info();
        eprintln!("AviQtl Slint GPU: {} ({:?})", info.name, info.backend);
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            errors,
        })
    }
}

enum WindowKind {
    Main,
    Timeline,
}

struct GpuValidation {
    expected_device: wgpu::Device,
    main_same_device: Cell<bool>,
    timeline_same_device: Cell<bool>,
    main_frames: Cell<u64>,
    timeline_frames: Cell<u64>,
    preview_updates: Cell<u64>,
    texture_imported: bool,
    errors: Arc<Mutex<Vec<String>>>,
}

impl GpuValidation {
    fn new(
        expected_device: wgpu::Device,
        errors: Arc<Mutex<Vec<String>>>,
        texture_imported: bool,
    ) -> Self {
        Self {
            expected_device,
            main_same_device: Cell::new(false),
            timeline_same_device: Cell::new(false),
            main_frames: Cell::new(0),
            timeline_frames: Cell::new(0),
            preview_updates: Cell::new(0),
            texture_imported,
            errors,
        }
    }

    fn print_and_validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        let errors = self
            .errors
            .lock()
            .map_err(|_| "wgpu error log was poisoned")?
            .clone();
        let summary = serde_json::json!({
            "main_same_device": self.main_same_device.get(),
            "timeline_same_device": self.timeline_same_device.get(),
            "texture_imported": self.texture_imported,
            "main_frames": self.main_frames.get(),
            "timeline_frames": self.timeline_frames.get(),
            "preview_updates": self.preview_updates.get(),
            "wgpu_errors": errors,
        });
        println!(
            "SLINT_WGPU_COMPATIBILITY {}",
            serde_json::to_string_pretty(&summary)?
        );
        if !self.main_same_device.get()
            || !self.timeline_same_device.get()
            || !self.texture_imported
            || self.main_frames.get() == 0
            || self.timeline_frames.get() == 0
            || self.preview_updates.get() == 0
            || !summary["wgpu_errors"].as_array().is_some_and(Vec::is_empty)
        {
            return Err("Slint/wgpu compatibility validation failed".into());
        }
        Ok(())
    }
}

fn install_render_probe<T: ComponentHandle + 'static>(
    component: &T,
    stats: Rc<GpuValidation>,
    kind: WindowKind,
) -> Result<(), slint::SetRenderingNotifierError> {
    component.window().set_rendering_notifier(move |phase, api| match phase {
        slint::RenderingState::RenderingSetup => {
            let same = matches!(api, slint::GraphicsAPI::WGPU29 { device, .. } if device == &stats.expected_device);
            match kind {
                WindowKind::Main => stats.main_same_device.set(same),
                WindowKind::Timeline => stats.timeline_same_device.set(same),
            }
        }
        slint::RenderingState::AfterRendering => match kind {
            WindowKind::Main => stats.main_frames.set(stats.main_frames.get() + 1),
            WindowKind::Timeline => stats.timeline_frames.set(stats.timeline_frames.get() + 1),
        },
        _ => {}
    })
}
