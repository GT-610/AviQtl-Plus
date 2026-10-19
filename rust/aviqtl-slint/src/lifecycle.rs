//! Project window ownership, save/close sequencing, and recovery presentation.

use crate::dialogs::{
    choose_exo_object_save_path, choose_exo_object_to_import, choose_project_save_path,
    choose_project_to_open, insert_visible_window_geometry, install_opaque_window, json_i32,
    show_and_redraw, show_error_dialog,
};
use crate::easing::refresh_easing_translations;
use crate::localization::{current_ui_language, localized, select_bundled_ui_translation};
use crate::object_settings::{
    initialize_timeline_object_catalog, sync_object_catalog, sync_object_settings,
};
use crate::packages::sync_plugin_permissions;
use crate::playback::PreviewRuntime;
use crate::projection::{sync_windows, update_vec_model};
use crate::settings::{project_defaults, sync_timeline_runtime_settings, system_theme_index};
use crate::{
    AboutWindow, AppTheme, EasingConfigWindow, ExportWindow, MainWindow, ObjectSettingsWindow,
    PackageManagerWindow, PluginPermissionWindow, ProjectLauncherWindow, ProjectRecoveryWindow,
    ProjectSettingsWindow, RecentProjectData, RecoveryEntryData, SceneSettingsWindow,
    SystemSettingsWindow, TimelineWindow,
};
use aviqtl_app::audio_plugin::AudioPluginCatalog;
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::{ApplicationModel, LifecycleStep, WorkspaceModel};
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, SharedString};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RecentProject {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) fps: f64,
}

pub(super) struct LifecycleUi {
    pub(super) launcher: slint::Weak<ProjectLauncherWindow>,
    pub(super) recovery: slint::Weak<ProjectRecoveryWindow>,
    pub(super) main: slint::Weak<MainWindow>,
    pub(super) timeline: slint::Weak<TimelineWindow>,
    pub(super) object_settings: slint::Weak<ObjectSettingsWindow>,
    pub(super) easing: slint::Weak<EasingConfigWindow>,
    pub(super) project_settings: slint::Weak<ProjectSettingsWindow>,
    pub(super) scene_settings: slint::Weak<SceneSettingsWindow>,
    pub(super) system_settings: slint::Weak<SystemSettingsWindow>,
    pub(super) export: slint::Weak<ExportWindow>,
    pub(super) package_manager: slint::Weak<PackageManagerWindow>,
    pub(super) plugin_permissions: slint::Weak<PluginPermissionWindow>,
    pub(super) about: slint::Weak<AboutWindow>,
    pub(super) preview: Rc<RefCell<PreviewRuntime>>,
    pub(super) model: Rc<RefCell<ApplicationModel>>,
    pub(super) settings: Rc<RefCell<SettingsStore>>,
    pub(super) effect_catalog: Rc<RefCell<EffectCatalog>>,
    pub(super) audio_plugin_catalog: Rc<RefCell<AudioPluginCatalog>>,
    pub(super) quit_confirmed: Cell<bool>,
}

pub(super) struct WindowRefs<'a> {
    pub(super) launcher: &'a ProjectLauncherWindow,
    pub(super) recovery: &'a ProjectRecoveryWindow,
    pub(super) main: &'a MainWindow,
    pub(super) timeline: &'a TimelineWindow,
    pub(super) object_settings: &'a ObjectSettingsWindow,
    pub(super) easing: &'a EasingConfigWindow,
    pub(super) project_settings: &'a ProjectSettingsWindow,
    pub(super) scene_settings: &'a SceneSettingsWindow,
    pub(super) system_settings: &'a SystemSettingsWindow,
    pub(super) package_manager: &'a PackageManagerWindow,
    pub(super) plugin_permissions: &'a PluginPermissionWindow,
    pub(super) about: &'a AboutWindow,
}

