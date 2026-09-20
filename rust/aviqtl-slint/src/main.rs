#![deny(unsafe_code)]

mod callbacks;
mod dialogs;
mod easing;
mod export;
mod gpu;
mod lifecycle;
mod localization;
mod object_settings;
mod packages;
mod playback;
mod projection;
mod settings;
mod shortcuts;

#[cfg(test)]
mod tests;

slint::include_modules!();

use crate::callbacks::{install_callbacks, install_timeline_file_drop};
use crate::dialogs::{
    WindowGeometry, install_window_geometry_close_handler, restore_window_geometry,
    show_and_redraw, sync_font_families, system_font_families,
};
use crate::easing::{easing_label, sync_easing_catalog};
use crate::export::{
    ExportCodecState, ExportPlannerRuntime, initialize_export_draft, install_export_callbacks,
    update_export,
};
use crate::gpu::{
    AppGpu, GpuValidation, VALIDATION_PROJECT, WindowKind, install_render_probe,
    parse_validation_frames,
};
use crate::lifecycle::{LifecycleUi, WindowRefs, sync_launcher, sync_recovery_window};
use crate::localization::select_bundled_ui_translation;
use crate::object_settings::{
    ObjectSettingsSyncKey, initialize_timeline_object_catalog, object_settings_sync_key,
    sync_effect_catalog, sync_object_catalog, sync_object_settings,
};
use crate::packages::{
    AudioPluginDiscoveryRuntime, PackageOperationEvent, PackageOperationRuntime,
    sync_package_manager,
};
use crate::playback::{AudioPlaybackRuntime, PreviewRuntime, TimelineWaveformRuntime};
use crate::projection::{sync_transport, sync_windows};
use crate::settings::{
    apply_runtime_settings, project_defaults, sync_system_settings, timeline_maximum_layers,
};
use crate::shortcuts::{install_keyboard_shortcuts, sync_timeline_zoom_settings};
use aviqtl_app::audio_plugin::AudioPluginCatalog;
use aviqtl_app::easing::BezierCurve;
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::mod_host::ModHost;
use aviqtl_app::object_settings::keyframe_interpolation_names;
use aviqtl_app::package_manager::PackageManagerModel;
use aviqtl_app::preset_store::PresetStore;
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::{ApplicationModel, ProjectSession};
use aviqtl_export::ExportManager;
use aviqtl_preview::PreviewSurface;
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    let package_manager_model =
        Rc::new(RefCell::new(PackageManagerModel::load(&settings.borrow())));
    let package_operation = Rc::new(RefCell::new(None::<PackageOperationRuntime>));
    let (effect_catalog, effect_catalog_status) = EffectCatalog::load();
    eprintln!("{effect_catalog_status}");
    let effect_catalog = Rc::new(RefCell::new(effect_catalog));
    let audio_plugin_catalog = Rc::new(RefCell::new(AudioPluginCatalog::default()));
    let (audio_plugin_discovery, audio_plugin_status) =
        match AudioPluginDiscoveryRuntime::start(&settings.borrow()) {
            Ok(runtime) => (Some(runtime), "Audio plugins · scanning…".to_owned()),
            Err(error) => (None, error),
        };
    let audio_plugin_discovery = Rc::new(RefCell::new(audio_plugin_discovery));
    let preset_store = Rc::new(PresetStore::load());
    let font_families = Rc::new(system_font_families());
    let mut application_model = ApplicationModel::default();
    apply_runtime_settings(&mut application_model, &settings.borrow());
    let model = Rc::new(RefCell::new(application_model));
    let mod_host = Rc::new(RefCell::new(ModHost::load(&settings.borrow())));
    eprintln!(
        "Script plugins · {} loaded",
        mod_host.borrow().plugin_count()
    );
    let launcher = ProjectLauncherWindow::new()?;
    select_bundled_ui_translation(&settings.borrow())?;
    let recovery = ProjectRecoveryWindow::new()?;
    let main = MainWindow::new()?;
    let timeline = TimelineWindow::new()?;
    launcher
        .window()
        .set_size(slint::LogicalSize::new(700.0, 500.0));
    restore_window_geometry(
        main.window(),
        &settings.borrow(),
        "main",
        WindowGeometry::new(100, 100, 640, 360),
    );
    restore_window_geometry(
        timeline.window(),
        &settings.borrow(),
        "timeline",
        WindowGeometry::new(100, 600, 1280, 300),
    );
    sync_timeline_zoom_settings(&timeline, &settings.borrow());
    let object_settings = ObjectSettingsWindow::new()?;
    let easing = EasingConfigWindow::new()?;
    let project_settings = ProjectSettingsWindow::new()?;
    let scene_settings = SceneSettingsWindow::new()?;
    let system_settings = SystemSettingsWindow::new()?;
    let export = ExportWindow::new()?;
    let package_manager = PackageManagerWindow::new()?;
    let plugin_permissions = PluginPermissionWindow::new()?;
    let about = AboutWindow::new()?;
    for (window, id, fallback) in [
        (
            project_settings.window(),
            "projectSettings",
            WindowGeometry::new(800, 100, 450, 240),
        ),
        (
            object_settings.window(),
            "objectSettings",
            WindowGeometry::new(800, 420, 900, 650),
        ),
        (
            system_settings.window(),
            "systemSettings",
            WindowGeometry::new(200, 200, 760, 680),
        ),
        (
            about.window(),
            "about",
            WindowGeometry::new(400, 300, 420, 260),
        ),
        (
            scene_settings.window(),
            "sceneSettings",
            WindowGeometry::new(300, 200, 450, 550),
        ),
        (
            easing.window(),
            "easingConfig",
            WindowGeometry::new(420, 180, 880, 560),
        ),
        (
            package_manager.window(),
            "packageManager",
            WindowGeometry::new(500, 300, 650, 450),
        ),
    ] {
        restore_window_geometry(window, &settings.borrow(), id, fallback);
    }
    about.set_version(SharedString::from(env!("CARGO_PKG_VERSION")));
    about.set_codename(SharedString::from("Rolling Release"));
    initialize_export_draft(&export, &settings.borrow());
    package_manager.set_packages(ModelRc::new(VecModel::<PackageData>::default()));
    package_manager.set_repositories(ModelRc::new(VecModel::<PackageRepositoryData>::default()));
    plugin_permissions.set_permissions(ModelRc::new(VecModel::<PluginPermissionData>::default()));
    sync_package_manager(&package_manager, &package_manager_model.borrow());
    let export_manager = Rc::new(RefCell::new(ExportManager::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        Arc::new(|| {}),
    )));
    let export_codecs = Rc::new(RefCell::new(ExportCodecState::default()));
    let export_planner = Rc::new(RefCell::new(ExportPlannerRuntime::default()));
    launcher.set_recent_projects(ModelRc::new(VecModel::<RecentProjectData>::default()));
    sync_launcher(&launcher, &settings.borrow());
    system_settings
        .set_plugin_settings(ModelRc::new(VecModel::<SystemPluginSettingData>::default()));
    system_settings.set_shortcut_settings(ModelRc::new(
        VecModel::<SystemShortcutSettingData>::default(),
    ));
    sync_system_settings(&system_settings, &settings.borrow());
    main.set_preview_image(preview_image);
    main.set_project_tabs(ModelRc::new(VecModel::<ProjectTabData>::default()));
    main.set_missing_media(ModelRc::new(VecModel::<MissingMediaData>::default()));
    recovery.set_recoveries(ModelRc::new(VecModel::<RecoveryEntryData>::default()));
    timeline.set_scene_tabs(ModelRc::new(VecModel::<SceneTabData>::default()));
    timeline.set_clips(ModelRc::new(VecModel::<TimelineClipData>::default()));
    timeline.set_layers(ModelRc::new(VecModel::<LayerData>::default()));
    timeline.set_object_catalog_items(ModelRc::new(VecModel::<EffectCatalogItemData>::default()));
    timeline.set_object_catalog_categories(ModelRc::new(VecModel::<SharedString>::default()));
    timeline.set_object_catalog_menu_categories(ModelRc::new(VecModel::<
        ObjectCatalogMenuCategoryData,
    >::default()));
    timeline.set_context_catalog_items(ModelRc::new(VecModel::<EffectCatalogItemData>::default()));
    timeline.set_context_catalog_categories(ModelRc::new(VecModel::<
        ObjectCatalogMenuCategoryData,
    >::default()));
    initialize_timeline_object_catalog(&timeline, &effect_catalog.borrow());
    object_settings.set_effects(ModelRc::new(VecModel::<ObjectEffectData>::default()));
    object_settings.set_setting_rows(ModelRc::new(VecModel::<ObjectSettingRowData>::default()));
    object_settings
        .set_effect_catalog_items(ModelRc::new(VecModel::<EffectCatalogItemData>::default()));
    object_settings.set_plugin_scan_status(SharedString::from(audio_plugin_status));
    sync_effect_catalog(&object_settings, &effect_catalog.borrow(), "");
    object_settings.set_font_families(ModelRc::new(VecModel::<SharedString>::default()));
    sync_font_families(&object_settings, &font_families, "");
    let easing_names = keyframe_interpolation_names();
    easing.set_easing_names(ModelRc::new(VecModel::from(
        easing_names
            .iter()
            .map(|name| SharedString::from(*name))
            .collect::<Vec<_>>(),
    )));
    easing.set_easing_labels(ModelRc::new(VecModel::from(
        easing_names
            .iter()
            .map(|name| SharedString::from(easing_label(name)))
            .collect::<Vec<_>>(),
    )));
    easing.set_curve_handles(ModelRc::new(VecModel::<CurveHandleData>::default()));
    easing.set_easing_catalog_rows(ModelRc::new(VecModel::<EasingCatalogRowData>::default()));
    sync_easing_catalog(&easing, "", &BezierCurve::default());

    let stats = Rc::new(GpuValidation::new(
        gpu.device.clone(),
        gpu.errors.clone(),
        texture_imported,
    ));
    install_render_probe(&main, stats.clone(), WindowKind::Main)?;
    install_render_probe(&timeline, stats.clone(), WindowKind::Timeline)?;
    let audio_playback = Rc::new(RefCell::new(AudioPlaybackRuntime::new()));
    let lifecycle_ui = Rc::new(LifecycleUi {
        launcher: launcher.as_weak(),
        recovery: recovery.as_weak(),
        main: main.as_weak(),
        timeline: timeline.as_weak(),
        object_settings: object_settings.as_weak(),
        easing: easing.as_weak(),
        project_settings: project_settings.as_weak(),
        scene_settings: scene_settings.as_weak(),
        system_settings: system_settings.as_weak(),
        export: export.as_weak(),
        package_manager: package_manager.as_weak(),
        plugin_permissions: plugin_permissions.as_weak(),
        about: about.as_weak(),
        preview: preview.clone(),
        model: model.clone(),
        settings: settings.clone(),
        effect_catalog: effect_catalog.clone(),
        audio_plugin_catalog: audio_plugin_catalog.clone(),
        quit_confirmed: Cell::new(false),
    });
    lifecycle_ui.sync_live_settings();
    install_callbacks(
        WindowRefs {
            launcher: &launcher,
            recovery: &recovery,
            main: &main,
            timeline: &timeline,
            object_settings: &object_settings,
            easing: &easing,
            project_settings: &project_settings,
            scene_settings: &scene_settings,
            system_settings: &system_settings,
            package_manager: &package_manager,
            plugin_permissions: &plugin_permissions,
            about: &about,
        },
        package_manager_model.clone(),
        package_operation.clone(),
        preset_store,
        font_families,
        lifecycle_ui,
        audio_playback.clone(),
    );
    install_window_geometry_close_handler(timeline.as_weak(), settings.clone(), "timeline");
    install_window_geometry_close_handler(
        object_settings.as_weak(),
        settings.clone(),
        "objectSettings",
    );
    install_window_geometry_close_handler(easing.as_weak(), settings.clone(), "easingConfig");
    install_window_geometry_close_handler(
        project_settings.as_weak(),
        settings.clone(),
        "projectSettings",
    );
    install_window_geometry_close_handler(
        scene_settings.as_weak(),
        settings.clone(),
        "sceneSettings",
    );
    install_window_geometry_close_handler(
        system_settings.as_weak(),
        settings.clone(),
        "systemSettings",
    );
    install_window_geometry_close_handler(
        package_manager.as_weak(),
        settings.clone(),
        "packageManager",
    );
    install_window_geometry_close_handler(about.as_weak(), settings.clone(), "about");
    install_timeline_file_drop(
        &main,
        &timeline,
        model.clone(),
        settings.clone(),
        effect_catalog.clone(),
    );
    install_keyboard_shortcuts(
        &main,
        &timeline,
        model.clone(),
        settings.clone(),
        audio_playback.clone(),
    );
    let mod_host_for_timer = mod_host.clone();
    let mod_settings_for_timer = settings.clone();
    let mod_model_for_timer = model.clone();
    let mod_catalog_for_timer = effect_catalog.clone();
    install_export_callbacks(
        &main,
        &export,
        model.clone(),
        settings,
        export_manager.clone(),
        export_codecs.clone(),
        export_planner.clone(),
    );

    {
        let defaults = project_defaults(&mod_settings_for_timer.borrow());
        let max_layers = timeline_maximum_layers(&mod_settings_for_timer.borrow());
        let default_duration = mod_settings_for_timer
            .borrow()
            .i32_value("defaultClipDuration", 100)
            .clamp(1, 10_000);
        let outcome = mod_host.borrow_mut().apply_pending(
            &mut model.borrow_mut(),
            &mut mod_settings_for_timer.borrow_mut(),
            &effect_catalog.borrow(),
            &defaults,
            max_layers,
            default_duration,
        );
        for diagnostic in outcome.diagnostics {
            eprintln!("MOD: {diagnostic}");
        }
        if outcome.model_changed {
            sync_windows(&main, &timeline, &model.borrow());
        }
    }

    if validation_frames.is_some() {
        let project =
            ProjectSession::from_json(VALIDATION_PROJECT).map_err(std::io::Error::other)?;
        model.borrow_mut().add_project_session(project);
        sync_windows(&main, &timeline, &model.borrow());
        show_and_redraw(&main)?;
        show_and_redraw(&timeline)?;
    } else {
        sync_recovery_window(&recovery, &model.borrow());
        show_and_redraw(&launcher)?;
        if !model.borrow().recovery_entries().is_empty() {
            show_and_redraw(&recovery)?;
        }
    }

    let rendered_ticks = Rc::new(Cell::new(0_u64));
    let timeline_waveforms = Rc::new(RefCell::new(TimelineWaveformRuntime::new()));
    let animation_timer = Timer::default();
    let animation_preview = preview.clone();
    let animation_main = main.as_weak();
    let animation_timeline = timeline.as_weak();
    let animation_launcher = launcher.as_weak();
    let animation_recovery = recovery.as_weak();
    let animation_settings = object_settings.as_weak();
    let animation_mod_host = mod_host_for_timer.clone();
    let animation_mod_model = mod_model_for_timer.clone();
    let animation_mod_settings = mod_settings_for_timer.clone();
    let animation_mod_catalog = mod_catalog_for_timer.clone();
    let animation_easing = easing.as_weak();
    let animation_effect_catalog = effect_catalog.clone();
    let animation_audio_catalog = audio_plugin_catalog;
    let animation_audio_discovery = audio_plugin_discovery;
    let animation_object_sync_key = Rc::new(RefCell::new(None::<ObjectSettingsSyncKey>));
    let object_sync_key = animation_object_sync_key.clone();
    let animation_project_settings = project_settings.as_weak();
    let animation_scene_settings = scene_settings.as_weak();
    let animation_system_settings = system_settings.as_weak();
    let animation_export = export.as_weak();
    let animation_package_manager = package_manager.as_weak();
    let animation_package_model = package_manager_model;
    let animation_package_operation = package_operation;
    let animation_export_manager = export_manager;
    let animation_export_planner = export_planner;
    let animation_stats = stats.clone();
    let animation_model = model.clone();
    let animation_audio_playback = audio_playback;
    let animation_waveforms = timeline_waveforms;
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
        if let Some(window) = animation_settings.upgrade()
            && window.window().is_visible()
        {
            let key = object_settings_sync_key(&animation_model.borrow());
            if *object_sync_key.borrow() != key {
                let effect_catalog = animation_effect_catalog.borrow();
                sync_object_settings(&window, &animation_model.borrow(), &effect_catalog);
                *object_sync_key.borrow_mut() = key;
            }
        }
        let package_poll = animation_package_operation
            .borrow_mut()
            .as_mut()
            .map(PackageOperationRuntime::poll);
        match package_poll {
            Some(Ok(Some(event))) => match event {
                PackageOperationEvent::Progress { status, progress } => {
                    if let Some(window) = animation_package_manager.upgrade() {
                        window.set_status_text(SharedString::from(status));
                        window.set_progress(progress);
                    }
                }
                PackageOperationEvent::Finished { model, outcome } => {
                    *animation_package_model.borrow_mut() = *model;
                    *animation_package_operation.borrow_mut() = None;
                    if outcome.reload_effect_catalog {
                        let (catalog, status) = EffectCatalog::load();
                        eprintln!("{status}");
                        *animation_effect_catalog.borrow_mut() = catalog;
                        animation_preview.borrow_mut().invalidate_native_catalog();
                        animation_export_planner.borrow_mut().reset();
                        if let Some(window) = animation_timeline.upgrade() {
                            let catalog = animation_effect_catalog.borrow();
                            initialize_timeline_object_catalog(&window, &catalog);
                        }
                        if let Some(window) = animation_settings.upgrade() {
                            let catalog = animation_effect_catalog.borrow();
                            sync_object_settings(&window, &animation_model.borrow(), &catalog);
                            sync_object_catalog(
                                &window,
                                &animation_model.borrow(),
                                &catalog,
                                &animation_audio_catalog.borrow(),
                                window.get_effect_filter().as_str(),
                            );
                        }
                    }
                    if outcome.reload_script_plugins {
                        let defaults = project_defaults(&animation_mod_settings.borrow());
                        let max_layers = timeline_maximum_layers(&animation_mod_settings.borrow());
                        let default_duration = animation_mod_settings
                            .borrow()
                            .i32_value("defaultClipDuration", 100)
                            .clamp(1, 10_000);
                        let reload = animation_mod_host.borrow_mut().reload(
                            &mut animation_mod_model.borrow_mut(),
                            &mut animation_mod_settings.borrow_mut(),
                            &animation_mod_catalog.borrow(),
                            &defaults,
                            max_layers,
                            default_duration,
                        );
                        for diagnostic in reload.diagnostics {
                            eprintln!("MOD: {diagnostic}");
                        }
                        if reload.model_changed
                            && let (Some(main), Some(timeline)) =
                                (animation_main.upgrade(), animation_timeline.upgrade())
                        {
                            sync_windows(&main, &timeline, &animation_mod_model.borrow());
                            sync_transport(&main, &timeline, &animation_mod_model.borrow());
                        }
                    }
                    if let Some(window) = animation_package_manager.upgrade() {
                        window.set_busy(false);
                        window.set_progress(1.0);
                        if !outcome.errors.is_empty() {
                            window.set_error_message(SharedString::from(outcome.errors.join("\n")));
                        }
                        if let Some(version) = outcome.self_update_version {
                            window.set_update_message(SharedString::from(format!(
                                "AviQtl Plus {version} is available. Restart to apply the update."
                            )));
                        }
                        sync_package_manager(&window, &animation_package_model.borrow());
                    }
                }
            },
            Some(Err(error)) => {
                eprintln!("Package operation failed: {error}");
                *animation_package_operation.borrow_mut() = None;
                if let Some(window) = animation_package_manager.upgrade() {
                    window.set_busy(false);
                    window.set_error_message(SharedString::from(error));
                }
            }
            _ => {}
        }
        let audio_plugin_poll = animation_audio_discovery
            .borrow_mut()
            .as_mut()
            .map(AudioPluginDiscoveryRuntime::poll);
        match audio_plugin_poll {
            Some(Ok(Some(outcome))) => {
                let mut status = outcome.status();
                for diagnostic in &outcome.diagnostics {
                    eprintln!("Audio plugin discovery warning: {diagnostic}");
                }
                *animation_audio_catalog.borrow_mut() = outcome.catalog;
                let hydration = {
                    let catalog = animation_audio_catalog.borrow();
                    animation_model.borrow_mut().hydrate_audio_plugins(&catalog)
                };
                if hydration.hydrated > 0 {
                    status.push_str(&format!(
                        " · restored {} project plugin(s)",
                        hydration.hydrated
                    ));
                }
                if hydration.deferred > 0 {
                    status.push_str(&format!(
                        " · deferred {} project plugin(s) to preserve undo",
                        hydration.deferred
                    ));
                }
                if !hydration.errors.is_empty() {
                    status.push_str(&format!(
                        " · {} project plugin(s) unavailable",
                        hydration.errors.len()
                    ));
                    for error in &hydration.errors {
                        eprintln!("Audio plugin restore warning: {error}");
                    }
                }
                *animation_audio_discovery.borrow_mut() = None;
                if let Some(window) = animation_settings.upgrade() {
                    let effect_catalog = animation_effect_catalog.borrow();
                    window.set_plugin_scan_status(SharedString::from(status));
                    sync_object_settings(&window, &animation_model.borrow(), &effect_catalog);
                    sync_object_catalog(
                        &window,
                        &animation_model.borrow(),
                        &effect_catalog,
                        &animation_audio_catalog.borrow(),
                        window.get_effect_filter().as_str(),
                    );
                }
            }
            Some(Err(error)) => {
                eprintln!("Audio plugin discovery failed: {error}");
                *animation_audio_discovery.borrow_mut() = None;
                if let Some(window) = animation_settings.upgrade() {
                    window.set_plugin_scan_status(SharedString::from(error));
                }
            }
            _ => {}
        }
        {
            let defaults = project_defaults(&animation_mod_settings.borrow());
            let max_layers = timeline_maximum_layers(&animation_mod_settings.borrow());
            let default_duration = animation_mod_settings
                .borrow()
                .i32_value("defaultClipDuration", 100)
                .clamp(1, 10_000);
            let tick = animation_mod_host.borrow_mut().tick(
                &mut animation_mod_model.borrow_mut(),
                &mut animation_mod_settings.borrow_mut(),
                &animation_mod_catalog.borrow(),
                &defaults,
                max_layers,
                default_duration,
            );
            for diagnostic in tick.diagnostics {
                eprintln!("MOD: {diagnostic}");
            }
            if tick.model_changed
                && let (Some(main), Some(timeline)) =
                    (animation_main.upgrade(), animation_timeline.upgrade())
            {
                sync_windows(&main, &timeline, &animation_mod_model.borrow());
                sync_transport(&main, &timeline, &animation_mod_model.borrow());
            }
        }
        let meters = animation_audio_playback
            .borrow_mut()
            .update(&animation_model.borrow(), &animation_mod_settings.borrow());
        if let Some(window) = animation_main.upgrade() {
            window.set_audio_peak_left(meters.master_peak_left);
            window.set_audio_peak_right(meters.master_peak_right);
            window.set_audio_rms_left(meters.master_rms_left);
            window.set_audio_rms_right(meters.master_rms_right);
            window.set_audio_output_status(SharedString::from(
                animation_audio_playback.borrow().output_error(),
            ));
        }
        if let Some(window) = animation_settings.upgrade() {
            window.set_audio_peak_left(meters.selected_peak_left);
            window.set_audio_peak_right(meters.selected_peak_right);
            window.set_audio_rms_left(meters.selected_rms_left);
            window.set_audio_rms_right(meters.selected_rms_right);
        }
        if let Some(window) = animation_timeline.upgrade() {
            animation_waveforms
                .borrow_mut()
                .update(&window, &animation_model.borrow());
        }
        if let Some(main) = animation_main.upgrade() {
            let preview_updated = {
                let model = animation_model.borrow();
                let effect_catalog = animation_effect_catalog.borrow();
                animation_preview
                    .borrow_mut()
                    .update(&model, &main, &effect_catalog)
            };
            if preview_updated {
                animation_stats
                    .preview_updates
                    .set(animation_stats.preview_updates.get() + 1);
                main.window().request_redraw();
            }
        }
        update_export(
            &animation_export,
            &animation_main,
            &animation_timeline,
            &animation_model,
            &animation_export_manager,
            &animation_export_planner,
            &animation_effect_catalog,
        );
        if validation_frames.is_some_and(|frames| ticks >= frames) {
            if let Some(window) = animation_timeline.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_settings.upgrade() {
                let _ = window.hide();
            }
            if let Some(window) = animation_easing.upgrade() {
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
            if let Some(window) = animation_export.upgrade() {
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