impl LifecycleUi {
    pub(super) fn workspace_action(&self, action: &str) {
        let (Some(main), Some(timeline), Some(objects)) = (
            self.main.upgrade(),
            self.timeline.upgrade(),
            self.object_settings.upgrade(),
        ) else {
            return;
        };
        let windows = [
            ("main", main.window()),
            ("timeline", timeline.window()),
            ("objectSettings", objects.window()),
        ];
        if action == "save" {
            let mut layout = serde_json::Map::new();
            for (id, window) in windows {
                layout.insert(
                    id.into(),
                    serde_json::json!({
                        "geometry": crate::dialogs::WindowGeometry::capture(window).json(),
                        "visible": window.is_visible(),
                    }),
                );
            }
            let mut settings = self.settings.borrow().snapshot();
            settings.insert(
                "editorWorkspaceLayout".into(),
                serde_json::Value::Object(layout),
            );
            if let Err(message) = self.settings.borrow_mut().apply(settings) {
                crate::dialogs::show_error_dialog(&message);
            }
            return;
        }
        let screen = main
            .window()
            .with_winit_window(|window| {
                window.current_monitor().map(|monitor| {
                    let scale = monitor.scale_factor();
                    let position = monitor.position().to_logical::<i32>(scale);
                    let size = monitor.size().to_logical::<i32>(scale);
                    crate::dialogs::WindowGeometry::new(
                        position.x + 12,
                        position.y + 36,
                        (size.width - 24).max(1),
                        (size.height - 100).max(1),
                    )
                })
            })
            .flatten()
            .unwrap_or(crate::dialogs::WindowGeometry::new(32, 48, 1280, 800));
        let defaults = editor_layout(screen, "editing");
        if action == "recover-screen" {
            for ((_, window), fallback) in windows.iter().zip(defaults) {
                let current = crate::dialogs::WindowGeometry::capture(window);
                let Some(geometry) = recover_window_geometry(current, screen, fallback) else {
                    continue;
                };
                window.set_size(slint::LogicalSize::new(
                    geometry.width as f32,
                    geometry.height as f32,
                ));
                window.set_position(slint::LogicalPosition::new(
                    geometry.x as f32,
                    geometry.y as f32,
                ));
            }
            return;
        }
        let defaults = editor_layout(screen, action);
        let saved = self
            .settings
            .borrow()
            .value("editorWorkspaceLayout")
            .cloned();
        for ((id, window), fallback) in windows.into_iter().zip(defaults) {
            let saved_window = if action == "restore" {
                saved.as_ref().and_then(|layout| layout.get(id))
            } else {
                None
            };
            let mut geometry = crate::dialogs::WindowGeometry::from_value(
                saved_window.and_then(|entry| entry.get("geometry")),
                fallback,
            );
            geometry.width = geometry.width.min(screen.width).max(1);
            geometry.height = geometry.height.min(screen.height).max(1);
            geometry.x = geometry
                .x
                .clamp(screen.x, screen.x + screen.width - geometry.width);
            geometry.y = geometry
                .y
                .clamp(screen.y, screen.y + screen.height - geometry.height);
            window.set_maximized(false);
            window.set_size(slint::LogicalSize::new(
                geometry.width as f32,
                geometry.height as f32,
            ));
            window.set_position(slint::LogicalPosition::new(
                geometry.x as f32,
                geometry.y as f32,
            ));
            if saved_window
                .and_then(|entry| entry.get("visible"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
            {
                let _ = window.show();
                // Restore shows the windows directly instead of going through
                // `show_and_redraw`, so the native frame never got the
                // appearance `sync_live_settings` could not apply while the
                // window did not exist yet.
                match id {
                    "main" => install_opaque_window(&main),
                    "timeline" => install_opaque_window(&timeline),
                    _ => install_opaque_window(&objects),
                }
            } else {
                let _ = window.hide();
            }
            window.set_maximized(geometry.maximized);
        }
    }

    pub(super) fn handle(&self, mut step: LifecycleStep) {
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
                        let _ = show_and_redraw(&main);
                    }
                    return;
                }
                LifecycleStep::ChooseSavePath { suggested_path, .. } => {
                    let path = choose_project_save_path(&suggested_path);
                    step = self.model.borrow_mut().complete_save_path(path.as_deref());
                }
                LifecycleStep::ProjectSaved { project_index } => {
                    self.record_recent_project(project_index);
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
                            sync_launcher(&launcher, &self.settings.borrow());
                            let _ = show_and_redraw(&launcher);
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

    pub(super) fn open_project_dialog(&self, from_launcher: bool) {
        if self.model.borrow().lifecycle_pending() {
            return;
        }
        let Some(path) = choose_project_to_open() else {
            return;
        };
        self.open_project_path(&path, from_launcher);
    }

    /// Imports an AviUtl ExEdit object file into the current scene.
    ///
    /// AviUtl's own "import object file" appends the file's objects after the existing data.
    /// Frames keep their relative positions, and the layer offset lets a file authored on a
    /// different layer count land beside the current work instead of on top of it.
    pub(super) fn import_exo_object_file(&self) {
        if self.model.borrow().lifecycle_pending() {
            return;
        }
        let Some(path) = choose_exo_object_to_import() else {
            return;
        };
        let file = match aviqtl_app::exo_import::load_object_file(&path) {
            Ok(file) => file,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        let catalog = self.effect_catalog.borrow();
        let imported = {
            let mut model = self.model.borrow_mut();
            let Some(workspace) = model.current_workspace_mut() else {
                show_error_dialog(localized(
                    "Open a project before importing an object file.",
                    "请先打开一个项目，再导入物件文件。",
                    "オブジェクトファイルを読み込む前にプロジェクトを開いてください。",
                ));
                return;
            };
            workspace.append_exo_object_file(&file, &catalog, aviqtl_app::MAX_TIMELINE_LAYERS)
        };
        drop(catalog);
        if imported.is_some() {
            self.sync();
            if let Some(timeline) = self.timeline.upgrade() {
                let _ = show_and_redraw(&timeline);
            }
        } else {
            let message = self
                .model
                .borrow()
                .current_workspace()
                .map(|workspace| workspace.status().to_owned())
                .unwrap_or_default();
            show_error_dialog(&message);
        }
    }

    /// Writes the current scene as an AviUtl ExEdit object file.
    pub(super) fn export_exo_object_file(&self) {
        let catalog = self.effect_catalog.borrow();
        let exported = self
            .model
            .borrow()
            .current_workspace()
            .and_then(|workspace| workspace.export_exo_object_file(&catalog));
        drop(catalog);
        let Some((file, report)) = exported else {
            show_error_dialog(localized(
                "This scene has no objects that AviUtl can read.",
                "当前场景没有可导出到 AviUtl 的对象。",
                "このシーンには AviUtl が読めるオブジェクトがありません。",
            ));
            return;
        };
        let suggested = self
            .model
            .borrow()
            .current_workspace()
            .and_then(|workspace| workspace.project().path.clone())
            .and_then(|path| {
                path.file_stem()
                    .map(|stem| PathBuf::from(stem).with_extension("exo"))
            })
            .unwrap_or_else(|| PathBuf::from("objects.exo"));
        let Some(path) = choose_exo_object_save_path(&suggested) else {
            return;
        };
        if let Err(message) = aviqtl_app::exo_import::save_object_file(&path, &file) {
            show_error_dialog(&message);
            return;
        }
        let status = report.summary();
        if let Some(main) = self.main.upgrade() {
            main.set_status_text(status.as_str().into());
        }
        if !report.skipped.is_empty() {
            show_error_dialog(&format!("{status}\n\n{}", report.skipped.join("\n")));
        }
    }

    pub(super) fn open_project_path(&self, path: &Path, from_launcher: bool) {
        let result = self.model.borrow_mut().open_project(path);
        match result {
            Ok(project_index) => {
                self.record_recent_project(project_index);
                self.hydrate_audio_plugins();
                self.sync();
                if let Some(main) = self.main.upgrade() {
                    let _ = show_and_redraw(&main);
                }
                if let Some(timeline) = self.timeline.upgrade() {
                    let _ = show_and_redraw(&timeline);
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

    pub(super) fn record_recent_project(&self, project_index: usize) {
        let result = self
            .model
            .borrow()
            .workspace(project_index)
            .map(|workspace| add_recent_project(&mut self.settings.borrow_mut(), workspace));
        if let Some(Err(error)) = result {
            eprintln!("Failed to update recent projects: {error}");
        }
        if let Some(launcher) = self.launcher.upgrade() {
            sync_launcher(&launcher, &self.settings.borrow());
        }
    }

    pub(super) fn sync(&self) {
        if let (Some(main), Some(timeline)) = (self.main.upgrade(), self.timeline.upgrade()) {
            sync_windows(&main, &timeline, &self.model.borrow());
        }
        if let Some(object_settings) = self.object_settings.upgrade() {
            let model = self.model.borrow();
            let effect_catalog = self.effect_catalog.borrow();
            sync_object_settings(&object_settings, &model, &effect_catalog);
            sync_object_catalog(
                &object_settings,
                &model,
                &effect_catalog,
                &self.audio_plugin_catalog.borrow(),
                object_settings.get_effect_filter().as_str(),
                &self.settings.borrow(),
            );
        }
        if let Some(recovery) = self.recovery.upgrade() {
            sync_recovery_window(&recovery, &self.model.borrow());
        }
    }

    pub(super) fn sync_live_settings(&self) {
        let settings = self.settings.borrow();
        let previous_language = current_ui_language();
        let language_changed = match select_bundled_ui_translation(&settings) {
            Ok(language) => language != previous_language,
            Err(error) => {
                eprintln!("UI translation unavailable: {error}");
                false
            }
        };
        self.preview
            .borrow_mut()
            .decoder
            .set_cache_size_mb(settings.i32_value("cacheSize", 512).clamp(64, 8_192) as usize);
        self.preview.borrow_mut().set_render_settings(
            settings
                .f64_value("previewRenderScale", 1.0)
                .clamp(0.25, 1.0) as f32,
            settings.i32_value("previewMsaaSamples", 0).max(0) as u32,
        );
        if let (Some(main), Some(timeline), Some(object_settings)) = (
            self.main.upgrade(),
            self.timeline.upgrade(),
            self.object_settings.upgrade(),
        ) {
            sync_timeline_runtime_settings(&main, &timeline, &object_settings, &settings);
        }
        let theme_index = system_theme_index(&settings);
        // Native window chrome follows the same choice as the Slint palette, so
        // a light OS appearance cannot leave a light titlebar above dark content.
        let dark = theme_index == 0;
        crate::dialogs::set_native_window_dark(dark);
        if let Some(window) = self.launcher.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.recovery.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.main.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.timeline.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
            if language_changed {
                initialize_timeline_object_catalog(&window, &self.effect_catalog.borrow());
            }
        }
        if let Some(window) = self.object_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
            if language_changed {
                let model = self.model.borrow();
                let catalog = self.effect_catalog.borrow();
                sync_object_settings(&window, &model, &catalog);
                sync_object_catalog(
                    &window,
                    &model,
                    &catalog,
                    &self.audio_plugin_catalog.borrow(),
                    window.get_effect_filter().as_str(),
                    &self.settings.borrow(),
                );
            }
        }
        if let Some(window) = self.easing.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
            if language_changed {
                refresh_easing_translations(&window);
            }
        }
        if let Some(window) = self.project_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.scene_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.system_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.export.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
            if language_changed && !window.get_exporting() {
                window.set_progress_label(
                    format!("0 / 0 {}", localized("frames", "帧", "フレーム")).into(),
                );
            }
        }
        if let Some(window) = self.package_manager.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
        if let Some(window) = self.plugin_permissions.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
            if language_changed {
                sync_plugin_permissions(&window, &settings);
            }
        }
        if let Some(window) = self.about.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            crate::dialogs::apply_native_window_appearance(window.window(), dark);
        }
    }

    pub(super) fn hydrate_audio_plugins(&self) {
        let result = {
            let catalog = self.audio_plugin_catalog.borrow();
            self.model.borrow_mut().hydrate_audio_plugins(&catalog)
        };
        if result.deferred > 0 {
            eprintln!(
                "Audio plugin restore deferred for {} plugin(s) to preserve undo history",
                result.deferred
            );
        }
        for error in result.errors {
            eprintln!("Audio plugin restore warning: {error}");
        }
    }

    pub(super) fn show_recoveries_if_available(&self) {
        let entries = self.model.borrow().recovery_entries();
        if entries.is_empty() {
            return;
        }
        if let Some(recovery) = self.recovery.upgrade() {
            sync_recovery_entries(&recovery, entries);
            recovery.set_error_message(SharedString::new());
            let _ = show_and_redraw(&recovery);
        }
    }

    pub(super) fn hide_all_windows(&self) {
        self.persist_visible_window_geometries();
        if let Some(window) = self.timeline.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.object_settings.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.easing.upgrade() {
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
        if let Some(window) = self.export.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.package_manager.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.plugin_permissions.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = self.about.upgrade() {
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

    pub(super) fn persist_visible_window_geometries(&self) {
        let mut replacement = self.settings.borrow().snapshot();
        let mut changed = false;
        changed |= insert_visible_window_geometry(&mut replacement, "main", &self.main);
        changed |= insert_visible_window_geometry(&mut replacement, "timeline", &self.timeline);
        changed |= insert_visible_window_geometry(
            &mut replacement,
            "projectSettings",
            &self.project_settings,
        );
        changed |= insert_visible_window_geometry(
            &mut replacement,
            "objectSettings",
            &self.object_settings,
        );
        changed |= insert_visible_window_geometry(
            &mut replacement,
            "systemSettings",
            &self.system_settings,
        );
        changed |= insert_visible_window_geometry(&mut replacement, "about", &self.about);
        changed |=
            insert_visible_window_geometry(&mut replacement, "sceneSettings", &self.scene_settings);
        changed |= insert_visible_window_geometry(&mut replacement, "easingConfig", &self.easing);
        changed |= insert_visible_window_geometry(
            &mut replacement,
            "packageManager",
            &self.package_manager,
        );
        if changed && let Err(error) = self.settings.borrow_mut().apply(replacement) {
            eprintln!("Failed to save window geometries: {error}");
        }
    }
}

pub(super) fn editor_layout(
    screen: crate::dialogs::WindowGeometry,
    mode: &str,
) -> [crate::dialogs::WindowGeometry; 3] {
    use crate::dialogs::WindowGeometry;
    let timeline_height = (screen.height * if mode == "audio" { 45 } else { 32 } / 100)
        .max(220)
        .min(screen.height);
    let upper_height = (screen.height - timeline_height - 36)
        .max(480)
        .min(screen.height);
    let object_width = (screen.width * if mode == "animation" { 55 } else { 45 } / 100)
        .max(680)
        .min(screen.width);
    let preview_width = (screen.width - object_width - 12)
        .max(480)
        .min(screen.width);
    [
        WindowGeometry::new(screen.x, screen.y, preview_width, upper_height),
        WindowGeometry::new(
            screen.x,
            screen.y + screen.height - timeline_height,
            screen.width,
            timeline_height,
        ),
        WindowGeometry::new(
            screen.x + screen.width - object_width,
            screen.y,
            object_width,
            upper_height,
        ),
    ]
}

pub(super) fn recover_window_geometry(
    current: crate::dialogs::WindowGeometry,
    screen: crate::dialogs::WindowGeometry,
    fallback: crate::dialogs::WindowGeometry,
) -> Option<crate::dialogs::WindowGeometry> {
    let intersects = current.x < screen.x + screen.width
        && current.x + current.width > screen.x
        && current.y < screen.y + screen.height
        && current.y + current.height > screen.y;
    if intersects {
        return None;
    }
    let width = current.width.min(screen.width).max(1);
    let height = current.height.min(screen.height).max(1);
    Some(crate::dialogs::WindowGeometry {
        x: fallback.x.clamp(screen.x, screen.x + screen.width - width),
        y: fallback
            .y
            .clamp(screen.y, screen.y + screen.height - height),
        width,
        height,
        maximized: current.maximized,
    })
}

pub(super) fn sync_launcher(window: &ProjectLauncherWindow, settings: &SettingsStore) {
    let defaults = project_defaults(settings);
    window.set_default_width(SharedString::from(defaults.width.to_string()));
    window.set_default_height(SharedString::from(defaults.height.to_string()));
    window.set_default_fps(SharedString::from(defaults.fps.to_string()));
    window.set_default_sample_rate(SharedString::from(defaults.sample_rate.to_string()));
    let rows = recent_projects(settings)
        .into_iter()
        .map(|project| RecentProjectData {
            name: SharedString::from(project.name),
            path: SharedString::from(project.path),
            details: SharedString::from(format!(
                "{} × {} @ {} fps",
                project.width, project.height, project.fps
            )),
        })
        .collect();
    update_vec_model(&window.get_recent_projects(), rows);
}

fn recent_projects(settings: &SettingsStore) -> Vec<RecentProject> {
    let maximum = settings.i32_value("recentProjectMaxCount", 10).clamp(1, 50) as usize;
    recent_projects_from_value(settings.value("recentProjects"), maximum)
}

pub(super) fn recent_projects_from_value(
    value: Option<&serde_json::Value>,
    maximum: usize,
) -> Vec<RecentProject> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let object = value.as_object()?;
            let path = object.get("path")?.as_str()?;
            if path.trim().is_empty() {
                return None;
            }
            let name = object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    Path::new(path)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| {
                    localized("Untitled Project", "未命名项目", "無題のプロジェクト").to_owned()
                });
            let fps = object
                .get("fps")
                .and_then(serde_json::Value::as_f64)
                .filter(|fps| fps.is_finite() && *fps > 0.0)
                .unwrap_or(60.0);
            Some(RecentProject {
                name,
                path: path.to_owned(),
                width: json_i32(object.get("width"), 1_920).max(1),
                height: json_i32(object.get("height"), 1_080).max(1),
                fps,
            })
        })
        .take(maximum)
        .collect()
}

fn add_recent_project(
    settings: &mut SettingsStore,
    workspace: &WorkspaceModel,
) -> Result<(), String> {
    let Some(path) = workspace.project().path.as_ref() else {
        return Ok(());
    };
    let path = path.display().to_string();
    if path.trim().is_empty() {
        return Ok(());
    }
    let project_settings = workspace.project_settings();
    let name = Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let recent = merge_recent_project(
        recent_projects(settings),
        RecentProject {
            name,
            path,
            width: project_settings.width,
            height: project_settings.height,
            fps: project_settings.fps,
        },
        settings.i32_value("recentProjectMaxCount", 10).clamp(1, 50) as usize,
    );
    let value = recent
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.name,
                "path": entry.path,
                "width": entry.width,
                "height": entry.height,
                "fps": entry.fps,
            })
        })
        .collect::<Vec<_>>();
    let mut replacement = settings.snapshot();
    replacement.insert("recentProjects".to_owned(), serde_json::Value::Array(value));
    settings.apply(replacement)
}

pub(super) fn merge_recent_project(
    mut recent: Vec<RecentProject>,
    project: RecentProject,
    maximum: usize,
) -> Vec<RecentProject> {
    recent.retain(|entry| entry.path != project.path);
    recent.insert(0, project);
    recent.truncate(maximum);
    recent
}

pub(super) fn sync_recovery_window(window: &ProjectRecoveryWindow, model: &ApplicationModel) {
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
                    localized(
                        "Recovered project",
                        "已恢复的项目",
                        "復元されたプロジェクト",
                    )
                    .to_owned()
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
