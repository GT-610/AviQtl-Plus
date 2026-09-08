#![deny(unsafe_code)]

use aviqtl_app::{
    ApplicationModel, LifecycleStep, ProjectDefaults, ProjectSession, ProjectSettingsInput,
    SaveDecision, SceneSettingsInput, WorkspaceModel,
    audio_plugin::{AudioPluginCatalog, AudioPluginScanOutcome, AudioPluginScanner},
    easing::{BezierCurve, sample_easing_curve},
    effect_catalog::EffectCatalog,
    mod_host::ModHost,
    object_settings::{
        KeyframePoint, ObjectControl, ObjectControlKind, ObjectSettings,
        keyframe_interpolation_names,
    },
    package_manager::{
        PackageManagerModel, PackageOperation, PackageOperationOutcome, PackageSection,
        UreqPackageHttpClient, plugin_permission_grants, save_plugin_permission_grants,
    },
    preset_store::PresetStore,
    selection::SelectionBox,
    settings::SettingsStore,
    timeline_interaction::{TimelineDragKind, TimelineDragRequest},
};
use aviqtl_export::{
    ExportEvent, ExportImageFormat, ExportManager, ExportMode, ExportProject, ExportRequest,
    ExportRuntimeSettings, available_audio_encoders, available_video_encoders, is_software_codec,
    nearest_audio_bitrate, valid_export_path,
};
use aviqtl_preview::{MediaPreview, PlannedPreview, PreviewPlanner, PreviewSurface};
use slint::platform::Key;
use slint::wgpu_29::wgpu;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{
    CloseRequestResponse, Color, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode,
    VecModel,
};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

slint::include_modules!();
include!(concat!(env!("OUT_DIR"), "/effect_metadata_translations.rs"));

const VIDEO_CODECS: [(&str, &str); 14] = [
    ("H.264 – libx264 (SW)", "libx264"),
    ("H.264 – NVENC (NVIDIA)", "h264_nvenc"),
    ("H.264 – AMF (AMD)", "h264_amf"),
    ("H.264 – QSV (Intel)", "h264_qsv"),
    ("H.264 – VAAPI (Linux)", "h264_vaapi"),
    ("HEVC – libx265 (SW)", "libx265"),
    ("HEVC – NVENC (NVIDIA)", "hevc_nvenc"),
    ("HEVC – AMF (AMD)", "hevc_amf"),
    ("HEVC – QSV (Intel)", "hevc_qsv"),
    ("HEVC – VAAPI (Linux)", "hevc_vaapi"),
    ("AV1 – libaom (SW)", "libaom-av1"),
    ("AV1 – NVENC (NVIDIA)", "av1_nvenc"),
    ("AV1 – AMF (AMD)", "av1_amf"),
    ("AV1 – VAAPI (Linux)", "av1_vaapi"),
];
const AUDIO_CODECS: [(&str, &str); 5] = [
    ("AAC", "aac"),
    ("Opus", "libopus"),
    ("MP3", "libmp3lame"),
    ("FLAC (Lossless)", "flac"),
    ("PCM 16-bit", "pcm_s16le"),
];
const PRESET_VALUES: [&str; 5] = ["ultrafast", "fast", "medium", "slow", "veryslow"];
const PROFILE_VALUES: [&str; 4] = ["", "baseline", "main", "high"];
const AUDIO_BITRATES: [i32; 5] = [96, 128, 192, 256, 320];
const SYSTEM_THEME_VALUES: [&str; 3] = ["Dark", "Light", "System"];
const SYSTEM_UI_LANGUAGE_VALUES: [&str; 4] = ["System", "English", "SimplifiedChinese", "Japanese"];
const SYSTEM_PREVIEW_RENDER_SCALES: [f64; 4] = [1.0, 0.75, 0.5, 0.25];
const SYSTEM_PREVIEW_MSAA_SAMPLES: [i32; 4] = [0, 2, 4, 8];
const SYSTEM_BAKE_STRATEGIES: [&str; 2] = ["OnDemand", "FullBake"];
const SYSTEM_EXPORT_VIDEO_CODECS: [&str; 3] = ["h264_vaapi", "hevc_vaapi", "libx264"];
const SYSTEM_EXPORT_AUDIO_CODECS: [&str; 4] = ["aac", "opus", "flac", "pcm_s16le"];
const SYSTEM_AUDIO_BLOCK_SIZES: [i32; 6] = [256, 512, 1024, 2048, 4096, 8192];
const SYSTEM_PLUGIN_FORMATS: [&str; 11] = [
    "LADSPA", "DSSI", "LV2", "VST2", "VST3", "CLAP", "SF2", "SFZ", "JSFX", "Effects", "Objects",
];
const SYSTEM_SHORTCUT_ROWS: [(&str, &str); 34] = [
    ("project.new", "Ctrl+N"),
    ("project.open", "Ctrl+O"),
    ("project.save", "Ctrl+S"),
    ("project.saveAs", "Ctrl+Shift+S"),
    ("project.export", "Ctrl+E"),
    ("app.quit", "Ctrl+Q"),
    ("app.settings", "Ctrl+P"),
    ("edit.undo", "Ctrl+Z"),
    ("edit.redo", "Ctrl+Shift+Z"),
    ("edit.cut", "Ctrl+X"),
    ("edit.copy", "Ctrl+C"),
    ("edit.paste", "Ctrl+V"),
    ("edit.delete", "Delete"),
    ("edit.duplicate", "Ctrl+D"),
    ("transport.playPause", "Space"),
    ("transport.nextFrame", "Right"),
    ("transport.prevFrame", "Left"),
    ("transport.jumpStart", "Home"),
    ("transport.jumpEnd", "End"),
    ("view.zoomIn", "Ctrl++"),
    ("view.zoomOut", "Ctrl+-"),
    ("view.timeline", "F3"),
    ("view.objectSettings", "F4"),
    ("project.settings", "Alt+Enter"),
    ("timeline.split", "S"),
    ("timeline.moveUp", "Alt+Up"),
    ("timeline.moveDown", "Alt+Down"),
    ("timeline.nudgeLeft", "Alt+Left"),
    ("timeline.nudgeRight", "Alt+Right"),
    ("timeline.addScene", "Ctrl+T"),
    ("timeline.sceneSettings", "Alt+S"),
    ("timeline.removeScene", "Ctrl+Shift+Delete"),
    ("timeline.layerLock", "Ctrl+L"),
    ("timeline.layerHide", "Ctrl+H"),
];
const EASING_CATEGORIES: [(&str, &[&str]); 5] = [
    ("Basic", &["none", "linear"]),
    (
        "Standard curves",
        &[
            "ease_in_sine",
            "ease_out_sine",
            "ease_in_out_sine",
            "ease_out_in_sine",
            "ease_in_quad",
            "ease_out_quad",
            "ease_in_out_quad",
            "ease_out_in_quad",
            "ease_in_cubic",
            "ease_out_cubic",
            "ease_in_out_cubic",
            "ease_out_in_cubic",
        ],
    ),
    (
        "Strong curves",
        &[
            "ease_in_quart",
            "ease_out_quart",
            "ease_in_out_quart",
            "ease_out_in_quart",
            "ease_in_quint",
            "ease_out_quint",
            "ease_in_out_quint",
            "ease_out_in_quint",
            "ease_in_expo",
            "ease_out_expo",
            "ease_in_out_expo",
            "ease_out_in_expo",
            "ease_in_circ",
            "ease_out_circ",
            "ease_in_out_circ",
            "ease_out_in_circ",
        ],
    ),
    (
        "Bounce and elasticity",
        &[
            "ease_in_back",
            "ease_out_back",
            "ease_in_out_back",
            "ease_out_in_back",
            "ease_in_elastic",
            "ease_out_elastic",
            "ease_in_out_elastic",
            "ease_out_in_elastic",
            "ease_in_bounce",
            "ease_out_bounce",
            "ease_in_out_bounce",
            "ease_out_in_bounce",
        ],
    ),
    ("Special", &["random", "alternate", "custom"]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiLanguage {
    English,
    SimplifiedChinese,
    Japanese,
}

impl UiLanguage {
    fn slint_locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh_CN",
            Self::Japanese => "ja_JP",
        }
    }
}

thread_local! {
    static CURRENT_UI_LANGUAGE: Cell<UiLanguage> = const { Cell::new(UiLanguage::English) };
}

fn localized(
    english: &'static str,
    simplified_chinese: &'static str,
    japanese: &'static str,
) -> &'static str {
    match current_ui_language() {
        UiLanguage::English => english,
        UiLanguage::SimplifiedChinese => simplified_chinese,
        UiLanguage::Japanese => japanese,
    }
}

fn current_ui_language() -> UiLanguage {
    CURRENT_UI_LANGUAGE.with(|language| language.get())
}

fn localized_effect_metadata(value: &str) -> Cow<'_, str> {
    let catalog = CURRENT_UI_LANGUAGE.with(|language| match language.get() {
        UiLanguage::English => Some(EFFECT_METADATA_ENGLISH),
        UiLanguage::SimplifiedChinese => Some(EFFECT_METADATA_SIMPLIFIED_CHINESE),
        UiLanguage::Japanese => None,
    });
    let Some(catalog) = catalog else {
        return Cow::Borrowed(value);
    };
    catalog
        .binary_search_by_key(&value, |(source, _)| *source)
        .ok()
        .map(|index| Cow::Borrowed(catalog[index].1))
        .unwrap_or_else(|| Cow::Borrowed(value))
}

fn localized_effect_categories(categories: &[String]) -> String {
    categories
        .iter()
        .map(|category| localized_effect_metadata(category).into_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutAction {
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ExportMedia,
    Quit,
    SystemSettings,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Delete,
    Duplicate,
    PlayPause,
    NextFrame,
    PreviousFrame,
    JumpStart,
    JumpEnd,
    ZoomIn,
    ZoomOut,
    ShowTimeline,
    ShowObjectSettings,
    ProjectSettings,
    Split,
    MoveUp,
    MoveDown,
    NudgeLeft,
    NudgeRight,
    AddScene,
    SceneSettings,
    RemoveScene,
    ToggleLayerLock,
    ToggleLayerVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShortcutInput {
    text: String,
    alt: bool,
    control: bool,
    shift: bool,
    meta: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShortcutPattern {
    text: String,
    alt: bool,
    control: bool,
    shift: bool,
    meta: bool,
    ignore_shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TimelineScrollInput {
    delta_x: f32,
    delta_y: f32,
    content_x: f32,
    viewport_x: f32,
    viewport_y: f32,
    visible_width: f32,
    viewport_width: f32,
    visible_height: f32,
    viewport_height: f32,
    duration_frames: f32,
    alt: bool,
    control: bool,
    shift: bool,
    ruler_zoom: bool,
    pixels_per_frame: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TimelineScrollPlan {
    pixels_per_frame: f32,
    viewport_x: f32,
    viewport_y: f32,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectSettingsSyncKey {
    project_instance_id: u64,
    document_revision: u64,
    clip_id: i32,
    playhead: i32,
    effect_selection: Vec<bool>,
}

struct PreviewRuntime {
    surface: PreviewSurface,
    decoder: MediaPreview,
    planner: Option<PreviewPlanner>,
    source_key: Option<PreviewSourceKey>,
    requested_frame: Option<PreviewFrameKey>,
}

#[derive(Default)]
struct ExportCodecState {
    video_values: Vec<String>,
    audio_values: Vec<String>,
}

#[derive(Default)]
struct ExportPlannerRuntime {
    planner: Option<PreviewPlanner>,
    source_key: Option<PreviewSourceKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    maximized: bool,
}

impl WindowGeometry {
    const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    fn load(settings: &SettingsStore, id: &str, fallback: Self) -> Self {
        let key = format!("windowGeometry_{id}");
        Self::from_value(settings.value(&key), fallback)
    }

    fn from_value(value: Option<&serde_json::Value>, fallback: Self) -> Self {
        let Some(value) = value.and_then(serde_json::Value::as_object) else {
            return fallback;
        };
        Self {
            x: json_i32(value.get("x"), fallback.x),
            y: json_i32(value.get("y"), fallback.y),
            width: json_i32(value.get("width"), fallback.width).max(1),
            height: json_i32(value.get("height"), fallback.height).max(1),
            maximized: value
                .get("maximized")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(fallback.maximized),
        }
    }

    fn capture(window: &slint::Window) -> Self {
        let scale_factor = window.scale_factor().max(f32::EPSILON);
        let position = window.position().to_logical(scale_factor);
        let size = window.size().to_logical(scale_factor);
        Self {
            x: position.x.round() as i32,
            y: position.y.round() as i32,
            width: size.width.round().max(1.0) as i32,
            height: size.height.round().max(1.0) as i32,
            maximized: window.is_maximized(),
        }
    }

    fn json(self) -> serde_json::Value {
        serde_json::json!({
            "x": self.x,
            "y": self.y,
            "width": self.width,
            "height": self.height,
            "maximized": self.maximized,
        })
    }
}

struct AudioPluginDiscoveryRuntime {
    stop: Arc<AtomicBool>,
    receiver: Receiver<AudioPluginScanOutcome>,
    worker: Option<JoinHandle<()>>,
}

enum PackageOperationEvent {
    Progress {
        status: String,
        progress: f32,
    },
    Finished {
        model: Box<PackageManagerModel>,
        outcome: PackageOperationOutcome,
    },
}

struct PackageOperationRuntime {
    receiver: Receiver<PackageOperationEvent>,
    worker: Option<JoinHandle<()>>,
}

impl PackageOperationRuntime {
    fn start(model: PackageManagerModel, operation: PackageOperation) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("aviqtl-package-operation".to_owned())
            .spawn(move || {
                let mut model = model;
                let client = UreqPackageHttpClient::default();
                let progress_sender = sender.clone();
                let outcome =
                    model.execute_operation(operation, &client, move |status, progress| {
                        let _ = progress_sender.send(PackageOperationEvent::Progress {
                            status: status.to_owned(),
                            progress,
                        });
                    });
                let _ = sender.send(PackageOperationEvent::Finished {
                    model: Box::new(model),
                    outcome,
                });
            })
            .map_err(|error| format!("Failed to start package operation: {error}"))?;
        Ok(Self {
            receiver,
            worker: Some(worker),
        })
    }

    fn poll(&mut self) -> Option<PackageOperationEvent> {
        let event = self.receiver.try_recv().ok()?;
        if matches!(event, PackageOperationEvent::Finished { .. })
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
        Some(event)
    }
}

fn start_package_operation(
    window: &PackageManagerWindow,
    runtime: &Rc<RefCell<Option<PackageOperationRuntime>>>,
    model: &Rc<RefCell<PackageManagerModel>>,
    operation: PackageOperation,
) {
    if runtime.borrow().is_some() {
        return;
    }
    window.set_error_message(SharedString::new());
    window.set_busy(true);
    window.set_progress(0.0);
    match PackageOperationRuntime::start(model.borrow().clone(), operation) {
        Ok(operation) => *runtime.borrow_mut() = Some(operation),
        Err(error) => {
            window.set_busy(false);
            window.set_error_message(SharedString::from(error));
        }
    }
}

impl AudioPluginDiscoveryRuntime {
    fn start(settings: &SettingsStore) -> Result<Self, String> {
        let scanner = AudioPluginScanner::from_settings(settings);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("aviqtl-audio-plugin-discovery".to_owned())
            .spawn(move || {
                let result = scanner.scan(&worker_stop);
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Audio plugins · failed to start scanner: {error}"))?;
        Ok(Self {
            stop,
            receiver,
            worker: Some(worker),
        })
    }

    fn poll(&mut self) -> Option<AudioPluginScanOutcome> {
        let result = self.receiver.try_recv().ok()?;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Some(result)
    }
}

impl Drop for AudioPluginDiscoveryRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl ExportPlannerRuntime {
    fn reset(&mut self) {
        self.planner = None;
        self.source_key = None;
    }

    fn build(
        &mut self,
        project_instance_id: u64,
        workspace: &WorkspaceModel,
        frame: i32,
    ) -> Option<PlannedPreview> {
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
            self.source_key = Some(source_key);
        }
        self.planner.as_mut().and_then(|planner| {
            planner.build(workspace.document(), workspace.selected_scene(), frame)
        })
    }
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

    fn set_render_settings(&mut self, render_scale: f32, msaa_samples: u32) {
        let changed = self.surface.set_render_scale(render_scale)
            | self.surface.set_msaa_samples(msaa_samples);
        if changed {
            self.decoder.reset();
            self.requested_frame = None;
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
    easing: slint::Weak<EasingConfigWindow>,
    project_settings: slint::Weak<ProjectSettingsWindow>,
    scene_settings: slint::Weak<SceneSettingsWindow>,
    system_settings: slint::Weak<SystemSettingsWindow>,
    export: slint::Weak<ExportWindow>,
    package_manager: slint::Weak<PackageManagerWindow>,
    plugin_permissions: slint::Weak<PluginPermissionWindow>,
    about: slint::Weak<AboutWindow>,
    preview: Rc<RefCell<PreviewRuntime>>,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    effect_catalog: Rc<RefCell<EffectCatalog>>,
    audio_plugin_catalog: Rc<RefCell<AudioPluginCatalog>>,
    quit_confirmed: Cell<bool>,
}

struct WindowRefs<'a> {
    launcher: &'a ProjectLauncherWindow,
    recovery: &'a ProjectRecoveryWindow,
    main: &'a MainWindow,
    timeline: &'a TimelineWindow,
    object_settings: &'a ObjectSettingsWindow,
    easing: &'a EasingConfigWindow,
    project_settings: &'a ProjectSettingsWindow,
    scene_settings: &'a SceneSettingsWindow,
    system_settings: &'a SystemSettingsWindow,
    package_manager: &'a PackageManagerWindow,
    plugin_permissions: &'a PluginPermissionWindow,
    about: &'a AboutWindow,
}

#[derive(Clone)]
struct ObjectSettingsUi {
    main: slint::Weak<MainWindow>,
    timeline: slint::Weak<TimelineWindow>,
    window: slint::Weak<ObjectSettingsWindow>,
    easing: slint::Weak<EasingConfigWindow>,
    easing_curve: Rc<RefCell<BezierCurve>>,
    model: Rc<RefCell<ApplicationModel>>,
    catalog: Rc<RefCell<EffectCatalog>>,
    audio_catalog: Rc<RefCell<aviqtl_app::audio_plugin::AudioPluginCatalog>>,
    presets: Rc<PresetStore>,
    font_families: Rc<Vec<String>>,
}

impl ObjectSettingsUi {
    fn sync(&self) {
        sync_weak_windows(&self.main, &self.timeline, &self.model);
        if let Some(window) = self.window.upgrade() {
            let model = self.model.borrow();
            let catalog = self.catalog.borrow();
            sync_object_settings(&window, &model, &catalog);
            sync_object_catalog(
                &window,
                &model,
                &catalog,
                &self.audio_catalog.borrow(),
                window.get_effect_filter().as_str(),
            );
        }
    }

    fn control(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
    ) -> Option<ObjectControl> {
        let projection = {
            let model = self.model.borrow();
            let catalog = self.catalog.borrow();
            model.current_workspace()?.object_settings(&catalog)?
        };
        let controls = if audio_plugin {
            &projection.audio_plugins.get(effect_index)?.controls
        } else {
            &projection.effects.get(effect_index)?.controls
        };
        controls
            .iter()
            .find(|control| control.param.as_deref() == Some(param_name))
            .cloned()
    }

    fn set_value(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        value: serde_json::Value,
    ) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if audio_plugin {
                    workspace.set_audio_plugin_parameter_at_frame(
                        effect_index,
                        param_name,
                        frame,
                        value,
                    )
                } else {
                    workspace.set_effect_parameter_at_frame(effect_index, param_name, frame, value)
                }
            });
        self.sync();
    }

    fn set_text(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        text: &str,
    ) {
        let Some(control) = self.control(audio_plugin, effect_index, param_name) else {
            return;
        };
        match control.parse_text(text) {
            Ok(value) => self.set_value(audio_plugin, effect_index, param_name, frame, value),
            Err(message) => show_error_dialog(&message),
        }
    }

    fn set_number(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        value: f32,
    ) {
        if !value.is_finite() {
            return;
        }
        self.set_text(
            audio_plugin,
            effect_index,
            param_name,
            frame,
            &value.to_string(),
        );
    }

    fn set_option(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        option_index: usize,
    ) {
        let Some(value) = self
            .control(audio_plugin, effect_index, param_name)
            .and_then(|control| control.option_value(option_index))
        else {
            return;
        };
        self.set_value(audio_plugin, effect_index, param_name, frame, value);
    }

    fn open_easing(&self, effect_index: usize, param_name: &str, start_frame: i32, end_frame: i32) {
        let point = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .and_then(|workspace| {
                workspace.prepare_effect_easing(effect_index, param_name, start_frame, end_frame)
            });
        let Some(point) = point else {
            return;
        };
        self.sync();
        if let Some(window) = self.easing.upgrade() {
            let curve = sync_easing_window(&window, effect_index, param_name, &point);
            *self.easing_curve.borrow_mut() = curve;
            let curve = self.easing_curve.borrow();
            sync_easing_preview(&window, &curve);
            sync_easing_catalog(&window, window.get_easing_filter().as_str(), &curve);
            let _ = show_and_redraw(&window);
        }
    }

    fn update_easing_custom_points(&self, controls: [f32; 4]) -> Vec<f64> {
        let mut curve = self.easing_curve.borrow_mut();
        curve.set_first_controls(controls.map(f64::from));
        curve.points().to_vec()
    }

    fn apply_easing(
        &self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        options: serde_json::Value,
    ) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.set_effect_keyframe_options(effect_index, param_name, frame, options)
            });
        self.sync();
    }

    fn add_effect(&self, effect_id: &str) {
        let audio_plugin = self
            .model
            .borrow()
            .current_workspace()
            .is_some_and(WorkspaceModel::object_settings_uses_audio_plugins);
        if audio_plugin {
            match self.audio_catalog.borrow().addition(effect_id) {
                Ok(addition) => {
                    let _ = self
                        .model
                        .borrow_mut()
                        .current_workspace_mut()
                        .is_some_and(|workspace| workspace.add_audio_plugin(addition));
                }
                Err(message) => show_error_dialog(&message),
            }
        } else {
            let catalog = self.catalog.borrow();
            let _ = self
                .model
                .borrow_mut()
                .current_workspace_mut()
                .is_some_and(|workspace| workspace.add_effect(&catalog, effect_id));
        }
        self.sync();
    }

    fn reorder_effect(&self, source: usize, delta_y: f32) {
        if !delta_y.is_finite() {
            return;
        }
        let Some(length) = self
            .model
            .borrow()
            .current_workspace()
            .and_then(WorkspaceModel::selected_clip_document)
            .map(|clip| {
                if clip.clip_type == "audio" {
                    clip.audio_plugins.len()
                } else {
                    clip.effects.len()
                }
            })
        else {
            return;
        };
        if length == 0 || source >= length {
            return;
        }
        let target = (source as i64 + i64::from((delta_y / 34.0).round() as i32))
            .clamp(0, length.saturating_sub(1) as i64) as usize;
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.reorder_audio_plugins(source, target)
                } else {
                    workspace.reorder_effects(source, target)
                }
            });
        self.sync();
    }

    fn save_preset(&self, effect_index: usize, name: &str) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.save_audio_plugin_preset(&self.presets, effect_index, name)
                } else {
                    workspace.save_effect_preset(&self.presets, effect_index, name)
                }
            });
        self.sync();
    }

    fn load_preset(&self, effect_index: usize, name: &str) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.load_audio_plugin_preset(&self.presets, effect_index, name)
                } else {
                    workspace.load_effect_preset(&self.presets, effect_index, name)
                }
            });
        self.sync();
    }

    fn delete_preset(&self, effect_index: usize, name: &str) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.delete_audio_plugin_preset(&self.presets, effect_index, name)
                } else {
                    workspace.delete_effect_preset(&self.presets, effect_index, name)
                }
            });
        self.sync();
    }
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
                        let _ = show_and_redraw(&main);
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

    fn sync(&self) {
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
            );
        }
        if let Some(recovery) = self.recovery.upgrade() {
            sync_recovery_window(&recovery, &self.model.borrow());
        }
    }

    fn sync_live_settings(&self) {
        let settings = self.settings.borrow();
        let previous_language = current_ui_language();
        let language_changed = match select_bundled_ui_translation(&settings) {
            Ok(language) => language != previous_language,
            Err(error) => {
                eprintln!("UI translation unavailable: {error}");
                false
            }
        };
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
        if let Some(window) = self.launcher.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
        if let Some(window) = self.recovery.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
        if let Some(window) = self.main.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
        if let Some(window) = self.timeline.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            if language_changed {
                initialize_timeline_object_catalog(&window, &self.effect_catalog.borrow());
            }
        }
        if let Some(window) = self.object_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
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
                );
            }
        }
        if let Some(window) = self.easing.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            if language_changed {
                refresh_easing_translations(&window);
            }
        }
        if let Some(window) = self.project_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
        if let Some(window) = self.scene_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
        if let Some(window) = self.system_settings.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
        if let Some(window) = self.export.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
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
        }
        if let Some(window) = self.plugin_permissions.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
            if language_changed {
                sync_plugin_permissions(&window, &settings);
            }
        }
        if let Some(window) = self.about.upgrade() {
            window
                .global::<AppTheme>()
                .set_preference_index(theme_index);
        }
    }

    fn hydrate_audio_plugins(&self) {
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

    fn show_recoveries_if_available(&self) {
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

    fn hide_all_windows(&self) {
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

    fn persist_visible_window_geometries(&self) {
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
    sync_launcher_defaults(&launcher, &settings.borrow());
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
    timeline.set_context_catalog_items(ModelRc::new(VecModel::<EffectCatalogItemData>::default()));
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
    install_keyboard_shortcuts(&main, &timeline, model.clone(), settings.clone());
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
        let package_event = animation_package_operation
            .borrow_mut()
            .as_mut()
            .and_then(PackageOperationRuntime::poll);
        if let Some(event) = package_event {
            match event {
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
            }
        }
        let audio_plugin_scan = animation_audio_discovery
            .borrow_mut()
            .as_mut()
            .and_then(AudioPluginDiscoveryRuntime::poll);
        if let Some(outcome) = audio_plugin_scan {
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
        update_export(
            &animation_export,
            &animation_main,
            &animation_timeline,
            &animation_model,
            &animation_export_manager,
            &animation_export_planner,
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

fn install_timeline_file_drop(
    main: &MainWindow,
    timeline: &TimelineWindow,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    catalog: Rc<RefCell<EffectCatalog>>,
) {
    let dropped_model = model.clone();
    let dropped_settings = settings;
    let dropped_catalog = catalog;
    let dropped_main = main.as_weak();
    let dropped_timeline = timeline.as_weak();
    timeline.on_file_dropped(move |path, frame, layer, ignore_snap| {
        let Some(window) = dropped_timeline.upgrade() else {
            return TimelineMediaDropResult {
                imported: false,
                next_frame: 0,
                layer: 0,
            };
        };
        let default_duration = dropped_settings
            .borrow()
            .i32_value("defaultClipDuration", 100)
            .max(1);
        let path = PathBuf::from(path.as_str());
        let result = {
            let catalog = dropped_catalog.borrow();
            let mut application = dropped_model.borrow_mut();
            application.current_workspace_mut().and_then(|workspace| {
                let frame = workspace.snap_timeline_frame(
                    f64::from(frame),
                    ignore_snap,
                    f64::from(window.get_pixels_per_frame()),
                );
                workspace.import_media_files(
                    std::slice::from_ref(&path),
                    frame,
                    layer,
                    default_duration,
                    window.get_maximum_layers(),
                    &catalog,
                )
            })
        };
        let drop_result = result.map_or(
            TimelineMediaDropResult {
                imported: false,
                next_frame: 0,
                layer: layer.clamp(0, timeline_last_layer(&window)),
            },
            |(next_frame, next_layer)| {
                window.set_skimmer_frame(next_frame.max(0));
                window.set_skimmer_layer(next_layer.clamp(0, timeline_last_layer(&window)));
                window.set_skimmer_visible(true);
                TimelineMediaDropResult {
                    imported: true,
                    next_frame,
                    layer: next_layer,
                }
            },
        );
        sync_weak_windows(&dropped_main, &dropped_timeline, &dropped_model);
        drop_result
    });

    let timeline_weak = timeline.as_weak();
    let mut cursor_position = None::<(f32, f32)>;
    let mut shift_pressed = false;
    let mut hovered_file_count = 0usize;
    let mut next_target = None::<(i32, i32)>;
    timeline
        .window()
        .on_winit_window_event(move |window, event| {
            let Some(timeline) = timeline_weak.upgrade() else {
                return EventResult::Propagate;
            };
            match event {
                winit::event::WindowEvent::CursorMoved { position, .. } => {
                    let logical = position.to_logical::<f32>(f64::from(window.scale_factor()));
                    cursor_position = Some((logical.x, logical.y));
                    if hovered_file_count > 0 {
                        timeline.invoke_native_file_hovered(logical.x, logical.y, shift_pressed);
                    }
                }
                winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                    shift_pressed = modifiers.state().shift_key();
                    if hovered_file_count > 0
                        && let Some((x, y)) = cursor_position
                    {
                        timeline.invoke_native_file_hovered(x, y, shift_pressed);
                    }
                }
                winit::event::WindowEvent::HoveredFile(_) => {
                    if hovered_file_count == 0 {
                        next_target = None;
                    }
                    hovered_file_count = hovered_file_count.saturating_add(1);
                    if let Some((x, y)) = cursor_position {
                        timeline.invoke_native_file_hovered(x, y, shift_pressed);
                    }
                }
                winit::event::WindowEvent::HoveredFileCancelled => {
                    hovered_file_count = 0;
                    next_target = None;
                    timeline.invoke_native_file_left();
                }
                winit::event::WindowEvent::DroppedFile(path) => {
                    hovered_file_count = hovered_file_count.saturating_sub(1);
                    let path = SharedString::from(path.to_string_lossy().as_ref());
                    let result = if let Some((frame, layer)) = next_target {
                        timeline.invoke_file_dropped(path, frame as f32, layer, true)
                    } else if let Some((x, y)) = cursor_position {
                        timeline.invoke_native_file_dropped(path, x, y, shift_pressed)
                    } else {
                        TimelineMediaDropResult {
                            imported: false,
                            next_frame: 0,
                            layer: 0,
                        }
                    };
                    if result.imported {
                        next_target = Some((result.next_frame, result.layer));
                    }
                    if hovered_file_count == 0 {
                        timeline.set_file_drop_active(false);
                    }
                }
                _ => {}
            }
            EventResult::Propagate
        });
}

fn install_callbacks(
    windows: WindowRefs<'_>,
    package_manager_model: Rc<RefCell<PackageManagerModel>>,
    package_operation: Rc<RefCell<Option<PackageOperationRuntime>>>,
    preset_store: Rc<PresetStore>,
    font_families: Rc<Vec<String>>,
    lifecycle_ui: Rc<LifecycleUi>,
) {
    let system_apply_ui = lifecycle_ui.clone();
    let model = lifecycle_ui.model.clone();
    let settings = lifecycle_ui.settings.clone();
    let effect_catalog = lifecycle_ui.effect_catalog.clone();
    let WindowRefs {
        launcher,
        recovery,
        main,
        timeline,
        object_settings,
        easing,
        project_settings,
        scene_settings,
        system_settings,
        package_manager,
        plugin_permissions,
        about,
    } = windows;
    let object_settings_ui = ObjectSettingsUi {
        main: main.as_weak(),
        timeline: timeline.as_weak(),
        window: object_settings.as_weak(),
        easing: easing.as_weak(),
        easing_curve: Rc::new(RefCell::new(BezierCurve::default())),
        model: model.clone(),
        catalog: effect_catalog.clone(),
        audio_catalog: lifecycle_ui.audio_plugin_catalog.clone(),
        presets: preset_store.clone(),
        font_families,
    };
    let object_select_ui = object_settings_ui.clone();
    object_settings.on_select_effect(move |index, control, shift| {
        let _ = object_select_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.select_effect(index.max(0) as usize, control, shift)
            });
        object_select_ui.sync();
    });
    let object_context_ui = object_settings_ui.clone();
    object_settings.on_context_select_effect(move |index| {
        let _ = object_context_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.context_select_effect(index.max(0) as usize));
        object_context_ui.sync();
    });
    let object_enabled_ui = object_settings_ui.clone();
    object_settings.on_set_effect_enabled(move |index, enabled| {
        let _ = object_enabled_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.set_audio_plugin_enabled(index.max(0) as usize, enabled)
                } else {
                    workspace.set_effect_enabled(index.max(0) as usize, enabled)
                }
            });
        object_enabled_ui.sync();
    });
    let object_remove_ui = object_settings_ui.clone();
    object_settings.on_remove_effect(move |index| {
        let _ = object_remove_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.remove_audio_plugin(index.max(0) as usize)
                } else {
                    workspace.remove_effect(index.max(0) as usize)
                }
            });
        object_remove_ui.sync();
    });
    let object_remove_selection_ui = object_settings_ui.clone();
    object_settings.on_remove_effect_selection(move |index| {
        let _ = object_remove_selection_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.remove_audio_plugin_group(index.max(0) as usize)
                } else {
                    workspace.remove_effect_group(index.max(0) as usize)
                }
            });
        object_remove_selection_ui.sync();
    });
    let object_delete_selection_ui = object_settings_ui.clone();
    object_settings.on_delete_selected_effects(move || {
        let _ = object_delete_selection_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.remove_selected_audio_plugins()
                } else {
                    workspace.remove_selected_effects()
                }
            });
        object_delete_selection_ui.sync();
    });
    let object_text_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_text(move |audio_plugin, index, param, frame, value| {
        object_text_ui.set_text(
            audio_plugin,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            value.as_str(),
        );
    });
    let object_number_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_number(move |audio_plugin, index, param, frame, value| {
        object_number_ui.set_number(
            audio_plugin,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            value,
        );
    });
    let object_bool_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_bool(move |audio_plugin, index, param, frame, value| {
        object_bool_ui.set_value(
            audio_plugin,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            serde_json::Value::Bool(value),
        );
    });
    let object_option_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_option(move |audio_plugin, index, param, frame, option| {
        if option >= 0 {
            object_option_ui.set_option(
                audio_plugin,
                index.max(0) as usize,
                param.as_str(),
                frame.max(0),
                option as usize,
            );
        }
    });
    let object_path_ui = object_settings_ui.clone();
    object_settings.on_choose_parameter_path(
        move |audio_plugin, index, param, frame, current, filter, label| {
            let Some(path) = pick_parameter_file(current.as_str(), filter.as_str(), label.as_str())
            else {
                return;
            };
            object_path_ui.set_text(
                audio_plugin,
                index.max(0) as usize,
                param.as_str(),
                frame.max(0),
                &path,
            );
        },
    );
    let object_color_window = object_settings.as_weak();
    object_settings.on_show_color_picker(move |_audio_plugin, index, param, frame, value| {
        let Some(window) = object_color_window.upgrade() else {
            return;
        };
        let [alpha, red, green, blue] = parse_qt_color(value.as_str());
        window.set_picker_effect_index(index);
        window.set_picker_param_name(param);
        window.set_picker_frame(frame.max(0));
        window.set_picker_red(i32::from(red));
        window.set_picker_green(i32::from(green));
        window.set_picker_blue(i32::from(blue));
        window.set_picker_alpha(i32::from(alpha));
        window.set_font_picker_visible(false);
        window.set_color_picker_visible(true);
    });
    let object_color_ui = object_settings_ui.clone();
    object_settings.on_apply_picked_color(move |index, param, frame, red, green, blue, alpha| {
        object_color_ui.set_text(
            false,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            &format_qt_color(red, green, blue, alpha),
        );
    });
    let object_font_window = object_settings.as_weak();
    let object_font_catalog = object_settings_ui.font_families.clone();
    object_settings.on_show_font_picker(move |_audio_plugin, index, param, frame, value| {
        let Some(window) = object_font_window.upgrade() else {
            return;
        };
        window.set_picker_effect_index(index);
        window.set_picker_param_name(param);
        window.set_picker_frame(frame.max(0));
        window.set_current_font_family(value);
        window.set_font_filter(SharedString::new());
        sync_font_families(&window, &object_font_catalog, "");
        window.set_color_picker_visible(false);
        window.set_font_picker_visible(true);
    });
    let object_font_filter_window = object_settings.as_weak();
    let object_font_filter_catalog = object_settings_ui.font_families.clone();
    object_settings.on_filter_fonts(move |query| {
        if let Some(window) = object_font_filter_window.upgrade() {
            sync_font_families(&window, &object_font_filter_catalog, query.as_str());
        }
    });
    let object_font_ui = object_settings_ui.clone();
    object_settings.on_apply_picked_font(move |index, param, frame, family| {
        object_font_ui.set_text(
            false,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            family.as_str(),
        );
    });
    let object_seek_keyframe_ui = object_settings_ui.clone();
    object_settings.on_seek_effect_frame(move |frame| {
        if let Some(workspace) = object_seek_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
        {
            workspace.seek_effect_frame(frame.max(0));
        }
        object_seek_keyframe_ui.sync();
    });
    let object_snap_model = model.clone();
    let object_snap_timeline = timeline.as_weak();
    let object_snap_settings = settings.clone();
    object_settings.on_snap_effect_frame(move |frame| {
        let timeline_scale = object_snap_timeline
            .upgrade()
            .map_or(1.0, |window| f64::from(window.get_pixels_per_frame()));
        let enable_snap = object_snap_settings.borrow().bool_value("enableSnap", true);
        object_snap_model.borrow().current_workspace().map_or_else(
            || frame.round().max(0.0) as i32,
            |workspace| {
                workspace.snap_effect_keyframe_frame(f64::from(frame), timeline_scale, enable_snap)
            },
        )
    });
    let object_add_keyframe_ui = object_settings_ui.clone();
    object_settings.on_add_effect_keyframe(move |audio_plugin, index, param, frame| {
        let _ = object_add_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if audio_plugin {
                    workspace.add_audio_plugin_keyframe(
                        index.max(0) as usize,
                        param.as_str(),
                        frame.max(0),
                    )
                } else {
                    workspace.add_effect_keyframe(
                        index.max(0) as usize,
                        param.as_str(),
                        frame.max(0),
                    )
                }
            });
        object_add_keyframe_ui.sync();
    });
    let object_seed_keyframe_ui = object_settings_ui.clone();
    object_settings.on_seed_audio_plugin_keyframes(move |index, param| {
        let _ = object_seed_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.seed_audio_plugin_keyframes(index.max(0) as usize, param.as_str())
            });
        object_seed_keyframe_ui.sync();
    });
    let object_remove_keyframe_ui = object_settings_ui.clone();
    object_settings.on_remove_effect_keyframe(move |audio_plugin, index, param, frame| {
        let _ = object_remove_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if audio_plugin {
                    workspace.remove_audio_plugin_keyframe(
                        index.max(0) as usize,
                        param.as_str(),
                        frame,
                    )
                } else {
                    workspace.remove_effect_keyframe(index.max(0) as usize, param.as_str(), frame)
                }
            });
        object_remove_keyframe_ui.sync();
    });
    let object_move_keyframe_ui = object_settings_ui.clone();
    object_settings.on_move_effect_keyframe(
        move |audio_plugin, index, param, old_frame, new_frame| {
            let _ = object_move_keyframe_ui
                .model
                .borrow_mut()
                .current_workspace_mut()
                .is_some_and(|workspace| {
                    if audio_plugin {
                        workspace.move_audio_plugin_keyframe(
                            index.max(0) as usize,
                            param.as_str(),
                            old_frame,
                            new_frame,
                        )
                    } else {
                        workspace.move_effect_keyframe(
                            index.max(0) as usize,
                            param.as_str(),
                            old_frame,
                            new_frame,
                        )
                    }
                });
            object_move_keyframe_ui.sync();
        },
    );
    let object_easing_ui = object_settings_ui.clone();
    object_settings.on_open_effect_easing(move |index, param, start_frame, end_frame| {
        object_easing_ui.open_easing(
            index.max(0) as usize,
            param.as_str(),
            start_frame.max(0),
            end_frame.max(0),
        );
    });
    let easing_apply_ui = object_settings_ui.clone();
    let easing_apply_window = easing.as_weak();
    easing.on_apply_easing(
        move |interpolation, step_frames, amplitude, period, x1, y1, x2, y2| {
            let Some(window) = easing_apply_window.upgrade() else {
                return;
            };
            let custom_points = easing_apply_ui.update_easing_custom_points([x1, y1, x2, y2]);
            let options = easing_options(
                interpolation.as_str(),
                step_frames,
                amplitude,
                period,
                &custom_points,
            );
            {
                let curve = easing_apply_ui.easing_curve.borrow();
                sync_easing_preview(&window, &curve);
                sync_easing_catalog(&window, window.get_easing_filter().as_str(), &curve);
            }
            easing_apply_ui.apply_easing(
                window.get_effect_index().max(0) as usize,
                window.get_param_name().as_str(),
                window.get_keyframe_frame().max(0),
                options,
            );
        },
    );
    let easing_custom_window = easing.as_weak();
    easing.on_custom_point_edited(move |index, text| {
        let Some(window) = easing_custom_window.upgrade() else {
            return;
        };
        let mut value = text.as_str().trim().parse::<f32>().unwrap_or(0.0);
        if index == 0 || index == 2 {
            value = value.clamp(0.0, 1.0);
        }
        match index {
            0 => window.set_custom_x1(value),
            1 => window.set_custom_y1(value),
            2 => window.set_custom_x2(value),
            3 => window.set_custom_y2(value),
            _ => return,
        }
        invoke_current_easing(&window);
    });
    let easing_hit_ui = object_settings_ui.clone();
    easing.on_hit_custom_point(move |x, y, tolerance_x, tolerance_y| {
        easing_hit_ui
            .easing_curve
            .borrow()
            .hit_test(
                f64::from(x),
                f64::from(y),
                f64::from(tolerance_x),
                f64::from(tolerance_y),
            )
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1)
    });
    let easing_insert_ui = object_settings_ui.clone();
    let easing_insert_window = easing.as_weak();
    easing.on_insert_custom_anchor(move |x, y| {
        let Some(window) = easing_insert_window.upgrade() else {
            return;
        };
        if easing_insert_ui
            .easing_curve
            .borrow_mut()
            .insert_anchor(f64::from(x), f64::from(y))
        {
            sync_easing_curve(&window, &easing_insert_ui.easing_curve.borrow());
            invoke_current_easing(&window);
        }
    });
    let easing_move_ui = object_settings_ui.clone();
    let easing_move_window = easing.as_weak();
    easing.on_move_custom_point(move |index, x, y| {
        let Some(window) = easing_move_window.upgrade() else {
            return;
        };
        if usize::try_from(index).ok().is_some_and(|index| {
            easing_move_ui
                .easing_curve
                .borrow_mut()
                .move_point(index, f64::from(x), f64::from(y))
        }) {
            sync_easing_curve(&window, &easing_move_ui.easing_curve.borrow());
            invoke_current_easing(&window);
        }
    });
    let easing_remove_ui = object_settings_ui.clone();
    let easing_remove_window = easing.as_weak();
    easing.on_remove_custom_anchor(move |index| {
        let Some(window) = easing_remove_window.upgrade() else {
            return;
        };
        if usize::try_from(index).ok().is_some_and(|index| {
            easing_remove_ui
                .easing_curve
                .borrow_mut()
                .remove_anchor(index)
        }) {
            sync_easing_curve(&window, &easing_remove_ui.easing_curve.borrow());
            invoke_current_easing(&window);
        }
    });
    let easing_filter_window = easing.as_weak();
    let easing_filter_ui = object_settings_ui.clone();
    easing.on_filter_easings(move |query| {
        if let Some(window) = easing_filter_window.upgrade() {
            sync_easing_catalog(
                &window,
                query.as_str(),
                &easing_filter_ui.easing_curve.borrow(),
            );
        }
    });
    let easing_close = easing.as_weak();
    let easing_close_settings = settings.clone();
    easing.on_close_window(move || {
        if let Some(window) = easing_close.upgrade() {
            persist_window_geometry(&easing_close_settings, "easingConfig", window.window());
            let _ = window.hide();
        }
    });
    let object_reorder_ui = object_settings_ui.clone();
    object_settings.on_reorder_effect(move |index, delta_y| {
        object_reorder_ui.reorder_effect(index.max(0) as usize, delta_y);
    });
    let object_filter_window = object_settings.as_weak();
    let object_filter_model = model.clone();
    let object_filter_catalog = effect_catalog.clone();
    let object_filter_audio_catalog = object_settings_ui.audio_catalog.clone();
    object_settings.on_filter_effects(move |query| {
        if let Some(window) = object_filter_window.upgrade() {
            let effect_catalog = object_filter_catalog.borrow();
            sync_object_catalog(
                &window,
                &object_filter_model.borrow(),
                &effect_catalog,
                &object_filter_audio_catalog.borrow(),
                query.as_str(),
            );
        }
    });
    let object_add_ui = object_settings_ui.clone();
    object_settings.on_add_effect(move |effect_id| {
        object_add_ui.add_effect(effect_id.as_str());
    });
    let preset_names_model = model.clone();
    let preset_names_store = preset_store.clone();
    object_settings.on_preset_names(move |index| {
        let effect_id = preset_names_model
            .borrow()
            .current_workspace()
            .and_then(WorkspaceModel::selected_clip_document)
            .and_then(|clip| {
                if clip.clip_type == "audio" {
                    clip.audio_plugins
                        .get(index.max(0) as usize)
                        .map(|plugin| plugin.id.clone())
                } else {
                    clip.effects
                        .get(index.max(0) as usize)
                        .map(|effect| effect.id.clone())
                }
            });
        let names = effect_id.map_or_else(Vec::new, |effect_id| {
            preset_names_store
                .names(&effect_id)
                .into_iter()
                .map(SharedString::from)
                .collect()
        });
        ModelRc::new(VecModel::from(names))
    });
    let preset_save_ui = object_settings_ui.clone();
    object_settings.on_save_effect_preset(move |index, name| {
        preset_save_ui.save_preset(index.max(0) as usize, name.as_str());
    });
    let preset_load_ui = object_settings_ui.clone();
    object_settings.on_load_effect_preset(move |index, name| {
        preset_load_ui.load_preset(index.max(0) as usize, name.as_str());
    });
    let preset_delete_ui = object_settings_ui.clone();
    object_settings.on_delete_effect_preset(move |index, name| {
        preset_delete_ui.delete_preset(index.max(0) as usize, name.as_str());
    });
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
        defaults.width =
            match parse_required_i32(&width, localized("Width", "宽度", "幅"), 1, 8_000) {
                Ok(value) => value,
                Err(message) => {
                    show_error_dialog(&message);
                    return;
                }
            };
        defaults.height =
            match parse_required_i32(&height, localized("Height", "高度", "高さ"), 1, 8_000) {
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
        defaults.sample_rate = match parse_required_i32(
            &sample_rate,
            localized("Sample rate", "采样率", "サンプリングレート"),
            8_000,
            192_000,
        ) {
            Ok(value) => value,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        create_model.borrow_mut().create_project(defaults);
        sync_weak_windows(&create_main, &create_timeline, &create_model);
        if let Some(main) = create_main.upgrade() {
            let _ = show_and_redraw(&main);
        }
        if let Some(timeline) = create_timeline.upgrade() {
            let _ = show_and_redraw(&timeline);
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
            let _ = show_and_redraw(&window);
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
            if let Some(window) = select_timeline.upgrade() {
                window.set_skimmer_visible(false);
            }
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

    let relink_model = model.clone();
    let relink_main = main.as_weak();
    let relink_timeline = timeline.as_weak();
    main.on_relink_missing_media(move |clip_id| {
        let target = relink_model
            .borrow()
            .current_workspace()
            .and_then(|workspace| {
                workspace
                    .missing_media()
                    .into_iter()
                    .find(|entry| entry.clip_id == clip_id)
                    .map(|entry| {
                        let path = PathBuf::from(&entry.path);
                        let suggested_path = if path.is_absolute() {
                            path
                        } else {
                            workspace
                                .project()
                                .path
                                .as_deref()
                                .and_then(Path::parent)
                                .map_or(path.clone(), |directory| directory.join(path))
                        };
                        (entry.clip_type, suggested_path)
                    })
            });
        let Some((clip_type, suggested_path)) = target else {
            return;
        };
        let Some(path) = choose_missing_media_replacement(&clip_type, &suggested_path) else {
            return;
        };
        let error = {
            let mut application = relink_model.borrow_mut();
            application.current_workspace_mut().and_then(|workspace| {
                (!workspace.relink_media(clip_id, &path)).then(|| workspace.status().to_owned())
            })
        };
        if let Some(message) = error {
            show_error_dialog(&message);
        }
        sync_weak_windows(&relink_main, &relink_timeline, &relink_model);
    });

    let recover_ui = lifecycle_ui.clone();
    recovery.on_recover_project(move |id| {
        let recover_ui = recover_ui.clone();
        let id = id.to_string();
        Timer::single_shot(Duration::ZERO, move || {
            let result = recover_ui.model.borrow_mut().recover_project(&id);
            match result {
                Ok(_) => {
                    recover_ui.hydrate_audio_plugins();
                    if let Some(window) = recover_ui.recovery.upgrade() {
                        let _ = window.hide();
                    }
                    recover_ui.sync();
                    if let Some(window) = recover_ui.main.upgrade() {
                        let _ = show_and_redraw(&window);
                    }
                    if let Some(window) = recover_ui.timeline.upgrade() {
                        let _ = show_and_redraw(&window);
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
            let _ = show_and_redraw(&window);
        }
    });
    let settings_window = object_settings.as_weak();
    let settings_model = model.clone();
    let settings_catalog = effect_catalog.clone();
    main.on_show_object_settings(move || {
        if let Some(window) = settings_window.upgrade() {
            sync_object_settings(
                &window,
                &settings_model.borrow(),
                &settings_catalog.borrow(),
            );
            let _ = show_and_redraw(&window);
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
            let _ = show_and_redraw(&window);
        }
    });

    let system_settings_store = settings.clone();
    let system_settings_window = system_settings.as_weak();
    main.on_show_system_settings(move || {
        if let Some(window) = system_settings_window.upgrade() {
            sync_system_settings(&window, &system_settings_store.borrow());
            let _ = show_and_redraw(&window);
        }
    });

    let package_window = package_manager.as_weak();
    let package_open_model = package_manager_model.clone();
    main.on_show_package_manager(move || {
        if let Some(window) = package_window.upgrade() {
            sync_package_manager(&window, &package_open_model.borrow());
            let _ = show_and_redraw(&window);
        }
    });
    let package_tab_window = package_manager.as_weak();
    let package_tab_model = package_manager_model.clone();
    package_manager.on_select_tab(move |_| {
        if let Some(window) = package_tab_window.upgrade() {
            sync_package_manager(&window, &package_tab_model.borrow());
        }
    });
    let package_filter_window = package_manager.as_weak();
    let package_filter_model = package_manager_model.clone();
    package_manager.on_filter_packages(move |_| {
        if let Some(window) = package_filter_window.upgrade() {
            sync_package_manager(&window, &package_filter_model.borrow());
        }
    });
    let package_sync_window = package_manager.as_weak();
    let package_sync_model = package_manager_model.clone();
    let package_sync_runtime = package_operation.clone();
    package_manager.on_sync_repositories(move || {
        if let Some(window) = package_sync_window.upgrade() {
            start_package_operation(
                &window,
                &package_sync_runtime,
                &package_sync_model,
                PackageOperation::Sync,
            );
        }
    });
    let package_install_window = package_manager.as_weak();
    let package_install_model = package_manager_model.clone();
    let package_install_runtime = package_operation.clone();
    package_manager.on_install_package(move |package_id, source_repository| {
        if let Some(window) = package_install_window.upgrade() {
            start_package_operation(
                &window,
                &package_install_runtime,
                &package_install_model,
                PackageOperation::Install {
                    package_id: package_id.to_string(),
                    source_repository: source_repository.to_string(),
                    version: String::new(),
                },
            );
        }
    });
    let package_remove_window = package_manager.as_weak();
    let package_remove_model = package_manager_model.clone();
    let package_remove_runtime = package_operation.clone();
    package_manager.on_remove_package(move |package_id| {
        if let Some(window) = package_remove_window.upgrade() {
            start_package_operation(
                &window,
                &package_remove_runtime,
                &package_remove_model,
                PackageOperation::Remove {
                    package_id: package_id.to_string(),
                },
            );
        }
    });
    let package_upgrade_window = package_manager.as_weak();
    let package_upgrade_model = package_manager_model.clone();
    let package_upgrade_runtime = package_operation.clone();
    package_manager.on_upgrade_all(move || {
        if let Some(window) = package_upgrade_window.upgrade() {
            start_package_operation(
                &window,
                &package_upgrade_runtime,
                &package_upgrade_model,
                PackageOperation::UpgradeAll,
            );
        }
    });
    let permission_window = plugin_permissions.as_weak();
    let permission_parent = package_manager.as_weak();
    let permission_settings = settings.clone();
    package_manager.on_show_permissions(move |plugin_id, plugin_name| {
        if let (Some(window), Some(parent)) =
            (permission_window.upgrade(), permission_parent.upgrade())
        {
            window.set_plugin_id(plugin_id.clone());
            window.set_plugin_name(if plugin_name.is_empty() {
                plugin_id
            } else {
                plugin_name
            });
            sync_plugin_permissions(&window, &permission_settings.borrow());
            let _ = show_centered_and_redraw(&window, &parent);
        }
    });
    let permission_toggle_window = plugin_permissions.as_weak();
    plugin_permissions.on_toggle_permission(move |index, granted| {
        if let Some(window) = permission_toggle_window.upgrade() {
            let mut rows = window.get_permissions().iter().collect::<Vec<_>>();
            if let Some(row) = rows.get_mut(index.max(0) as usize) {
                row.granted = granted;
                update_vec_model(&window.get_permissions(), rows);
            }
        }
    });
    let permission_all_window = plugin_permissions.as_weak();
    plugin_permissions.on_set_all(move |granted| {
        if let Some(window) = permission_all_window.upgrade() {
            let rows = window
                .get_permissions()
                .iter()
                .map(|mut row| {
                    row.granted = granted;
                    row
                })
                .collect();
            update_vec_model(&window.get_permissions(), rows);
        }
    });
    let permission_accept_window = plugin_permissions.as_weak();
    let permission_accept_settings = settings.clone();
    plugin_permissions.on_accept(move || {
        let Some(window) = permission_accept_window.upgrade() else {
            return;
        };
        let granted = window
            .get_permissions()
            .iter()
            .filter(|row| row.granted)
            .map(|row| row.name.to_string())
            .collect::<Vec<_>>();
        match save_plugin_permission_grants(
            &mut permission_accept_settings.borrow_mut(),
            window.get_plugin_id().as_str(),
            &granted,
        ) {
            Ok(()) => {
                let _ = window.hide();
            }
            Err(error) => show_error_dialog(&error),
        }
    });
    let permission_close_window = plugin_permissions.as_weak();
    plugin_permissions.on_close_window(move || {
        if let Some(window) = permission_close_window.upgrade() {
            let _ = window.hide();
        }
    });
    let package_add_window = package_manager.as_weak();
    let package_add_model = package_manager_model.clone();
    let package_add_settings = settings.clone();
    package_manager.on_add_repository(move |url| {
        let result = package_add_model
            .borrow_mut()
            .add_repository(&mut package_add_settings.borrow_mut(), url.as_str());
        if let Some(window) = package_add_window.upgrade() {
            match result {
                Ok(true) => window.set_repository_url(SharedString::new()),
                Ok(false) => {}
                Err(message) => window.set_error_message(SharedString::from(message)),
            }
            sync_package_manager(&window, &package_add_model.borrow());
        }
    });
    let package_enabled_window = package_manager.as_weak();
    let package_enabled_model = package_manager_model.clone();
    let package_enabled_settings = settings.clone();
    package_manager.on_set_repository_enabled(move |url, enabled| {
        let result = package_enabled_model.borrow_mut().set_repository_enabled(
            &mut package_enabled_settings.borrow_mut(),
            url.as_str(),
            enabled,
        );
        if let Some(window) = package_enabled_window.upgrade() {
            if let Err(message) = result {
                window.set_error_message(SharedString::from(message));
            }
            sync_package_manager(&window, &package_enabled_model.borrow());
        }
    });
    let package_repository_remove_window = package_manager.as_weak();
    let package_repository_remove_model = package_manager_model.clone();
    let package_repository_remove_settings = settings.clone();
    package_manager.on_remove_repository(move |url| {
        let result = package_repository_remove_model
            .borrow_mut()
            .remove_repository(
                &mut package_repository_remove_settings.borrow_mut(),
                url.as_str(),
            );
        if let Some(window) = package_repository_remove_window.upgrade() {
            if let Err(message) = result {
                window.set_error_message(SharedString::from(message));
            }
            sync_package_manager(&window, &package_repository_remove_model.borrow());
        }
    });
    let package_error_window = package_manager.as_weak();
    package_manager.on_dismiss_error(move || {
        if let Some(window) = package_error_window.upgrade() {
            window.set_error_message(SharedString::new());
        }
    });
    let package_update_window = package_manager.as_weak();
    package_manager.on_dismiss_update(move || {
        if let Some(window) = package_update_window.upgrade() {
            window.set_update_message(SharedString::new());
        }
    });

    let about_window = about.as_weak();
    main.on_show_about(move || {
        if let Some(window) = about_window.upgrade() {
            let _ = show_and_redraw(&window);
        }
    });
    about.on_open_project_page(move || {
        if let Err(error) = webbrowser::open("https://codeberg.org/taisho-guy/AviQtl") {
            show_error_dialog(&format!("Failed to open the project page: {error}"));
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
    let project_close_settings = settings.clone();
    project_settings.on_close_window(move || {
        if let Some(window) = project_close_window.upgrade() {
            persist_window_geometry(&project_close_settings, "projectSettings", window.window());
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
                    name: format!(
                        "{} {}",
                        localized("Scene", "场景", "シーン"),
                        workspace.document().scenes.len() + 1
                    ),
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
            let _ = show_and_redraw(&window);
        }
    });

    let timeline_settings = object_settings.as_weak();
    let timeline_settings_model = model.clone();
    let timeline_settings_catalog = effect_catalog.clone();
    timeline.on_show_object_settings(move || {
        if let Some(window) = timeline_settings.upgrade() {
            sync_object_settings(
                &window,
                &timeline_settings_model.borrow(),
                &timeline_settings_catalog.borrow(),
            );
            let _ = show_and_redraw(&window);
        }
    });

    let timeline_action_model = model.clone();
    let timeline_action_main = main.as_weak();
    let timeline_action_window = timeline.as_weak();
    let timeline_action_project_settings = project_settings.as_weak();
    let timeline_action_scene_settings = scene_settings.as_weak();
    let timeline_action_system_settings = system_settings.as_weak();
    let timeline_action_settings = settings.clone();
    timeline.on_timeline_action(move |action, frame, layer| {
        match action.as_str() {
            "undo" | "redo" | "paste" => {
                let (pixels_per_frame, last_layer) = timeline_action_window
                    .upgrade()
                    .map_or((1.0, 127), |window| {
                        (window.get_pixels_per_frame(), timeline_last_layer(&window))
                    });
                if let Some(workspace) = timeline_action_model.borrow_mut().current_workspace_mut()
                {
                    match action.as_str() {
                        "undo" => {
                            workspace.undo();
                        }
                        "redo" => {
                            workspace.redo();
                        }
                        "paste" => {
                            let frame = workspace.snap_timeline_frame(
                                f64::from(frame),
                                false,
                                f64::from(pixels_per_frame),
                            );
                            workspace.paste_clips_at(
                                frame,
                                layer.clamp(0, last_layer),
                                last_layer + 1,
                            );
                        }
                        _ => unreachable!(),
                    }
                }
            }
            "scene-settings" => {
                let input =
                    timeline_action_model
                        .borrow()
                        .current_workspace()
                        .and_then(|workspace| {
                            let scene_id = workspace.selected_scene_document()?.id;
                            workspace
                                .scene_settings(scene_id)
                                .map(|settings| (scene_id, settings))
                        });
                if let (Some(window), Some((scene_id, input))) =
                    (timeline_action_scene_settings.upgrade(), input)
                {
                    sync_scene_settings(&window, false, scene_id, &input);
                    let _ = show_and_redraw(&window);
                }
            }
            "project-settings" => {
                let input = timeline_action_model
                    .borrow()
                    .current_workspace()
                    .map(WorkspaceModel::project_settings);
                if let (Some(window), Some(input)) =
                    (timeline_action_project_settings.upgrade(), input)
                {
                    sync_project_settings(&window, &input);
                    let _ = show_and_redraw(&window);
                }
            }
            "system-settings" => {
                if let Some(window) = timeline_action_system_settings.upgrade() {
                    sync_system_settings(&window, &timeline_action_settings.borrow());
                    let _ = show_and_redraw(&window);
                }
            }
            _ => {}
        }
        sync_weak_windows(
            &timeline_action_main,
            &timeline_action_window,
            &timeline_action_model,
        );
    });

    let seek_audio_model = model.clone();
    let seek_audio_main = main.as_weak();
    let seek_audio_timeline = timeline.as_weak();
    timeline.on_seek_audio_frame(move |clip_id, frame| {
        if let Some(workspace) = seek_audio_model.borrow_mut().current_workspace_mut()
            && let Some(clip) = workspace
                .document()
                .clips
                .iter()
                .find(|clip| clip.id == clip_id && clip.scene_id == workspace.selected_scene())
        {
            let start = clip.start;
            let duration = clip.duration;
            if clip.clip_type == "audio" && duration > 0 {
                workspace
                    .seek(start.saturating_add(frame.saturating_sub(start).clamp(0, duration)));
            }
        }
        sync_weak_windows(&seek_audio_main, &seek_audio_timeline, &seek_audio_model);
    });

    let object_filter_window = timeline.as_weak();
    let object_filter_catalog = effect_catalog.clone();
    timeline.on_filter_objects(move |query, category_index| {
        if let Some(window) = object_filter_window.upgrade() {
            sync_timeline_object_catalog(
                &window,
                &object_filter_catalog.borrow(),
                query.as_str(),
                category_index,
            );
        }
    });

    let context_filter_window = timeline.as_weak();
    let context_filter_effect_catalog = effect_catalog.clone();
    let context_filter_audio_catalog = object_settings_ui.audio_catalog.clone();
    timeline.on_filter_context_catalog(move |query, target_kind| {
        if let Some(window) = context_filter_window.upgrade() {
            sync_timeline_context_catalog(
                &window,
                &context_filter_effect_catalog.borrow(),
                &context_filter_audio_catalog.borrow(),
                query.as_str(),
                target_kind,
            );
        }
    });

    let object_add_model = model.clone();
    let object_add_settings = settings.clone();
    let object_add_catalog = effect_catalog.clone();
    let object_add_main = main.as_weak();
    let object_add_timeline = timeline.as_weak();
    timeline.on_add_catalog_object(move |object_id, frame, layer| {
        let (pixels_per_frame, last_layer) =
            object_add_timeline.upgrade().map_or((1.0, 127), |window| {
                (window.get_pixels_per_frame(), timeline_last_layer(&window))
            });
        let default_duration = object_add_settings
            .borrow()
            .i32_value("defaultClipDuration", 100)
            .max(1);
        let catalog = object_add_catalog.borrow();
        let added = object_add_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                let frame = workspace.snap_timeline_frame(
                    f64::from(frame),
                    false,
                    f64::from(pixels_per_frame),
                );
                workspace.insert_catalog_object_at(
                    object_id.as_str(),
                    frame,
                    layer.clamp(0, last_layer),
                    default_duration,
                    &catalog,
                )
            });
        sync_weak_windows(&object_add_main, &object_add_timeline, &object_add_model);
        added
    });

    let scene_model = model.clone();
    let scene_main = main.as_weak();
    let scene_timeline = timeline.as_weak();
    timeline.on_scene_selected(move |scene_id| {
        if let Some(workspace) = scene_model.borrow_mut().current_workspace_mut() {
            workspace.switch_scene(scene_id);
        }
        if let Some(window) = scene_timeline.upgrade() {
            window.set_skimmer_visible(false);
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
        if let Some(window) = close_scene_timeline.upgrade() {
            window.set_skimmer_visible(false);
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
            let _ = show_and_redraw(&window);
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
    let scene_close_settings = settings.clone();
    scene_settings.on_close_window(move || {
        if let Some(window) = scene_close_window.upgrade() {
            persist_window_geometry(&scene_close_settings, "sceneSettings", window.window());
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
    let system_plugin_enabled_window = system_settings.as_weak();
    system_settings.on_plugin_enabled_changed(move |index, enabled| {
        let Some(window) = system_plugin_enabled_window.upgrade() else {
            return;
        };
        let model = window.get_plugin_settings();
        if let Some(mut row) = usize::try_from(index)
            .ok()
            .and_then(|index| model.row_data(index).map(|row| (index, row)))
        {
            row.1.enabled = enabled;
            model.set_row_data(row.0, row.1);
        }
    });
    let system_plugin_paths_window = system_settings.as_weak();
    system_settings.on_plugin_paths_changed(move |index, paths| {
        let Some(window) = system_plugin_paths_window.upgrade() else {
            return;
        };
        let model = window.get_plugin_settings();
        if let Some(mut row) = usize::try_from(index)
            .ok()
            .and_then(|index| model.row_data(index).map(|row| (index, row)))
        {
            row.1.paths = paths;
            model.set_row_data(row.0, row.1);
        }
    });
    let system_shortcut_window = system_settings.as_weak();
    system_settings.on_shortcut_value_changed(move |index, value| {
        let Some(window) = system_shortcut_window.upgrade() else {
            return;
        };
        let model = window.get_shortcut_settings();
        if let Some(mut row) = usize::try_from(index)
            .ok()
            .and_then(|index| model.row_data(index).map(|row| (index, row)))
        {
            row.1.value = value;
            model.set_row_data(row.0, row.1);
        }
    });
    let system_apply_store = settings.clone();
    let system_apply_model = model.clone();
    let system_apply_launcher = launcher.as_weak();
    let system_apply_timeline = timeline.as_weak();
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
                if let Some(timeline) = system_apply_timeline.upgrade() {
                    sync_timeline_zoom_settings(&timeline, &system_apply_store.borrow());
                }
                system_apply_ui.sync_live_settings();
                system_apply_ui.sync();
                true
            }
            Err(message) => {
                show_error_dialog(&message);
                false
            }
        }
    });
    let system_close_window = system_settings.as_weak();
    let system_close_settings = settings.clone();
    system_settings.on_close_window(move || {
        if let Some(window) = system_close_window.upgrade() {
            persist_window_geometry(&system_close_settings, "systemSettings", window.window());
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
    let clip_command_ui = object_settings_ui.clone();
    timeline.on_clip_command(move |action, clip_id, frame, layer| {
        let maximum_layers = clip_command_timeline
            .upgrade()
            .map_or(128, |window| window.get_maximum_layers());
        let open_effect_picker = action.as_str() == "browse-effect";
        let extension_id = action
            .as_str()
            .strip_prefix("add-extension:")
            .map(str::to_owned);
        if let Some(workspace) = clip_command_model.borrow_mut().current_workspace_mut() {
            workspace.context_click_clip(clip_id);
            match action.as_str() {
                "delete" => {
                    workspace.remove_selected_clips();
                }
                "split" => {
                    workspace.split_selected_clips_at(frame);
                }
                "duplicate" => {
                    workspace.duplicate_selected_clips_at(frame, layer, maximum_layers);
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
                _ => {}
            }
        }
        if let Some(extension_id) = extension_id {
            clip_command_ui.add_effect(&extension_id);
            return;
        }
        sync_weak_windows(
            &clip_command_main,
            &clip_command_timeline,
            &clip_command_model,
        );
        if open_effect_picker && let Some(window) = clip_command_settings.upgrade() {
            window.set_effect_filter(SharedString::new());
            clip_command_ui.sync();
            window.set_effect_picker_visible(true);
            let _ = show_and_redraw(&window);
        }
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
        let (pixels_per_frame, layer_height, minimum_duration_frames, maximum_layers) =
            clip_drag_timeline
                .upgrade()
                .map_or((1.0, 30.0, 5, 128), |window| {
                    (
                        window.get_pixels_per_frame(),
                        window.get_timeline_track_height() as f32,
                        window.get_minimum_clip_duration_frames(),
                        window.get_maximum_layers(),
                    )
                });
        if let Some(workspace) = clip_drag_model.borrow_mut().current_workspace_mut() {
            workspace.drag_selected_clips(TimelineDragRequest {
                anchor_clip_id: clip_id,
                kind,
                delta_pixels: (delta_x, delta_y),
                pixels_per_frame,
                layer_height,
                minimum_duration_frames,
                maximum_layers,
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
        let maximum_layers = layer_command_timeline
            .upgrade()
            .map_or(128, |window| window.get_maximum_layers());
        if let Some(workspace) = layer_command_model.borrow_mut().current_workspace_mut() {
            match action.as_str() {
                "insert-above" => {
                    workspace.insert_layers(layer, 1, true, maximum_layers);
                }
                "insert-below" => {
                    workspace.insert_layers(layer, 1, false, maximum_layers);
                }
                "shift-down" => {
                    workspace.shift_layers(layer, layer, 1, maximum_layers);
                }
                "shift-up" => {
                    workspace.shift_layers(layer, layer, -1, maximum_layers);
                }
                "toggle-lock" => {
                    workspace.toggle_layer_lock(layer);
                }
                "toggle-visible" => {
                    workspace.toggle_layer_visibility(layer);
                }
                "show-all" => {
                    workspace.set_all_layers_visible(true, maximum_layers);
                }
                "hide-all" => {
                    workspace.set_all_layers_visible(false, maximum_layers);
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

    let insert_layers_model = model.clone();
    let insert_layers_main = main.as_weak();
    let insert_layers_timeline = timeline.as_weak();
    timeline.on_insert_layers(move |layer, count, above| {
        let maximum_layers = insert_layers_timeline
            .upgrade()
            .map_or(128, |window| window.get_maximum_layers());
        let _ = insert_layers_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.insert_layers(layer, count, above, maximum_layers));
        sync_weak_windows(
            &insert_layers_main,
            &insert_layers_timeline,
            &insert_layers_model,
        );
    });

    let shift_layers_model = model.clone();
    let shift_layers_main = main.as_weak();
    let shift_layers_timeline = timeline.as_weak();
    timeline.on_shift_layers(move |start, end, delta| {
        let maximum_layers = shift_layers_timeline
            .upgrade()
            .map_or(128, |window| window.get_maximum_layers());
        let _ = shift_layers_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.shift_layers(start, end, delta, maximum_layers));
        sync_weak_windows(
            &shift_layers_main,
            &shift_layers_timeline,
            &shift_layers_model,
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

    let skimmer_model = model.clone();
    let skimmer_window = timeline.as_weak();
    timeline.on_skimmer_hovered(move |frame, layer, ignore_snap| {
        let Some(window) = skimmer_window.upgrade() else {
            return;
        };
        let snapped_frame = skimmer_model
            .borrow()
            .current_workspace()
            .map_or(0, |workspace| {
                workspace.snap_timeline_frame(
                    f64::from(frame),
                    ignore_snap,
                    f64::from(window.get_pixels_per_frame()),
                )
            });
        window.set_skimmer_frame(snapped_frame);
        window.set_skimmer_layer(layer.clamp(0, timeline_last_layer(&window)));
        window.set_skimmer_visible(true);
    });
    let skimmer_leave_window = timeline.as_weak();
    timeline.on_skimmer_left(move || {
        if let Some(window) = skimmer_leave_window.upgrade() {
            window.set_skimmer_visible(false);
        }
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
    let speed_model = model.clone();
    let speed_main = main.as_weak();
    let speed_timeline = timeline.as_weak();
    main.on_playback_speed_changed(move |percent| {
        if let Some(workspace) = speed_model.borrow_mut().current_workspace_mut() {
            workspace.set_playback_speed(f64::from(percent.clamp(10, 400)) / 100.0);
        }
        sync_transport_weak(&speed_main, &speed_timeline, &speed_model);
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

    let scrub_begin_model = model.clone();
    timeline.on_begin_scrub(move || {
        if let Some(workspace) = scrub_begin_model.borrow_mut().current_workspace_mut() {
            workspace.begin_scrub();
        }
    });
    let scrub_model = model.clone();
    let scrub_main = main.as_weak();
    let scrub_timeline = timeline.as_weak();
    timeline.on_scrub_to(move |frame| {
        if let Some(workspace) = scrub_model.borrow_mut().current_workspace_mut() {
            workspace.scrub_to(frame.round() as i32);
        }
        sync_transport_weak(&scrub_main, &scrub_timeline, &scrub_model);
    });
    let scrub_end_model = model;
    let scrub_end_main = main.as_weak();
    let scrub_end_timeline = timeline.as_weak();
    timeline.on_end_scrub(move || {
        if let Some(workspace) = scrub_end_model.borrow_mut().current_workspace_mut() {
            workspace.end_scrub();
        }
        sync_transport_weak(&scrub_end_main, &scrub_end_timeline, &scrub_end_model);
    });
}

fn install_keyboard_shortcuts(
    main: &MainWindow,
    timeline: &TimelineWindow,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
) {
    let main_window = main.as_weak();
    let main_timeline = timeline.as_weak();
    let main_model = model.clone();
    let main_settings = settings.clone();
    main.on_keyboard_shortcut(move |text, alt, control, shift, meta| {
        handle_keyboard_shortcut(
            ShortcutInput {
                text: text.to_string(),
                alt,
                control,
                shift,
                meta,
            },
            true,
            false,
            &main_window,
            &main_timeline,
            &main_model,
            &main_settings,
        )
    });

    let timeline_main = main.as_weak();
    let timeline_window = timeline.as_weak();
    let timeline_model = model.clone();
    let timeline_settings = settings.clone();
    timeline.on_keyboard_shortcut(move |text, alt, control, shift, meta| {
        handle_keyboard_shortcut(
            ShortcutInput {
                text: text.to_string(),
                alt,
                control,
                shift,
                meta,
            },
            true,
            true,
            &timeline_main,
            &timeline_window,
            &timeline_model,
            &timeline_settings,
        )
    });

    let scroll_window = timeline.as_weak();
    let scroll_settings = settings.clone();
    timeline.on_timeline_scrolled(
        move |delta_x,
              delta_y,
              content_x,
              viewport_x,
              viewport_y,
              visible_width,
              viewport_width,
              visible_height,
              viewport_height,
              duration_frames,
              alt,
              control,
              shift,
              ruler_zoom| {
            let pixels_per_frame = scroll_window
                .upgrade()
                .map_or(1.0, |window| window.get_pixels_per_frame());
            let plan = plan_timeline_scroll(
                TimelineScrollInput {
                    delta_x,
                    delta_y,
                    content_x,
                    viewport_x,
                    viewport_y,
                    visible_width,
                    viewport_width,
                    visible_height,
                    viewport_height,
                    duration_frames,
                    alt,
                    control,
                    shift,
                    ruler_zoom,
                    pixels_per_frame,
                },
                &scroll_settings.borrow(),
            );
            TimelineViewportData {
                pixels_per_frame: plan.pixels_per_frame,
                viewport_x: plan.viewport_x,
                viewport_y: plan.viewport_y,
            }
        },
    );
}

fn handle_keyboard_shortcut(
    input: ShortcutInput,
    editor_window: bool,
    use_skimmer: bool,
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
    settings: &Rc<RefCell<SettingsStore>>,
) -> bool {
    let action = {
        let settings = settings.borrow();
        configured_shortcut_action(&settings, &input, editor_window)
    };
    let Some(action) = action else {
        return false;
    };
    dispatch_shortcut(action, use_skimmer, main, timeline, model, settings);
    true
}

fn configured_shortcut_action(
    settings: &SettingsStore,
    input: &ShortcutInput,
    editor_window: bool,
) -> Option<ShortcutAction> {
    const BINDINGS: [(ShortcutAction, &str, &str, bool); 34] = [
        (ShortcutAction::NewProject, "project.new", "Ctrl+N", false),
        (ShortcutAction::OpenProject, "project.open", "Ctrl+O", false),
        (
            ShortcutAction::SystemSettings,
            "app.settings",
            "Ctrl+P",
            false,
        ),
        (ShortcutAction::Quit, "app.quit", "Ctrl+Q", false),
        (ShortcutAction::SaveProject, "project.save", "Ctrl+S", false),
        (
            ShortcutAction::SaveProjectAs,
            "project.saveAs",
            "Ctrl+Shift+S",
            false,
        ),
        (
            ShortcutAction::ProjectSettings,
            "project.settings",
            "Alt+Enter",
            false,
        ),
        (
            ShortcutAction::ExportMedia,
            "project.export",
            "Ctrl+E",
            false,
        ),
        (ShortcutAction::ShowTimeline, "view.timeline", "F3", false),
        (
            ShortcutAction::ShowObjectSettings,
            "view.objectSettings",
            "F4",
            false,
        ),
        (ShortcutAction::ZoomIn, "view.zoomIn", "Ctrl++", false),
        (ShortcutAction::ZoomOut, "view.zoomOut", "Ctrl+-", false),
        (ShortcutAction::Undo, "edit.undo", "Ctrl+Z", false),
        (ShortcutAction::Redo, "edit.redo", "Ctrl+Shift+Z", false),
        (ShortcutAction::Copy, "edit.copy", "Ctrl+C", true),
        (ShortcutAction::Cut, "edit.cut", "Ctrl+X", true),
        (ShortcutAction::Paste, "edit.paste", "Ctrl+V", true),
        (ShortcutAction::Duplicate, "edit.duplicate", "Ctrl+D", true),
        (ShortcutAction::Delete, "edit.delete", "Delete", true),
        (ShortcutAction::Split, "timeline.split", "S", true),
        (ShortcutAction::MoveUp, "timeline.moveUp", "Alt+Up", true),
        (
            ShortcutAction::MoveDown,
            "timeline.moveDown",
            "Alt+Down",
            true,
        ),
        (
            ShortcutAction::NudgeLeft,
            "timeline.nudgeLeft",
            "Alt+Left",
            true,
        ),
        (
            ShortcutAction::NudgeRight,
            "timeline.nudgeRight",
            "Alt+Right",
            true,
        ),
        (
            ShortcutAction::ToggleLayerLock,
            "timeline.layerLock",
            "Ctrl+L",
            true,
        ),
        (
            ShortcutAction::ToggleLayerVisibility,
            "timeline.layerHide",
            "Ctrl+H",
            true,
        ),
        (
            ShortcutAction::AddScene,
            "timeline.addScene",
            "Ctrl+T",
            false,
        ),
        (
            ShortcutAction::SceneSettings,
            "timeline.sceneSettings",
            "Alt+S",
            true,
        ),
        (
            ShortcutAction::RemoveScene,
            "timeline.removeScene",
            "Ctrl+Shift+Delete",
            true,
        ),
        (
            ShortcutAction::PlayPause,
            "transport.playPause",
            "Space",
            false,
        ),
        (
            ShortcutAction::PreviousFrame,
            "transport.prevFrame",
            "Left",
            false,
        ),
        (
            ShortcutAction::NextFrame,
            "transport.nextFrame",
            "Right",
            false,
        ),
        (
            ShortcutAction::JumpStart,
            "transport.jumpStart",
            "Home",
            false,
        ),
        (ShortcutAction::JumpEnd, "transport.jumpEnd", "End", false),
    ];

    BINDINGS
        .iter()
        .find_map(|(action, key, fallback, editor_only)| {
            if *editor_only && !editor_window {
                return None;
            }
            let configured = shortcut_setting(settings, key, fallback);
            shortcut_matches(&configured, input).then_some(*action)
        })
}

fn shortcut_setting(settings: &SettingsStore, key: &str, fallback: &str) -> String {
    settings
        .value("shortcuts")
        .and_then(serde_json::Value::as_object)
        .and_then(|shortcuts| shortcuts.get(key))
        .and_then(serde_json::Value::as_str)
        .filter(|shortcut| !shortcut.trim().is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn shortcut_matches(value: &str, input: &ShortcutInput) -> bool {
    let Some(pattern) = parse_shortcut(value) else {
        return false;
    };
    pattern.text == input.text.to_lowercase()
        && pattern.alt == input.alt
        && pattern.control == input.control
        && pattern.meta == input.meta
        && (pattern.ignore_shift || pattern.shift == input.shift)
}

fn parse_shortcut(value: &str) -> Option<ShortcutPattern> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let plus_key = value.ends_with('+');
    let mut parts = value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let key_name = if plus_key { "+" } else { parts.pop()? };
    let mut pattern = ShortcutPattern {
        text: String::new(),
        alt: false,
        control: false,
        shift: false,
        meta: false,
        ignore_shift: key_name == "+",
    };
    for modifier in parts {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" => pattern.control = true,
            "cmd" | "command" => {
                if cfg!(target_os = "macos") {
                    pattern.control = true;
                } else {
                    pattern.meta = true;
                }
            }
            "control" => {
                if cfg!(target_os = "macos") {
                    pattern.meta = true;
                } else {
                    pattern.control = true;
                }
            }
            "meta" | "super" => pattern.meta = true,
            "alt" | "option" => pattern.alt = true,
            "shift" => pattern.shift = true,
            _ => return None,
        }
    }
    pattern.text = shortcut_key_text(key_name)?;
    Some(pattern)
}

fn shortcut_key_text(value: &str) -> Option<String> {
    let named = match value.trim().to_ascii_lowercase().as_str() {
        "left" | "arrowleft" => Some(Key::LeftArrow),
        "right" | "arrowright" => Some(Key::RightArrow),
        "up" | "arrowup" => Some(Key::UpArrow),
        "down" | "arrowdown" => Some(Key::DownArrow),
        "escape" | "esc" => Some(Key::Escape),
        "tab" => Some(Key::Tab),
        "backspace" => Some(Key::Backspace),
        "enter" | "return" => Some(Key::Return),
        "space" => Some(Key::Space),
        "delete" | "del" => Some(Key::Delete),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        _ => None,
    };
    if let Some(key) = named {
        return Some(char::from(key).to_string());
    }
    let text = value.trim().to_lowercase();
    (text.chars().count() == 1).then_some(text)
}

fn dispatch_shortcut(
    action: ShortcutAction,
    use_skimmer: bool,
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
    settings: &Rc<RefCell<SettingsStore>>,
) {
    let Some(main_window) = main.upgrade() else {
        return;
    };
    let Some(timeline_window) = timeline.upgrade() else {
        return;
    };
    match action {
        ShortcutAction::NewProject => main_window.invoke_new_project(),
        ShortcutAction::OpenProject => main_window.invoke_open_project(),
        ShortcutAction::SaveProject => main_window.invoke_save_project(),
        ShortcutAction::SaveProjectAs => main_window.invoke_save_project_as(),
        ShortcutAction::ExportMedia => main_window.invoke_export_media(),
        ShortcutAction::Quit => main_window.invoke_quit_requested(),
        ShortcutAction::SystemSettings => main_window.invoke_show_system_settings(),
        ShortcutAction::Undo => main_window.invoke_undo(),
        ShortcutAction::Redo => main_window.invoke_redo(),
        ShortcutAction::PlayPause => main_window.invoke_toggle_playback(),
        ShortcutAction::PreviousFrame => main_window.invoke_previous_frame(),
        ShortcutAction::NextFrame => main_window.invoke_next_frame(),
        ShortcutAction::ShowTimeline => main_window.invoke_show_timeline(),
        ShortcutAction::ShowObjectSettings => main_window.invoke_show_object_settings(),
        ShortcutAction::ProjectSettings => main_window.invoke_show_project_settings(),
        ShortcutAction::AddScene => timeline_window.invoke_show_scene_settings(),
        ShortcutAction::SceneSettings => {
            if let Some(scene_id) = model
                .borrow()
                .current_workspace()
                .map(WorkspaceModel::selected_scene)
            {
                timeline_window.invoke_scene_settings(scene_id);
            }
        }
        ShortcutAction::ZoomIn | ShortcutAction::ZoomOut => {
            let direction = if action == ShortcutAction::ZoomIn {
                1
            } else {
                -1
            };
            step_timeline_zoom(&timeline_window, &settings.borrow(), direction);
        }
        ShortcutAction::Copy
        | ShortcutAction::Cut
        | ShortcutAction::Paste
        | ShortcutAction::Delete
        | ShortcutAction::Duplicate
        | ShortcutAction::JumpStart
        | ShortcutAction::JumpEnd
        | ShortcutAction::Split
        | ShortcutAction::MoveUp
        | ShortcutAction::MoveDown
        | ShortcutAction::NudgeLeft
        | ShortcutAction::NudgeRight
        | ShortcutAction::RemoveScene
        | ShortcutAction::ToggleLayerLock
        | ShortcutAction::ToggleLayerVisibility => {
            if let Some(workspace) = model.borrow_mut().current_workspace_mut() {
                let skimmer_targets = use_skimmer && timeline_window.get_skimmer_visible();
                let maximum_layers = timeline_window.get_maximum_layers();
                let selected_layer = workspace.selected_layer();
                let frame = if skimmer_targets {
                    timeline_window.get_skimmer_frame()
                } else {
                    workspace.playhead()
                };
                let layer = if skimmer_targets {
                    timeline_window.get_skimmer_layer()
                } else {
                    selected_layer
                };
                match action {
                    ShortcutAction::Copy => {
                        workspace.copy_selected_clips();
                    }
                    ShortcutAction::Cut => {
                        workspace.cut_selected_clips();
                    }
                    ShortcutAction::Paste => {
                        if let Some((next_frame, next_layer)) =
                            workspace.paste_clips_at(frame, layer, maximum_layers)
                        {
                            advance_shortcut_target(
                                workspace,
                                &timeline_window,
                                skimmer_targets,
                                next_frame,
                                next_layer,
                            );
                        }
                    }
                    ShortcutAction::Delete => {
                        workspace.remove_selected_clips();
                    }
                    ShortcutAction::Duplicate => {
                        if let Some((next_frame, next_layer)) =
                            workspace.duplicate_selected_clips_at(frame, layer, maximum_layers)
                        {
                            advance_shortcut_target(
                                workspace,
                                &timeline_window,
                                skimmer_targets,
                                next_frame,
                                next_layer,
                            );
                        }
                    }
                    ShortcutAction::JumpStart => workspace.seek(0),
                    ShortcutAction::JumpEnd => {
                        let end_frame = workspace.timeline_duration();
                        workspace.seek(end_frame);
                    }
                    ShortcutAction::Split => {
                        workspace.split_selected_clips_at(frame);
                    }
                    ShortcutAction::MoveUp => {
                        workspace.move_selected_clips(-1, 0, maximum_layers);
                    }
                    ShortcutAction::MoveDown => {
                        workspace.move_selected_clips(1, 0, maximum_layers);
                    }
                    ShortcutAction::NudgeLeft => {
                        workspace.move_selected_clips(0, -1, maximum_layers);
                    }
                    ShortcutAction::NudgeRight => {
                        workspace.move_selected_clips(0, 1, maximum_layers);
                    }
                    ShortcutAction::RemoveScene => {
                        workspace.remove_scene(workspace.selected_scene());
                    }
                    ShortcutAction::ToggleLayerLock => {
                        workspace.toggle_layer_lock(selected_layer);
                    }
                    ShortcutAction::ToggleLayerVisibility => {
                        workspace.toggle_layer_visibility(selected_layer);
                    }
                    _ => unreachable!("direct timeline shortcut is exhaustively matched"),
                }
            }
            sync_weak_windows(main, timeline, model);
        }
    }
}

fn advance_shortcut_target(
    workspace: &mut WorkspaceModel,
    timeline: &TimelineWindow,
    skimmer_targets: bool,
    frame: i32,
    layer: i32,
) {
    if skimmer_targets {
        timeline.set_skimmer_frame(frame.max(0));
        timeline.set_skimmer_layer(layer.clamp(0, timeline_last_layer(timeline)));
    } else {
        workspace.set_edit_target(frame, layer);
    }
}

fn step_timeline_zoom(window: &TimelineWindow, settings: &SettingsStore, direction: i32) {
    window.set_pixels_per_frame(stepped_timeline_scale(
        window.get_pixels_per_frame(),
        settings,
        direction,
    ));
}

fn stepped_timeline_scale(current_scale: f32, settings: &SettingsStore, direction: i32) -> f32 {
    stepped_timeline_scale_with(current_scale, direction, timeline_zoom_settings(settings))
}

fn stepped_timeline_scale_with(
    current_scale: f32,
    direction: i32,
    (minimum, maximum, step): (f32, f32, f32),
) -> f32 {
    let current = scale_to_zoom_percent(current_scale);
    let next = (current + direction.signum() as f32 * step).clamp(minimum, maximum);
    zoom_percent_to_scale(next)
}

fn plan_timeline_scroll(
    input: TimelineScrollInput,
    settings: &SettingsStore,
) -> TimelineScrollPlan {
    plan_timeline_scroll_with(input, timeline_zoom_settings(settings))
}

fn plan_timeline_scroll_with(
    input: TimelineScrollInput,
    zoom_settings: (f32, f32, f32),
) -> TimelineScrollPlan {
    let mut plan = TimelineScrollPlan {
        pixels_per_frame: input.pixels_per_frame,
        viewport_x: input.viewport_x,
        viewport_y: input.viewport_y,
    };
    let dominant_delta = if input.delta_x.abs() > input.delta_y.abs() {
        input.delta_x
    } else {
        input.delta_y
    };
    if dominant_delta.abs() <= f32::EPSILON {
        return plan;
    }

    if input.ruler_zoom || input.alt || input.control {
        let direction = if dominant_delta > 0.0 { 1 } else { -1 };
        let new_scale = if input.ruler_zoom {
            let (minimum, maximum, _) = zoom_settings;
            (input.pixels_per_frame * if direction > 0 { 1.1 } else { 0.9 }).clamp(
                zoom_percent_to_scale(minimum),
                zoom_percent_to_scale(maximum),
            )
        } else {
            stepped_timeline_scale_with(input.pixels_per_frame, direction, zoom_settings)
        };
        let mouse_x = input.content_x + input.viewport_x;
        let anchor_frame = input.content_x / input.pixels_per_frame.max(f32::EPSILON);
        let new_content_x = anchor_frame * new_scale - mouse_x;
        plan.pixels_per_frame = new_scale;
        plan.viewport_x = clamp_viewport(
            -new_content_x,
            input.visible_width,
            (input.duration_frames * new_scale).max(input.visible_width),
        );
    } else if input.shift {
        plan.viewport_y = clamp_viewport(
            input.viewport_y + input.delta_y,
            input.visible_height,
            input.viewport_height,
        );
    } else {
        plan.viewport_x = clamp_viewport(
            input.viewport_x + dominant_delta,
            input.visible_width,
            input.viewport_width,
        );
    }
    plan
}

fn timeline_zoom_settings(settings: &SettingsStore) -> (f32, f32, f32) {
    let minimum = settings.i32_value("timelineZoomMin", 10).clamp(1, 400) as f32;
    let maximum = settings
        .i32_value("timelineZoomMax", 400)
        .clamp(minimum as i32, 1_000) as f32;
    let step = settings.i32_value("timelineZoomStep", 10).clamp(1, 100) as f32;
    (minimum, maximum, step)
}

fn sync_timeline_zoom_settings(window: &TimelineWindow, settings: &SettingsStore) {
    let (minimum, maximum, _) = timeline_zoom_settings(settings);
    window.set_zoom_min(minimum.round() as i32);
    window.set_zoom_max(maximum.round() as i32);
    let current = scale_to_zoom_percent(window.get_pixels_per_frame()).clamp(minimum, maximum);
    window.set_pixels_per_frame(zoom_percent_to_scale(current));
}

fn system_theme_index(settings: &SettingsStore) -> i32 {
    choice_index_str(
        &setting_string(settings, "theme", "Dark"),
        &SYSTEM_THEME_VALUES,
        0,
    )
}

fn timeline_maximum_layers(settings: &SettingsStore) -> i32 {
    settings.i32_value("timelineMaxLayers", 128).clamp(1, 512)
}

fn timeline_last_layer(window: &TimelineWindow) -> i32 {
    window.get_maximum_layers().clamp(1, 512) - 1
}

fn sync_timeline_runtime_settings(
    main: &MainWindow,
    timeline: &TimelineWindow,
    object_settings: &ObjectSettingsWindow,
    settings: &SettingsStore,
) {
    let header_height = settings
        .i32_value("timelineHeaderHeight", 28)
        .clamp(16, 100);
    main.set_project_tab_height(header_height);
    timeline.set_timeline_header_height(header_height);
    timeline
        .set_timeline_track_height(settings.i32_value("timelineTrackHeight", 30).clamp(16, 100));
    timeline
        .set_timeline_ruler_height(settings.i32_value("timelineRulerHeight", 32).clamp(16, 100));
    timeline.set_maximum_layers(timeline_maximum_layers(settings));
    timeline.set_layer_header_width(
        settings
            .i32_value("timelineLayerHeaderWidth", 60)
            .clamp(40, 300),
    );
    timeline.set_clip_resize_handle_width(
        settings
            .i32_value("timelineClipResizeHandleWidth", 10)
            .clamp(4, 40),
    );
    timeline.set_minimum_clip_duration_frames(
        settings.i32_value("minClipDurationFrames", 5).clamp(1, 100),
    );
    let skimming_enabled = settings.bool_value("enableTimelineSkimming", true);
    timeline.set_timeline_skimming_enabled(skimming_enabled);
    if !skimming_enabled {
        timeline.set_skimmer_visible(false);
    }
    object_settings.set_sidebar_on_right(settings.bool_value("settingDialogSidebarRight", false));
}

fn clamp_viewport(value: f32, visible: f32, viewport: f32) -> f32 {
    value.clamp((visible - viewport).min(0.0), 0.0)
}

fn zoom_percent_to_scale(percent: f32) -> f32 {
    let percent = percent.max(1.0);
    if percent <= 100.0 {
        percent / 100.0
    } else {
        1.0 + (percent - 100.0) * 9.0 / 300.0
    }
}

fn scale_to_zoom_percent(scale: f32) -> f32 {
    if scale <= 1.0 {
        scale * 100.0
    } else {
        100.0 + (scale - 1.0) * 300.0 / 9.0
    }
}

fn initialize_export_draft(window: &ExportWindow, settings: &SettingsStore) {
    window.set_format_index(0);
    window.set_image_format_index(0);
    window.set_crf_mode(true);
    window.set_bitrate_mode(false);
    window.set_crf(settings.i32_value("exportDefaultCrf", 20).clamp(0, 51) as f32);
    window.set_bitrate_mbps(
        settings
            .i32_value("exportDefaultBitrateMbps", 15)
            .clamp(1, 500),
    );
    window.set_preset_index(2);
    window.set_profile_index(0);
    let audio_bitrate =
        nearest_audio_bitrate(settings.i32_value("exportDefaultAudioBitrateKbps", 192));
    window.set_audio_bitrate_index(
        AUDIO_BITRATES
            .iter()
            .position(|candidate| *candidate == audio_bitrate)
            .unwrap_or(2) as i32,
    );
    window.set_full_range(true);
    window.set_start_frame(0);
}

#[allow(clippy::too_many_arguments)]
fn install_export_callbacks(
    main: &MainWindow,
    export: &ExportWindow,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    manager: Rc<RefCell<ExportManager>>,
    codecs: Rc<RefCell<ExportCodecState>>,
    planner: Rc<RefCell<ExportPlannerRuntime>>,
) {
    let open_window = export.as_weak();
    let open_model = model.clone();
    let open_settings = settings.clone();
    let open_manager = manager.clone();
    let open_codecs = codecs.clone();
    main.on_export_media(move || {
        let Some(window) = open_window.upgrade() else {
            return;
        };
        if !open_manager.borrow().is_exporting() {
            let application = open_model.borrow();
            let Some(workspace) = application.current_workspace() else {
                return;
            };
            sync_export_window(
                &window,
                workspace,
                &open_settings.borrow(),
                &mut open_codecs.borrow_mut(),
            );
        }
        let _ = show_and_redraw(&window);
    });

    export.on_validate_output_path(|path| valid_export_path(&path));

    let browse_window = export.as_weak();
    export.on_browse_output(move |choose_folder| {
        let Some(window) = browse_window.upgrade() else {
            return;
        };
        if let Some(path) = choose_export_output(choose_folder, &window.get_output_path()) {
            window.set_output_path(path.to_string_lossy().into_owned().into());
        }
    });

    let video_window = export.as_weak();
    let video_codecs = codecs.clone();
    export.on_video_codec_selected(move |index| {
        let Some(window) = video_window.upgrade() else {
            return;
        };
        let codecs = video_codecs.borrow();
        let software = is_software_codec(selected_codec(&codecs.video_values, index, "libx264"));
        window.set_software_codec(software);
    });

    let audio_window = export.as_weak();
    let audio_codecs = codecs.clone();
    export.on_audio_codec_selected(move |index| {
        let Some(window) = audio_window.upgrade() else {
            return;
        };
        let codecs = audio_codecs.borrow();
        let codec = selected_codec(&codecs.audio_values, index, "aac");
        window.set_audio_bitrate_enabled(codec != "flac" && codec != "pcm_s16le");
    });

    let start_window = export.as_weak();
    let start_model = model.clone();
    let start_settings = settings.clone();
    let start_manager = manager.clone();
    let start_codecs = codecs.clone();
    let start_planner = planner.clone();
    export.on_start_export(move || {
        let Some(window) = start_window.upgrade() else {
            return;
        };
        let Some((project_instance_id, project)) = current_export_project(&start_model.borrow())
        else {
            show_export_result(
                &window,
                false,
                localized(
                    "There is no project to export.",
                    "没有可导出的项目。",
                    "書き出すプロジェクトがありません",
                ),
            );
            return;
        };
        let request = match export_request(&window, &start_codecs.borrow()) {
            Ok(request) => request,
            Err(message) => {
                show_export_result(&window, false, &message);
                return;
            }
        };
        let runtime_settings = export_runtime_settings(&start_settings.borrow());
        match start_manager.borrow_mut().start(
            project_instance_id,
            project,
            request,
            runtime_settings,
        ) {
            Ok(total_frames) => {
                start_planner.borrow_mut().reset();
                if let Some(workspace) = start_model.borrow_mut().current_workspace_mut() {
                    workspace.pause_playback();
                }
                window.set_exporting(true);
                window.set_export_progress(0.0);
                window.set_progress_label(
                    format!(
                        "0 / {total_frames} {}",
                        localized("frames", "帧", "フレーム")
                    )
                    .into(),
                );
                window.set_cancel_confirmation_visible(false);
                window.set_result_visible(false);
            }
            Err(error) => show_export_result(&window, false, &error.to_string()),
        }
    });

    let close_window = export.as_weak();
    let close_manager = manager.clone();
    export.on_close_window(move || {
        let Some(window) = close_window.upgrade() else {
            return;
        };
        if close_manager.borrow().is_exporting() {
            window.set_cancel_confirmation_visible(true);
        } else {
            let _ = window.hide();
        }
    });

    let native_close_window = export.as_weak();
    let native_close_manager = manager.clone();
    export.window().on_close_requested(move || {
        let Some(window) = native_close_window.upgrade() else {
            return CloseRequestResponse::HideWindow;
        };
        if native_close_manager.borrow().is_exporting() {
            window.set_cancel_confirmation_visible(true);
            CloseRequestResponse::KeepWindowShown
        } else {
            CloseRequestResponse::HideWindow
        }
    });

    let cancel_window = export.as_weak();
    export.on_request_cancel(move || {
        if let Some(window) = cancel_window.upgrade() {
            window.set_cancel_confirmation_visible(true);
        }
    });

    let confirm_window = export.as_weak();
    let confirm_manager = manager.clone();
    export.on_confirm_cancel(move || {
        confirm_manager.borrow_mut().cancel();
        if let Some(window) = confirm_window.upgrade() {
            window.set_cancel_confirmation_visible(false);
        }
    });

    let dismiss_cancel_window = export.as_weak();
    export.on_dismiss_cancel(move || {
        if let Some(window) = dismiss_cancel_window.upgrade() {
            window.set_cancel_confirmation_visible(false);
        }
    });

    let dismiss_result_window = export.as_weak();
    export.on_dismiss_result(move || {
        if let Some(window) = dismiss_result_window.upgrade() {
            window.set_result_visible(false);
        }
    });
}

fn sync_export_window(
    window: &ExportWindow,
    workspace: &WorkspaceModel,
    settings: &SettingsStore,
    codecs: &mut ExportCodecState,
) {
    let project = workspace.project_settings();
    let duration = workspace.timeline_duration();
    window.set_project_width(project.width);
    window.set_project_height(project.height);
    window.set_project_fps(project.fps as f32);
    window.set_timeline_duration(duration);
    window.set_end_frame(duration);
    window.set_export_progress(0.0);
    window.set_progress_label(format!("0 / 0 {}", localized("frames", "帧", "フレーム")).into());
    window.set_cancel_confirmation_visible(false);

    let available_video = available_video_encoders();
    let (video_labels, video_values) = filtered_codec_catalog(&available_video, &VIDEO_CODECS);
    let default_video = setting_string(settings, "exportDefaultCodec", "libx264");
    let video_index = selected_codec_index(&video_values, &default_video);
    codecs.video_values = video_values;
    window.set_video_codec_labels(ModelRc::new(VecModel::from(video_labels)));
    window.set_video_codec_index(video_index);
    let video_codec = selected_codec(&codecs.video_values, video_index, "libx264");
    window.set_software_codec(is_software_codec(video_codec));

    let available_audio = available_audio_encoders();
    let (audio_labels, audio_values) = filtered_codec_catalog(&available_audio, &AUDIO_CODECS);
    let default_audio = match setting_string(settings, "exportDefaultAudioCodec", "aac").as_str() {
        "opus" => "libopus".to_owned(),
        value => value.to_owned(),
    };
    let audio_index = selected_codec_index(&audio_values, &default_audio);
    codecs.audio_values = audio_values;
    window.set_audio_codec_labels(ModelRc::new(VecModel::from(audio_labels)));
    window.set_audio_codec_index(audio_index);
    let audio_codec = selected_codec(&codecs.audio_values, audio_index, "aac");
    window.set_audio_bitrate_enabled(audio_codec != "flac" && audio_codec != "pcm_s16le");
}

fn current_export_project(model: &ApplicationModel) -> Option<(u64, ExportProject)> {
    let project_instance_id = model.current_project_instance_id()?;
    let workspace = model.current_workspace()?;
    let settings = workspace.project_settings();
    let duration = workspace.timeline_duration();
    Some((
        project_instance_id,
        ExportProject {
            width: settings.width,
            height: settings.height,
            fps: settings.fps,
            duration,
            sample_rate: settings.sample_rate,
        },
    ))
}

fn export_workspace_for_frame(
    model: &mut ApplicationModel,
    project_instance_id: u64,
) -> Result<&mut WorkspaceModel, String> {
    if model.current_project_instance_id() != Some(project_instance_id) {
        return Err("Frame render error: project tab changed during export".to_owned());
    }
    model
        .current_workspace_mut()
        .ok_or_else(|| "Frame render error: export project is no longer available".to_owned())
}

fn export_request(
    window: &ExportWindow,
    codecs: &ExportCodecState,
) -> Result<ExportRequest, String> {
    let output_path = window.get_output_path().to_string();
    if !valid_export_path(&output_path) {
        return Err(localized(
            "Choose a valid export destination.",
            "请选择有效的导出位置。",
            "有効な書き出し先を指定してください",
        )
        .to_owned());
    }
    let video_codec = selected_codec(
        &codecs.video_values,
        window.get_video_codec_index(),
        "libx264",
    )
    .to_owned();
    let audio_codec =
        selected_codec(&codecs.audio_values, window.get_audio_codec_index(), "aac").to_owned();
    let preset = PRESET_VALUES
        .get(window.get_preset_index().max(0) as usize)
        .copied()
        .unwrap_or("medium")
        .to_owned();
    let profile = PROFILE_VALUES
        .get(window.get_profile_index().max(0) as usize)
        .copied()
        .unwrap_or("")
        .to_owned();
    let audio_bitrate_kbps = AUDIO_BITRATES
        .get(window.get_audio_bitrate_index().max(0) as usize)
        .copied()
        .unwrap_or(192);
    Ok(ExportRequest {
        mode: if window.get_format_index() == 1 {
            ExportMode::ImageSequence
        } else {
            ExportMode::Video
        },
        output_path: PathBuf::from(output_path.trim()),
        image_format: if window.get_image_format_index() == 1 {
            ExportImageFormat::Jpeg
        } else {
            ExportImageFormat::Png
        },
        video_codec,
        crf: window
            .get_crf_mode()
            .then_some(window.get_crf().round() as i32),
        bitrate_mbps: window.get_bitrate_mbps(),
        preset,
        profile,
        audio_codec,
        audio_bitrate_kbps,
        full_range: window.get_full_range(),
        start_frame: window.get_start_frame(),
        end_frame: window.get_end_frame(),
    })
}

fn export_runtime_settings(settings: &SettingsStore) -> ExportRuntimeSettings {
    ExportRuntimeSettings {
        image_quality: settings.i32_value("exportImageQuality", 95).clamp(1, 100) as u8,
        sequence_padding: settings.i32_value("exportSequencePadding", 6),
        progress_interval: settings.i32_value("exportProgressInterval", 5).max(1),
        max_plugin_block_size: usize::try_from(
            settings
                .i32_value("audioPluginMaxBlockSize", 1_024)
                .clamp(1, 8_192),
        )
        .unwrap_or(1_024),
    }
}

fn update_export(
    export: &slint::Weak<ExportWindow>,
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
    manager: &Rc<RefCell<ExportManager>>,
    planner: &Rc<RefCell<ExportPlannerRuntime>>,
) {
    let Some(window) = export.upgrade() else {
        return;
    };
    for event in manager.borrow_mut().poll() {
        match event {
            ExportEvent::Progress(progress) => {
                window.set_export_progress(progress.progress as f32);
                window.set_progress_label(export_progress_label(progress).into());
            }
            ExportEvent::Finished(result) => {
                planner.borrow_mut().reset();
                window.set_exporting(false);
                window.set_cancel_confirmation_visible(false);
                show_export_result(&window, result.success, &result.message);
            }
        }
    }

    let request = manager.borrow_mut().next_frame_request();
    let Some(request) = request else {
        return;
    };
    let planned = {
        let mut application = model.borrow_mut();
        match export_workspace_for_frame(&mut application, request.project_instance_id) {
            Ok(workspace) => {
                workspace.pause_playback();
                workspace.seek(request.frame);
                let project_path = workspace.project().path.clone();
                planner
                    .borrow_mut()
                    .build(request.project_instance_id, workspace, request.frame)
                    .map(|planned| (planned, project_path))
                    .ok_or_else(|| {
                        format!("Frame render error: failed to plan frame {}", request.frame)
                    })
            }
            Err(message) => Err(message),
        }
    };
    sync_transport_weak(main, timeline, model);
    match planned {
        Ok((planned, project_path)) => {
            for warning in planned.warnings {
                eprintln!("Export planning warning: {warning}");
            }
            manager.borrow_mut().submit_scene(
                planned.scene,
                planned.audio,
                project_path.as_deref(),
            );
        }
        Err(message) => manager.borrow_mut().fail_current_frame(message),
    }
}

fn filtered_codec_catalog(
    available: &[String],
    catalog: &[(&str, &str)],
) -> (Vec<SharedString>, Vec<String>) {
    catalog
        .iter()
        .filter(|(_, value)| available.is_empty() || available.iter().any(|item| item == value))
        .map(|(label, value)| (SharedString::from(*label), (*value).to_owned()))
        .unzip()
}

fn selected_codec_index(values: &[String], selected: &str) -> i32 {
    if values.is_empty() {
        return -1;
    }
    values
        .iter()
        .position(|value| value == selected)
        .unwrap_or(0) as i32
}

fn selected_codec<'a>(values: &'a [String], index: i32, fallback: &'a str) -> &'a str {
    usize::try_from(index)
        .ok()
        .and_then(|index| values.get(index))
        .map(String::as_str)
        .unwrap_or(fallback)
}

fn setting_string(settings: &SettingsStore, key: &str, fallback: &str) -> String {
    settings
        .value(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

fn export_progress_label(progress: aviqtl_export::ExportProgressPlan) -> String {
    let language = CURRENT_UI_LANGUAGE.with(|language| language.get());
    let eta = match (language, progress.eta_seconds) {
        (_, seconds) if seconds <= 0 => String::new(),
        (UiLanguage::English, seconds) if seconds >= 3_600 => format!(
            " ({}h {}m remaining)",
            seconds / 3_600,
            seconds % 3_600 / 60
        ),
        (UiLanguage::SimplifiedChinese, seconds) if seconds >= 3_600 => format!(
            "（剩余 {} 小时 {} 分钟）",
            seconds / 3_600,
            seconds % 3_600 / 60
        ),
        (UiLanguage::Japanese, seconds) if seconds >= 3_600 => {
            format!(" (残り {}時間{}分)", seconds / 3_600, seconds % 3_600 / 60)
        }
        (UiLanguage::English, seconds) if seconds >= 60 => {
            format!(" ({}m {}s remaining)", seconds / 60, seconds % 60)
        }
        (UiLanguage::SimplifiedChinese, seconds) if seconds >= 60 => {
            format!("（剩余 {} 分 {} 秒）", seconds / 60, seconds % 60)
        }
        (UiLanguage::Japanese, seconds) if seconds >= 60 => {
            format!(" (残り {}分{}秒)", seconds / 60, seconds % 60)
        }
        (UiLanguage::English, seconds) => format!(" ({}s remaining)", seconds),
        (UiLanguage::SimplifiedChinese, seconds) => format!("（剩余 {} 秒）", seconds),
        (UiLanguage::Japanese, seconds) => format!(" (残り {}秒)", seconds),
    };
    format!(
        "{} / {} {}{}",
        progress.current_frame,
        progress.total_frames,
        localized("frames", "帧", "フレーム"),
        eta
    )
}

fn show_export_result(window: &ExportWindow, success: bool, message: &str) {
    window.set_result_success(success);
    window.set_result_message(message.into());
    window.set_result_visible(true);
}

fn choose_export_output(choose_folder: bool, current_path: &str) -> Option<PathBuf> {
    let current_path = PathBuf::from(current_path.trim());
    if choose_folder {
        let mut dialog = rfd::FileDialog::new().set_title(localized(
            "Choose the destination folder",
            "选择目标文件夹",
            "保存先フォルダを指定",
        ));
        if current_path.is_dir() {
            dialog = dialog.set_directory(current_path);
        }
        dialog.pick_folder()
    } else {
        let mut dialog = rfd::FileDialog::new()
            .set_title(localized(
                "Choose the export destination",
                "选择导出位置",
                "保存先を指定",
            ))
            .add_filter("MP4 Video", &["mp4"])
            .add_filter("MKV Video", &["mkv"])
            .add_filter("All Files", &["*"]);
        if let Some(parent) = current_path.parent().filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(parent);
        }
        if let Some(name) = current_path.file_name().filter(|name| !name.is_empty()) {
            dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
        }
        dialog.save_file()
    }
}

fn pick_parameter_file(current_path: &str, filter: &str, label: &str) -> Option<String> {
    let mut dialog = rfd::FileDialog::new().set_title(if label.is_empty() {
        localized("Choose a file", "选择文件", "ファイルを選択")
    } else {
        label
    });
    for (name, extensions) in qt_file_filters(filter) {
        dialog = dialog.add_filter(name, &extensions);
    }
    dialog = dialog.add_filter("All Files", &["*"]);

    let current_path = PathBuf::from(current_path.trim());
    if let Some(parent) = current_path.parent().filter(|path| path.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    if let Some(name) = current_path.file_name().filter(|name| !name.is_empty()) {
        dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
    }
    dialog
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

fn qt_file_filters(value: &str) -> Vec<(String, Vec<String>)> {
    value
        .split(";;")
        .filter_map(|filter| {
            let filter = filter.trim();
            let (name, patterns) = filter.split_once('(')?;
            let patterns = patterns.strip_suffix(')')?;
            let extensions = patterns
                .split_whitespace()
                .filter_map(|pattern| {
                    let extension = pattern
                        .trim()
                        .strip_prefix("*.")
                        .or_else(|| (pattern.trim() == "*").then_some("*"))?;
                    (!extension.is_empty()).then(|| extension.to_owned())
                })
                .collect::<Vec<_>>();
            (!extensions.is_empty()).then(|| (name.trim().to_owned(), extensions))
        })
        .collect()
}

fn parse_qt_color(value: &str) -> [u8; 4] {
    let hex = value.trim().strip_prefix('#').unwrap_or_default();
    let nibble = |value: u8| value.saturating_mul(17);
    match hex.len() {
        3 => {
            let bytes = hex.as_bytes();
            let Some(red) = hex_nibble(bytes[0]) else {
                return [255; 4];
            };
            let Some(green) = hex_nibble(bytes[1]) else {
                return [255; 4];
            };
            let Some(blue) = hex_nibble(bytes[2]) else {
                return [255; 4];
            };
            [255, nibble(red), nibble(green), nibble(blue)]
        }
        4 => {
            let bytes = hex.as_bytes();
            let Some(alpha) = hex_nibble(bytes[0]) else {
                return [255; 4];
            };
            let Some(red) = hex_nibble(bytes[1]) else {
                return [255; 4];
            };
            let Some(green) = hex_nibble(bytes[2]) else {
                return [255; 4];
            };
            let Some(blue) = hex_nibble(bytes[3]) else {
                return [255; 4];
            };
            [nibble(alpha), nibble(red), nibble(green), nibble(blue)]
        }
        6 => parse_hex_bytes(hex).map_or([255; 4], |bytes| [255, bytes[0], bytes[1], bytes[2]]),
        8 => {
            parse_hex_bytes(hex).map_or([255; 4], |bytes| [bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        _ => [255; 4],
    }
}

fn hex_nibble(value: u8) -> Option<u8> {
    (value as char).to_digit(16).map(|value| value as u8)
}

fn parse_hex_bytes(value: &str) -> Option<Vec<u8>> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|digits| {
            let digits = std::str::from_utf8(digits).ok()?;
            u8::from_str_radix(digits, 16).ok()
        })
        .collect()
}

fn format_qt_color(red: i32, green: i32, blue: i32, alpha: i32) -> String {
    let [red, green, blue, alpha] = [red, green, blue, alpha].map(|value| value.clamp(0, 255));
    if alpha == 255 {
        format!("#{red:02x}{green:02x}{blue:02x}")
    } else {
        format!("#{alpha:02x}{red:02x}{green:02x}{blue:02x}")
    }
}

fn slint_color(value: &str) -> Color {
    let [alpha, red, green, blue] = parse_qt_color(value);
    Color::from_argb_u8(alpha, red, green, blue)
}

fn system_font_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    database
        .faces()
        .flat_map(|face| face.families.iter().map(|(family, _)| family.trim()))
        .filter(|family| !family.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn filtered_font_families(families: &[String], query: &str) -> Vec<SharedString> {
    let query = query.trim().to_lowercase();
    families
        .iter()
        .filter(|family| query.is_empty() || family.to_lowercase().contains(&query))
        .map(|family| SharedString::from(family.clone()))
        .collect()
}

fn sync_font_families(window: &ObjectSettingsWindow, families: &[String], query: &str) {
    update_vec_model(
        &window.get_font_families(),
        filtered_font_families(families, query),
    );
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

fn json_i32(value: Option<&serde_json::Value>, fallback: i32) -> i32 {
    value
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| {
            value
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| value.round() as i32)
        })
        .unwrap_or(fallback)
}

fn restore_window_geometry(
    window: &slint::Window,
    settings: &SettingsStore,
    id: &str,
    fallback: WindowGeometry,
) {
    let geometry = WindowGeometry::load(settings, id, fallback);
    window.set_size(slint::LogicalSize::new(
        geometry.width as f32,
        geometry.height as f32,
    ));
    window.set_position(slint::LogicalPosition::new(
        geometry.x as f32,
        geometry.y as f32,
    ));
    window.set_maximized(geometry.maximized);
}

fn persist_window_geometry(
    settings: &Rc<RefCell<SettingsStore>>,
    id: &str,
    window: &slint::Window,
) {
    let mut replacement = settings.borrow().snapshot();
    replacement.insert(
        format!("windowGeometry_{id}"),
        WindowGeometry::capture(window).json(),
    );
    if let Err(error) = settings.borrow_mut().apply(replacement) {
        eprintln!("Failed to save {id} window geometry: {error}");
    }
}

fn insert_visible_window_geometry<T: ComponentHandle + 'static>(
    replacement: &mut serde_json::Map<String, serde_json::Value>,
    id: &str,
    window: &slint::Weak<T>,
) -> bool {
    let Some(window) = window.upgrade() else {
        return false;
    };
    if !window.window().is_visible() {
        return false;
    }
    replacement.insert(
        format!("windowGeometry_{id}"),
        WindowGeometry::capture(window.window()).json(),
    );
    true
}

fn install_window_geometry_close_handler<T: ComponentHandle + 'static>(
    window: slint::Weak<T>,
    settings: Rc<RefCell<SettingsStore>>,
    id: &'static str,
) {
    let Some(component) = window.upgrade() else {
        return;
    };
    component.window().on_close_requested(move || {
        if let Some(component) = window.upgrade() {
            persist_window_geometry(&settings, id, component.window());
        }
        CloseRequestResponse::HideWindow
    });
}

fn choose_missing_media_replacement(clip_type: &str, suggested_path: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(localized(
        "Replace missing media",
        "替换缺失媒体",
        "不足しているメディアを置換",
    ));
    dialog = match clip_type {
        "audio" => dialog.add_filter("Audio files", &["wav", "mp3", "aac", "m4a", "flac", "ogg"]),
        "image" => dialog.add_filter(
            "Image files",
            &["png", "jpg", "jpeg", "bmp", "gif", "webp", "svg"],
        ),
        "video" => dialog.add_filter("Video files", &["mp4", "mov", "avi", "mkv", "webm", "wmv"]),
        _ => return None,
    };
    if let Some(parent) = suggested_path.parent().filter(|parent| parent.exists()) {
        dialog = dialog.set_directory(parent);
    }
    if let Some(name) = suggested_path.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
    }
    dialog.pick_file()
}

fn show_and_redraw<T: ComponentHandle + 'static>(window: &T) -> Result<(), slint::PlatformError> {
    window.show()?;
    let window = window.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        if let Some(window) = window.upgrade() {
            window.window().request_redraw();
            let _ = window
                .window()
                .with_winit_window(|winit_window| winit_window.focus_window());
        }
    });
    Ok(())
}

fn show_centered_and_redraw<T, P>(window: &T, parent: &P) -> Result<(), slint::PlatformError>
where
    T: ComponentHandle + 'static,
    P: ComponentHandle + 'static,
{
    window.show()?;
    let window = window.as_weak();
    let parent = parent.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        let (Some(window), Some(parent)) = (window.upgrade(), parent.upgrade()) else {
            return;
        };
        let parent_position = parent.window().position();
        let parent_size = parent.window().size();
        let window_size = window.window().size();
        let centered_coordinate = |origin: i32, parent: u32, child: u32| {
            (i64::from(origin) + (i64::from(parent) - i64::from(child)) / 2)
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        };
        window.window().set_position(slint::PhysicalPosition::new(
            centered_coordinate(parent_position.x, parent_size.width, window_size.width),
            centered_coordinate(parent_position.y, parent_size.height, window_size.height),
        ));
        window.window().request_redraw();
    });
    Ok(())
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
    window
        .set_recent_project_max_count(settings.i32_value("recentProjectMaxCount", 10).clamp(1, 50));
    window.set_undo_count(settings.i32_value("undoCount", 32).clamp(1, 1_000));
    window.set_splash_size(settings.i32_value("splashSize", 512).clamp(128, 2_048));
    window.set_max_image_size(
        settings
            .i32_value("maxImageSize", 8_192)
            .clamp(1_024, 16_384),
    );
    window.set_cache_size(settings.i32_value("cacheSize", 512).clamp(64, 8_192));
    window.set_preview_render_scale_index(choice_index_f64(
        settings.f64_value("previewRenderScale", 1.0),
        &SYSTEM_PREVIEW_RENDER_SCALES,
        0,
    ));
    window.set_preview_msaa_index(choice_index_i32(
        settings.i32_value("previewMsaaSamples", 0),
        &SYSTEM_PREVIEW_MSAA_SAMPLES,
        0,
    ));
    window.set_bake_strategy_index(choice_index_str(
        &setting_string(settings, "bakeStrategy", "OnDemand"),
        &SYSTEM_BAKE_STRATEGIES,
        0,
    ));
    window.set_on_demand_prefetch_frames(
        settings
            .i32_value("onDemandPrefetchFrames", 30)
            .clamp(0, 600),
    );
    window.set_enable_timeline_skimming(settings.bool_value("enableTimelineSkimming", true));
    window.set_timeline_track_height(settings.i32_value("timelineTrackHeight", 30).clamp(16, 100));
    window.set_timeline_header_height(
        settings
            .i32_value("timelineHeaderHeight", 28)
            .clamp(16, 100),
    );
    window
        .set_setting_dialog_sidebar_right(settings.bool_value("settingDialogSidebarRight", false));
    window.set_timeline_ruler_height(settings.i32_value("timelineRulerHeight", 32).clamp(16, 100));
    window.set_timeline_max_layers(settings.i32_value("timelineMaxLayers", 128).clamp(1, 512));
    window.set_timeline_layer_header_width(
        settings
            .i32_value("timelineLayerHeaderWidth", 60)
            .clamp(40, 300),
    );
    window.set_timeline_clip_resize_handle_width(
        settings
            .i32_value("timelineClipResizeHandleWidth", 10)
            .clamp(4, 40),
    );
    window
        .set_min_clip_duration_frames(settings.i32_value("minClipDurationFrames", 5).clamp(1, 100));
    let (zoom_minimum, zoom_maximum, zoom_step) = timeline_zoom_settings(settings);
    window.set_timeline_zoom_min(zoom_minimum.round() as i32);
    window.set_timeline_zoom_max(zoom_maximum.round() as i32);
    window.set_timeline_zoom_step(zoom_step.round() as i32);
    window.set_theme_index(choice_index_str(
        &setting_string(settings, "theme", "Dark"),
        &SYSTEM_THEME_VALUES,
        0,
    ));
    window.set_language_index(choice_index_str(
        &setting_string(settings, "uiLanguage", "System"),
        &SYSTEM_UI_LANGUAGE_VALUES,
        0,
    ));
    window.set_default_project_width(defaults.width);
    window.set_default_project_height(defaults.height);
    window.set_default_project_fps(SharedString::from(defaults.fps.to_string()));
    window.set_default_project_frames(defaults.duration);
    window.set_default_project_sample_rate(defaults.sample_rate);
    window.set_default_clip_duration(
        settings
            .i32_value("defaultClipDuration", 100)
            .clamp(1, 100_000),
    );
    window.set_export_video_codec_index(choice_index_str(
        &setting_string(settings, "exportDefaultCodec", "libx264"),
        &SYSTEM_EXPORT_VIDEO_CODECS,
        2,
    ));
    window.set_export_default_bitrate_mbps(
        settings
            .i32_value("exportDefaultBitrateMbps", 15)
            .clamp(1, 500),
    );
    window.set_export_default_crf(settings.i32_value("exportDefaultCrf", 20).clamp(0, 51));
    window.set_export_image_quality(settings.i32_value("exportImageQuality", 95).clamp(0, 100));
    window.set_export_sequence_padding(settings.i32_value("exportSequencePadding", 6).clamp(2, 10));
    let audio_codec = setting_string(settings, "exportDefaultAudioCodec", "aac");
    window.set_export_audio_codec_index(if audio_codec == "libopus" {
        1
    } else {
        choice_index_str(&audio_codec, &SYSTEM_EXPORT_AUDIO_CODECS, 0)
    });
    window.set_export_default_audio_bitrate_kbps(
        settings
            .i32_value("exportDefaultAudioBitrateKbps", 192)
            .clamp(32, 1_536),
    );
    window.set_export_frame_grab_timeout_ms(
        settings
            .i32_value("exportFrameGrabTimeoutMs", 2_000)
            .clamp(100, 10_000),
    );
    window
        .set_export_progress_interval(settings.i32_value("exportProgressInterval", 5).clamp(1, 60));
    window.set_export_encoder_queue_mb(
        settings
            .i32_value("exportEncoderQueueMB", 128)
            .clamp(16, 1_024),
    );
    window.set_video_decoder_index_reserve(
        settings
            .i32_value("videoDecoderIndexReserve", 108_000)
            .clamp(1_000, 1_000_000),
    );
    window.set_video_decoder_min_cache_mb(
        settings
            .i32_value("videoDecoderMinCacheMB", 64)
            .clamp(16, 4_096),
    );
    window.set_hw_frame_pool_size(settings.i32_value("hwFramePoolSize", 32).clamp(1, 256));
    window.set_audio_plugin_block_size_index(choice_index_i32(
        settings.i32_value("audioPluginMaxBlockSize", 4_096),
        &SYSTEM_AUDIO_BLOCK_SIZES,
        4,
    ));
    window.set_lua_hook_interval_ms(settings.i32_value("luaHookIntervalMs", 16).clamp(1, 1_000));
    window.set_lua_hot_reload(settings.bool_value("luaHotReload", false));

    update_vec_model(
        &window.get_plugin_settings(),
        SYSTEM_PLUGIN_FORMATS
            .iter()
            .map(|format| SystemPluginSettingData {
                format_name: SharedString::from(*format),
                enabled: settings.bool_value(&format!("pluginEnable{format}"), true),
                paths: SharedString::from(plugin_paths_text(settings, format)),
            })
            .collect(),
    );
    update_vec_model(
        &window.get_shortcut_settings(),
        SYSTEM_SHORTCUT_ROWS
            .iter()
            .map(|(action_id, fallback)| SystemShortcutSettingData {
                action_id: SharedString::from(*action_id),
                value: SharedString::from(shortcut_setting(settings, action_id, fallback)),
            })
            .collect(),
    );
}

fn choice_index_str(value: &str, choices: &[&str], fallback: usize) -> i32 {
    choices
        .iter()
        .position(|choice| *choice == value)
        .unwrap_or(fallback) as i32
}

fn choice_index_i32(value: i32, choices: &[i32], fallback: usize) -> i32 {
    choices
        .iter()
        .position(|choice| *choice == value)
        .unwrap_or(fallback) as i32
}

fn choice_index_f64(value: f64, choices: &[f64], fallback: usize) -> i32 {
    choices
        .iter()
        .position(|choice| (*choice - value).abs() <= f64::EPSILON)
        .unwrap_or(fallback) as i32
}

fn plugin_paths_text(settings: &SettingsStore, format: &str) -> String {
    settings
        .value(&format!("pluginPaths{format}"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

fn sync_package_manager(window: &PackageManagerWindow, model: &PackageManagerModel) {
    let section = match window.get_tab_index() {
        0 => Some(PackageSection::Effect),
        1 => Some(PackageSection::Object),
        2 => Some(PackageSection::Mod),
        3 => Some(PackageSection::Installed),
        4 => Some(PackageSection::Application),
        _ => None,
    };
    let packages = section
        .map(|section| model.packages(section, window.get_search_query().as_str()))
        .unwrap_or_default()
        .into_iter()
        .map(|package| PackageData {
            can_manage_permissions: package.can_manage_permissions(),
            can_remove: package.can_remove(),
            id: SharedString::from(package.id),
            package_type: SharedString::from(package.package_type),
            display_name: SharedString::from(package.display_name),
            description: SharedString::from(package.description),
            author: SharedString::from(package.author),
            version: SharedString::from(package.version),
            installed_version: SharedString::from(package.installed_version),
            latest_version: SharedString::from(package.latest_version),
            source_repository: SharedString::from(package.source_repository),
            local_file_plugin: package.local_file_plugin,
            has_update: package.has_update,
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_packages(), packages);
    update_vec_model(
        &window.get_repositories(),
        model
            .repositories()
            .into_iter()
            .map(|repository| PackageRepositoryData {
                name: SharedString::from(repository.name),
                url: SharedString::from(repository.url),
                enabled: repository.enabled,
                priority: repository.priority,
            })
            .collect(),
    );
    window.set_has_updates(model.has_updates());
    window.set_status_text(SharedString::from(model.status()));
}

fn sync_plugin_permissions(window: &PluginPermissionWindow, settings: &SettingsStore) {
    let rows = plugin_permission_grants(settings, window.get_plugin_id().as_str())
        .into_iter()
        .map(|permission| {
            let (title, description) = plugin_permission_metadata(&permission.name);
            PluginPermissionData {
                name: SharedString::from(permission.name),
                title: SharedString::from(title),
                description: SharedString::from(description),
                granted: permission.granted,
            }
        })
        .collect();
    update_vec_model(&window.get_permissions(), rows);
}

fn plugin_permission_metadata(name: &str) -> (&'static str, &'static str) {
    match name {
        "transport.control" => (
            localized("Playback control", "播放控制", "再生制御"),
            localized(
                "Play, pause, and seek",
                "播放、暂停和定位",
                "再生、一時停止、シーク",
            ),
        ),
        "clip.read" => (
            localized("Read clips", "读取剪辑", "クリップ読み取り"),
            localized(
                "List clip information",
                "列出剪辑信息",
                "クリップ情報の一覧表示",
            ),
        ),
        "clip.modify" => (
            localized("Modify clips", "修改剪辑", "クリップ変更"),
            localized(
                "Create, delete, and move clips",
                "创建、删除和移动剪辑",
                "クリップの作成、削除、移動",
            ),
        ),
        "effect.modify" => (
            localized("Modify effects", "修改特效", "エフェクト変更"),
            localized(
                "Add, delete, and change effects",
                "添加、删除和修改特效",
                "エフェクトの追加、削除、変更",
            ),
        ),
        "project.read" => (
            localized("Read project", "读取项目", "プロジェクト読み取り"),
            localized(
                "Read resolution, FPS, and other project information",
                "读取分辨率、FPS 等项目信息",
                "解像度、FPS等の情報取得",
            ),
        ),
        "project.save" => (
            localized("Save project", "保存项目", "プロジェクト保存"),
            localized(
                "Save project files",
                "保存项目文件",
                "プロジェクトファイルの保存",
            ),
        ),
        "project.load" => (
            localized("Load project", "加载项目", "プロジェクト読み込み"),
            localized(
                "Load project files",
                "加载项目文件",
                "プロジェクトファイルの読み込み",
            ),
        ),
        "scene.manage" => (
            localized("Manage scenes", "管理场景", "シーン管理"),
            localized(
                "Create, delete, and switch scenes",
                "创建、删除和切换场景",
                "シーンの作成、削除、切り替え",
            ),
        ),
        "settings.read" => (
            localized("Read settings", "读取设置", "設定読み取り"),
            localized(
                "Read plugin settings",
                "读取插件设置",
                "プラグイン設定の読み取り",
            ),
        ),
        "settings.write" => (
            localized("Write settings", "写入设置", "設定書き込み"),
            localized(
                "Save plugin settings",
                "保存插件设置",
                "プラグイン設定の保存",
            ),
        ),
        "clipboard.access" => (
            localized("Clipboard access", "剪贴板访问", "クリップボード"),
            localized(
                "Copy, cut, and paste",
                "复制、剪切和粘贴",
                "コピー、切り取り、貼り付け",
            ),
        ),
        "history.control" => (
            localized("History control", "历史记录控制", "履歴操作"),
            localized(
                "Undo, redo, and group commands",
                "撤销、重做和命令分组",
                "元に戻す、やり直し、コマンドのグループ化",
            ),
        ),
        "log.output" => (
            localized("Log output", "日志输出", "ログ出力"),
            localized(
                "Write messages to the console",
                "向控制台输出消息",
                "コンソールへのログ出力",
            ),
        ),
        _ => ("", ""),
    }
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
    let replacement = system_settings_replacement(window, settings.borrow().snapshot())?;
    settings.borrow_mut().apply(replacement)?;
    apply_runtime_settings(&mut model.borrow_mut(), &settings.borrow());
    Ok(())
}

fn system_settings_replacement(
    window: &SystemSettingsWindow,
    mut replacement: serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let fps = parse_required_f64(
        &window.get_default_project_fps(),
        localized("Default frame rate", "默认帧率", "既定のフレームレート"),
        1.0,
        240.0,
    )?;
    let zoom_minimum = window.get_timeline_zoom_min().clamp(1, 400);
    let zoom_maximum = window.get_timeline_zoom_max().clamp(zoom_minimum, 1_000);
    for (key, value) in [
        (
            "showConfirmOnClose",
            serde_json::json!(window.get_confirm_unsaved()),
        ),
        (
            "enableAutoBackup",
            serde_json::json!(window.get_auto_backup()),
        ),
        (
            "backupInterval",
            serde_json::json!(window.get_backup_interval().clamp(1, 60)),
        ),
        (
            "recentProjectMaxCount",
            serde_json::json!(window.get_recent_project_max_count().clamp(1, 50)),
        ),
        (
            "undoCount",
            serde_json::json!(window.get_undo_count().clamp(1, 1_000)),
        ),
        (
            "splashSize",
            serde_json::json!(window.get_splash_size().clamp(128, 2_048)),
        ),
        (
            "maxImageSize",
            serde_json::json!(window.get_max_image_size().clamp(1_024, 16_384)),
        ),
        (
            "cacheSize",
            serde_json::json!(window.get_cache_size().clamp(64, 8_192)),
        ),
        (
            "previewRenderScale",
            serde_json::json!(choice_f64(
                &SYSTEM_PREVIEW_RENDER_SCALES,
                window.get_preview_render_scale_index(),
                0,
            )),
        ),
        (
            "previewMsaaSamples",
            serde_json::json!(choice_i32(
                &SYSTEM_PREVIEW_MSAA_SAMPLES,
                window.get_preview_msaa_index(),
                0,
            )),
        ),
        (
            "bakeStrategy",
            serde_json::json!(choice_str(
                &SYSTEM_BAKE_STRATEGIES,
                window.get_bake_strategy_index(),
                0,
            )),
        ),
        (
            "onDemandPrefetchFrames",
            serde_json::json!(window.get_on_demand_prefetch_frames().clamp(0, 600)),
        ),
        (
            "enableTimelineSkimming",
            serde_json::json!(window.get_enable_timeline_skimming()),
        ),
        (
            "timelineTrackHeight",
            serde_json::json!(window.get_timeline_track_height().clamp(16, 100)),
        ),
        (
            "timelineHeaderHeight",
            serde_json::json!(window.get_timeline_header_height().clamp(16, 100)),
        ),
        (
            "settingDialogSidebarRight",
            serde_json::json!(window.get_setting_dialog_sidebar_right()),
        ),
        (
            "timelineRulerHeight",
            serde_json::json!(window.get_timeline_ruler_height().clamp(16, 100)),
        ),
        (
            "timelineMaxLayers",
            serde_json::json!(window.get_timeline_max_layers().clamp(1, 512)),
        ),
        (
            "timelineLayerHeaderWidth",
            serde_json::json!(window.get_timeline_layer_header_width().clamp(40, 300)),
        ),
        (
            "timelineClipResizeHandleWidth",
            serde_json::json!(window.get_timeline_clip_resize_handle_width().clamp(4, 40)),
        ),
        (
            "minClipDurationFrames",
            serde_json::json!(window.get_min_clip_duration_frames().clamp(1, 100)),
        ),
        ("timelineZoomMin", serde_json::json!(zoom_minimum)),
        ("timelineZoomMax", serde_json::json!(zoom_maximum)),
        (
            "timelineZoomStep",
            serde_json::json!(window.get_timeline_zoom_step().clamp(1, 100)),
        ),
        (
            "theme",
            serde_json::json!(choice_str(
                &SYSTEM_THEME_VALUES,
                window.get_theme_index(),
                0,
            )),
        ),
        (
            "uiLanguage",
            serde_json::json!(choice_str(
                &SYSTEM_UI_LANGUAGE_VALUES,
                window.get_language_index(),
                0,
            )),
        ),
        (
            "defaultProjectWidth",
            serde_json::json!(window.get_default_project_width().clamp(1, 16_000)),
        ),
        (
            "defaultProjectHeight",
            serde_json::json!(window.get_default_project_height().clamp(1, 16_000)),
        ),
        ("defaultProjectFps", serde_json::json!(fps)),
        (
            "defaultProjectFrames",
            serde_json::json!(window.get_default_project_frames().clamp(1, 1_000_000)),
        ),
        (
            "defaultProjectSampleRate",
            serde_json::json!(
                window
                    .get_default_project_sample_rate()
                    .clamp(8_000, 192_000)
            ),
        ),
        (
            "defaultClipDuration",
            serde_json::json!(window.get_default_clip_duration().clamp(1, 100_000)),
        ),
        (
            "exportDefaultCodec",
            serde_json::json!(choice_str(
                &SYSTEM_EXPORT_VIDEO_CODECS,
                window.get_export_video_codec_index(),
                2,
            )),
        ),
        (
            "exportDefaultBitrateMbps",
            serde_json::json!(window.get_export_default_bitrate_mbps().clamp(1, 500)),
        ),
        (
            "exportDefaultCrf",
            serde_json::json!(window.get_export_default_crf().clamp(0, 51)),
        ),
        (
            "exportImageQuality",
            serde_json::json!(window.get_export_image_quality().clamp(0, 100)),
        ),
        (
            "exportSequencePadding",
            serde_json::json!(window.get_export_sequence_padding().clamp(2, 10)),
        ),
        (
            "exportDefaultAudioCodec",
            serde_json::json!(choice_str(
                &SYSTEM_EXPORT_AUDIO_CODECS,
                window.get_export_audio_codec_index(),
                0,
            )),
        ),
        (
            "exportDefaultAudioBitrateKbps",
            serde_json::json!(
                window
                    .get_export_default_audio_bitrate_kbps()
                    .clamp(32, 1_536)
            ),
        ),
        (
            "exportFrameGrabTimeoutMs",
            serde_json::json!(window.get_export_frame_grab_timeout_ms().clamp(100, 10_000)),
        ),
        (
            "exportProgressInterval",
            serde_json::json!(window.get_export_progress_interval().clamp(1, 60)),
        ),
        (
            "exportEncoderQueueMB",
            serde_json::json!(window.get_export_encoder_queue_mb().clamp(16, 1_024)),
        ),
        (
            "videoDecoderIndexReserve",
            serde_json::json!(
                window
                    .get_video_decoder_index_reserve()
                    .clamp(1_000, 1_000_000)
            ),
        ),
        (
            "videoDecoderMinCacheMB",
            serde_json::json!(window.get_video_decoder_min_cache_mb().clamp(16, 4_096)),
        ),
        (
            "hwFramePoolSize",
            serde_json::json!(window.get_hw_frame_pool_size().clamp(1, 256)),
        ),
        (
            "audioPluginMaxBlockSize",
            serde_json::json!(choice_i32(
                &SYSTEM_AUDIO_BLOCK_SIZES,
                window.get_audio_plugin_block_size_index(),
                4,
            )),
        ),
        (
            "luaHookIntervalMs",
            serde_json::json!(window.get_lua_hook_interval_ms().clamp(1, 1_000)),
        ),
        (
            "luaHotReload",
            serde_json::json!(window.get_lua_hot_reload()),
        ),
    ] {
        replacement.insert(key.to_owned(), value);
    }

    let plugin_settings = window.get_plugin_settings();
    for index in 0..plugin_settings.row_count() {
        let Some(row) = plugin_settings.row_data(index) else {
            continue;
        };
        replacement.insert(
            format!("pluginEnable{}", row.format_name),
            serde_json::json!(row.enabled),
        );
        replacement.insert(
            format!("pluginPaths{}", row.format_name),
            plugin_paths_value(&row.paths),
        );
    }

    let mut shortcuts = replacement
        .get("shortcuts")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let shortcut_settings = window.get_shortcut_settings();
    for index in 0..shortcut_settings.row_count() {
        let Some(row) = shortcut_settings.row_data(index) else {
            continue;
        };
        shortcuts.insert(
            row.action_id.to_string(),
            serde_json::Value::String(row.value.to_string()),
        );
    }
    replacement.insert("shortcuts".to_owned(), serde_json::Value::Object(shortcuts));
    Ok(replacement)
}

fn choice_str<'a>(choices: &'a [&'a str], index: i32, fallback: usize) -> &'a str {
    usize::try_from(index)
        .ok()
        .and_then(|index| choices.get(index).copied())
        .unwrap_or(choices[fallback])
}

fn choice_i32(choices: &[i32], index: i32, fallback: usize) -> i32 {
    usize::try_from(index)
        .ok()
        .and_then(|index| choices.get(index).copied())
        .unwrap_or(choices[fallback])
}

fn choice_f64(choices: &[f64], index: i32, fallback: usize) -> f64 {
    usize::try_from(index)
        .ok()
        .and_then(|index| choices.get(index).copied())
        .unwrap_or(choices[fallback])
}

fn plugin_paths_value(text: &str) -> serde_json::Value {
    serde_json::Value::Array(
        text.lines()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(|path| serde_json::Value::String(path.to_owned()))
            .collect(),
    )
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
        update_vec_model(&main.get_missing_media(), Vec::new());
        main.set_missing_media_dialog_visible(false);
        update_vec_model(&timeline.get_scene_tabs(), Vec::new());
        update_vec_model(&timeline.get_clips(), Vec::new());
        update_vec_model(&timeline.get_layers(), Vec::new());
        return;
    };
    let missing_media = workspace
        .missing_media()
        .into_iter()
        .map(|entry| MissingMediaData {
            clip_id: entry.clip_id,
            scene_id: entry.scene_id,
            layer: entry.layer,
            clip_type: SharedString::from(entry.clip_type),
            path: SharedString::from(entry.path),
            name: SharedString::from(entry.name),
        })
        .collect::<Vec<_>>();
    let has_missing_media = !missing_media.is_empty();
    update_vec_model(&main.get_missing_media(), missing_media);
    if !has_missing_media {
        main.set_missing_media_dialog_visible(false);
    }
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
            audio: clip.audio,
            clip_by_upper_object: clip.clip_by_upper_object,
            selected: clip.selected,
            primary: clip.primary,
        })
        .collect::<Vec<_>>();
    update_vec_model(&timeline.get_clips(), clips);
    let selected_layer = workspace.selected_layer();
    let scene = workspace.selected_scene_document();
    let layers = (0..timeline.get_maximum_layers().clamp(1, 512))
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

fn object_settings_sync_key(model: &ApplicationModel) -> Option<ObjectSettingsSyncKey> {
    let project_instance_id = model.current_project_instance_id()?;
    let workspace = model.current_workspace()?;
    let clip = workspace.selected_clip_document()?;
    Some(ObjectSettingsSyncKey {
        project_instance_id,
        document_revision: workspace.document_revision(),
        clip_id: clip.id,
        playhead: workspace.playhead(),
        effect_selection: (0..if clip.clip_type == "audio" {
            clip.audio_plugins.len()
        } else {
            clip.effects.len()
        })
            .map(|index| workspace.effect_is_selected(index))
            .collect(),
    })
}

fn sync_object_settings(
    window: &ObjectSettingsWindow,
    model: &ApplicationModel,
    catalog: &EffectCatalog,
) {
    let projection = model
        .current_workspace()
        .and_then(|workspace| workspace.object_settings(catalog));
    let Some(projection) = projection else {
        window.set_has_selection(false);
        window.set_clip_title(SharedString::new());
        window.set_audio_plugin_mode(false);
        window.set_selected_effect_count(0);
        window.set_selected_effects_removable(false);
        update_vec_model(&window.get_effects(), Vec::new());
        update_vec_model(&window.get_setting_rows(), Vec::new());
        return;
    };
    window.set_has_selection(true);
    window.set_audio_plugin_mode(projection.audio_plugin_mode);
    window.set_clip_title(SharedString::from(format!(
        "{}  (ID {})",
        projection.clip_label, projection.clip_id
    )));
    let effects = if projection.audio_plugin_mode {
        window.set_selected_effect_count(
            projection
                .audio_plugins
                .iter()
                .filter(|plugin| plugin.selected)
                .count() as i32,
        );
        window.set_selected_effects_removable(
            projection
                .audio_plugins
                .iter()
                .any(|plugin| plugin.selected),
        );
        projection
            .audio_plugins
            .iter()
            .map(|plugin| ObjectEffectData {
                index: plugin.index as i32,
                id: SharedString::from(plugin.id.clone()),
                name: SharedString::from(plugin.name.clone()),
                enabled: plugin.enabled,
                selected: plugin.selected,
                removable: true,
            })
            .collect::<Vec<_>>()
    } else {
        window.set_selected_effect_count(
            projection
                .effects
                .iter()
                .filter(|effect| effect.selected)
                .count() as i32,
        );
        window.set_selected_effects_removable(
            projection
                .effects
                .iter()
                .any(|effect| effect.selected && effect.removable),
        );
        projection
            .effects
            .iter()
            .map(|effect| ObjectEffectData {
                index: effect.index as i32,
                id: SharedString::from(effect.id.clone()),
                name: SharedString::from(localized_effect_metadata(&effect.name).into_owned()),
                enabled: effect.enabled,
                selected: effect.selected,
                removable: effect.removable,
            })
            .collect::<Vec<_>>()
    };
    update_vec_model(&window.get_effects(), effects);
    update_vec_model(
        &window.get_setting_rows(),
        object_settings_rows(&projection),
    );
}

fn sync_effect_catalog(window: &ObjectSettingsWindow, catalog: &EffectCatalog, query: &str) {
    let query = query.trim().to_lowercase();
    let items = catalog
        .query("effect", "", "")
        .into_iter()
        .filter(|metadata| {
            query.is_empty()
                || metadata.id.to_lowercase().contains(&query)
                || metadata.name.to_lowercase().contains(&query)
                || localized_effect_metadata(&metadata.name)
                    .to_lowercase()
                    .contains(&query)
                || metadata.categories.iter().any(|category| {
                    category.to_lowercase().contains(&query)
                        || localized_effect_metadata(category)
                            .to_lowercase()
                            .contains(&query)
                })
        })
        .map(|metadata| EffectCatalogItemData {
            header: false,
            id: SharedString::from(metadata.id.clone()),
            name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
            categories: SharedString::from(localized_effect_categories(&metadata.categories)),
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_effect_catalog_items(), items);
}

fn initialize_timeline_object_catalog(window: &TimelineWindow, catalog: &EffectCatalog) {
    let categories = std::iter::once(SharedString::from(localized(
        "All categories",
        "所有分类",
        "すべてのカテゴリ",
    )))
    .chain(
        catalog
            .categories("object")
            .into_iter()
            .map(|category| SharedString::from(localized_effect_metadata(&category).into_owned())),
    )
    .collect::<Vec<_>>();
    update_vec_model(&window.get_object_catalog_categories(), categories);
    sync_timeline_object_catalog(window, catalog, "", 0);
}

fn sync_timeline_object_catalog(
    window: &TimelineWindow,
    catalog: &EffectCatalog,
    query: &str,
    category_index: i32,
) {
    let categories = catalog.categories("object");
    let category = usize::try_from(category_index)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| categories.get(index))
        .map_or("", String::as_str);
    let query = query.trim().to_lowercase();
    let items = catalog
        .query("object", "", category)
        .into_iter()
        .filter(|metadata| {
            query.is_empty()
                || metadata.id.to_lowercase().contains(&query)
                || metadata.name.to_lowercase().contains(&query)
                || localized_effect_metadata(&metadata.name)
                    .to_lowercase()
                    .contains(&query)
                || metadata.categories.iter().any(|category| {
                    category.to_lowercase().contains(&query)
                        || localized_effect_metadata(category)
                            .to_lowercase()
                            .contains(&query)
                })
        })
        .map(|metadata| EffectCatalogItemData {
            header: false,
            id: SharedString::from(metadata.id.clone()),
            name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
            categories: SharedString::from(localized_effect_categories(&metadata.categories)),
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_object_catalog_items(), items);
}

fn sync_timeline_context_catalog(
    window: &TimelineWindow,
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    query: &str,
    target_kind: i32,
) {
    update_vec_model(
        &window.get_context_catalog_items(),
        timeline_context_catalog_items(effect_catalog, audio_catalog, query, target_kind),
    );
}

fn timeline_context_catalog_items(
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    query: &str,
    target_kind: i32,
) -> Vec<EffectCatalogItemData> {
    if target_kind == 2 {
        audio_catalog
            .entries(query)
            .into_iter()
            .map(|plugin| EffectCatalogItemData {
                header: false,
                id: SharedString::from(plugin.id),
                name: SharedString::from(plugin.name),
                categories: SharedString::from(plugin.category),
            })
            .collect::<Vec<_>>()
    } else {
        let kind = if target_kind == 0 { "object" } else { "effect" };
        let query = query.trim().to_lowercase();
        effect_catalog
            .query(kind, "", "")
            .into_iter()
            .filter(|metadata| {
                query.is_empty()
                    || metadata.id.to_lowercase().contains(&query)
                    || metadata.name.to_lowercase().contains(&query)
                    || localized_effect_metadata(&metadata.name)
                        .to_lowercase()
                        .contains(&query)
                    || metadata.categories.iter().any(|category| {
                        category.to_lowercase().contains(&query)
                            || localized_effect_metadata(category)
                                .to_lowercase()
                                .contains(&query)
                    })
            })
            .map(|metadata| EffectCatalogItemData {
                header: false,
                id: SharedString::from(metadata.id.clone()),
                name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
                categories: SharedString::from(localized_effect_categories(&metadata.categories)),
            })
            .collect::<Vec<_>>()
    }
}

fn sync_audio_plugin_catalog(
    window: &ObjectSettingsWindow,
    catalog: &AudioPluginCatalog,
    query: &str,
) {
    let mut items = Vec::new();
    let mut previous_category = None::<String>;
    for plugin in catalog.entries(query) {
        if query.trim().is_empty() && previous_category.as_deref() != Some(plugin.category.as_str())
        {
            previous_category = Some(plugin.category.clone());
            items.push(EffectCatalogItemData {
                header: true,
                id: SharedString::new(),
                name: SharedString::from(plugin.category.clone()),
                categories: SharedString::new(),
            });
        }
        items.push(EffectCatalogItemData {
            header: false,
            id: SharedString::from(plugin.id),
            name: SharedString::from(plugin.name),
            categories: SharedString::from(plugin.category),
        });
    }
    update_vec_model(&window.get_effect_catalog_items(), items);
}

fn sync_object_catalog(
    window: &ObjectSettingsWindow,
    model: &ApplicationModel,
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    query: &str,
) {
    if model
        .current_workspace()
        .is_some_and(WorkspaceModel::object_settings_uses_audio_plugins)
    {
        sync_audio_plugin_catalog(window, audio_catalog, query);
    } else {
        sync_effect_catalog(window, effect_catalog, query);
    }
}

fn object_settings_rows(settings: &ObjectSettings) -> Vec<ObjectSettingRowData> {
    let mut rows = Vec::new();
    for effect in &settings.effects {
        push_object_settings_rows(
            &mut rows,
            effect.index,
            &effect.name,
            effect.enabled,
            effect.selected,
            effect.removable && !settings.audio_plugin_mode,
            !settings.audio_plugin_mode,
            false,
            &effect.controls,
        );
    }
    for plugin in &settings.audio_plugins {
        let label = if plugin.format.is_empty() {
            plugin.name.clone()
        } else {
            format!("{} ({})", plugin.name, plugin.format)
        };
        push_object_settings_rows(
            &mut rows,
            plugin.index,
            &label,
            plugin.enabled,
            plugin.selected,
            true,
            false,
            true,
            &plugin.controls,
        );
    }
    rows
}

#[allow(clippy::too_many_arguments)]
fn push_object_settings_rows(
    rows: &mut Vec<ObjectSettingRowData>,
    index: usize,
    name: &str,
    enabled: bool,
    selected: bool,
    removable: bool,
    header_toggle_visible: bool,
    audio_plugin: bool,
    controls: &[ObjectControl],
) {
    rows.push(ObjectSettingRowData {
        row_kind: SharedString::from("effect"),
        source_kind: SharedString::new(),
        audio_plugin,
        effect_index: index as i32,
        param_name: SharedString::new(),
        label: SharedString::from(localized_effect_metadata(name).into_owned()),
        effect_enabled: enabled,
        header_toggle_visible,
        selected,
        removable,
        interactive: true,
        checked: false,
        keyframed: false,
        range_mode: false,
        start_frame: 0,
        end_frame: 0,
        current_frame: 0,
        clip_duration: 0,
        interpolation: SharedString::new(),
        number_value: 0.0,
        end_number_value: 0.0,
        minimum: 0.0,
        maximum: 0.0,
        step: 1.0,
        text_value: SharedString::new(),
        end_text_value: SharedString::new(),
        filter: SharedString::new(),
        color_value: Color::from_rgb_u8(255, 255, 255),
        end_color_value: Color::from_rgb_u8(255, 255, 255),
        unit: SharedString::new(),
        keyframe_markers: ModelRc::new(VecModel::<KeyframeMarkerData>::default()),
        option_labels: ModelRc::new(VecModel::<SharedString>::default()),
        selected_option: -1,
    });
    for control in controls {
        let (minimum, maximum) = object_control_range(control);
        let option_labels = control
            .options
            .iter()
            .map(|option| SharedString::from(localized_effect_metadata(&option.label).into_owned()))
            .collect::<Vec<_>>();
        let supports_track = matches!(
            control.kind,
            ObjectControlKind::Number | ObjectControlKind::Integer | ObjectControlKind::Color
        ) && control.param.is_some();
        let start_value = if audio_plugin {
            &control.value
        } else {
            &control.start_value
        };
        let text_value = control.display_value_at(start_value);
        let end_text_value = control.display_value_at(&control.end_value);
        let parameter_row = ObjectSettingRowData {
            row_kind: SharedString::from(control.kind.as_str()),
            source_kind: SharedString::from(control.source_kind.clone()),
            audio_plugin,
            effect_index: index as i32,
            param_name: SharedString::from(control.param.clone().unwrap_or_default()),
            label: SharedString::from(localized_effect_metadata(&control.label).into_owned()),
            effect_enabled: enabled,
            header_toggle_visible,
            selected,
            removable,
            interactive: (audio_plugin || enabled) && !control.disabled && control.param.is_some(),
            checked: control.bool_value_at(start_value),
            keyframed: control.keyframed,
            range_mode: !audio_plugin && supports_track && control.keyframed,
            start_frame: if audio_plugin {
                control.relative_frame
            } else {
                control.interval_start
            },
            end_frame: control.interval_end,
            current_frame: control.relative_frame,
            clip_duration: control.clip_duration,
            interpolation: SharedString::from(control.start_interpolation.clone()),
            number_value: finite_f32(control.number_value_at(start_value), 0.0),
            end_number_value: finite_f32(control.number_value_at(&control.end_value), 0.0),
            minimum,
            maximum,
            step: control
                .step
                .filter(|step| step.is_finite() && *step > 0.0)
                .map_or(if audio_plugin { 0.001 } else { 1.0 }, |step| {
                    finite_f32(step, if audio_plugin { 0.001 } else { 1.0 })
                }),
            text_value: SharedString::from(text_value.clone()),
            end_text_value: SharedString::from(end_text_value.clone()),
            filter: SharedString::from(control.filter.clone()),
            color_value: slint_color(&text_value),
            end_color_value: slint_color(&end_text_value),
            unit: SharedString::from(control.unit.clone()),
            keyframe_markers: ModelRc::new(VecModel::from(keyframe_markers(control, audio_plugin))),
            option_labels: ModelRc::new(VecModel::from(option_labels)),
            selected_option: control
                .selected_option_at(start_value)
                .map_or(-1, |option| option as i32),
        };
        rows.push(parameter_row.clone());
        if supports_track {
            rows.push(ObjectSettingRowData {
                row_kind: SharedString::from("keyframes"),
                label: SharedString::new(),
                ..parameter_row
            });
        }
    }
}

fn keyframe_markers(control: &ObjectControl, audio_plugin: bool) -> Vec<KeyframeMarkerData> {
    let mut points = control
        .keyframes
        .iter()
        .map(|point| (point.frame, false))
        .collect::<Vec<_>>();
    if !audio_plugin
        && control.clip_duration > 0
        && !points
            .iter()
            .any(|(frame, _)| *frame == control.clip_duration)
    {
        points.push((control.clip_duration, true));
    }
    points.sort_unstable_by_key(|(frame, _)| *frame);
    points.dedup_by_key(|(frame, _)| *frame);
    points
        .iter()
        .enumerate()
        .map(|(index, (frame, virtual_end))| {
            let minimum_frame = index
                .checked_sub(1)
                .and_then(|previous| points.get(previous))
                .map_or(0, |(frame, _)| frame.saturating_add(1));
            let maximum_frame = points
                .get(index + 1)
                .map_or(control.clip_duration, |(frame, _)| frame.saturating_sub(1));
            KeyframeMarkerData {
                frame: *frame,
                minimum_frame,
                maximum_frame,
                virtual_end: *virtual_end,
                removable: *frame != 0
                    && !virtual_end
                    && !(audio_plugin && *frame == control.clip_duration),
                draggable: !audio_plugin && *frame != 0 && !virtual_end,
            }
        })
        .collect()
}

fn object_control_range(control: &ObjectControl) -> (f32, f32) {
    let minimum = control.minimum.unwrap_or(-100_000.0);
    let maximum = control.maximum.unwrap_or(100_000.0);
    if minimum.is_finite() && maximum.is_finite() && minimum <= maximum {
        (
            finite_f32(minimum, -100_000.0),
            finite_f32(maximum, 100_000.0),
        )
    } else {
        (-100_000.0, 100_000.0)
    }
}

fn finite_f32(value: f64, fallback: f32) -> f32 {
    if value.is_finite() && value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX) {
        value as f32
    } else {
        fallback
    }
}

fn easing_name_at(index: i32) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|index| keyframe_interpolation_names().get(index).copied())
        .unwrap_or("none")
}

fn easing_label(name: &str) -> String {
    match name {
        "none" => return localized("Instant", "瞬间", "瞬間移動").to_owned(),
        "linear" => return localized("Linear", "线性", "直線").to_owned(),
        "custom" => return localized("Custom", "自定义", "カスタム").to_owned(),
        "random" => return localized("Random", "随机", "ランダム移動").to_owned(),
        "alternate" => return localized("Alternate", "往复", "反復移動").to_owned(),
        _ => {}
    }
    let (direction, family) = [
        (
            "ease_in_out_",
            localized("Ease in/out", "缓入缓出", "加減速"),
        ),
        (
            "ease_out_in_",
            localized("Ease out/in", "缓出缓入", "減加速"),
        ),
        ("ease_in_", localized("Ease in", "缓入", "加速")),
        ("ease_out_", localized("Ease out", "缓出", "減速")),
    ]
    .into_iter()
    .find_map(|(prefix, direction)| name.strip_prefix(prefix).map(|family| (direction, family)))
    .unwrap_or(("", name));
    let family = match family {
        "sine" => localized("Sine", "正弦", "サイン"),
        "quad" => localized("Quadratic", "二次", "2次"),
        "cubic" => localized("Cubic", "三次", "3次"),
        "quart" => localized("Quartic", "四次", "4次"),
        "quint" => localized("Quintic", "五次", "5次"),
        "expo" => localized("Exponential", "指数", "指数"),
        "circ" => localized("Circular", "圆形", "円"),
        "back" => localized("Back", "回退", "戻る"),
        "elastic" => localized("Elastic", "弹性", "弾性"),
        "bounce" => localized("Bounce", "回弹", "跳ね返り"),
        other => other,
    };
    if direction.is_empty() {
        family.to_owned()
    } else {
        format!("{family} {direction}")
    }
}

fn normalized_easing_filter(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn easing_catalog_rows(
    query: &str,
    step_frames: i32,
    amplitude: f32,
    period: f32,
    points: &[f64],
) -> Vec<EasingCatalogRowData> {
    let names = keyframe_interpolation_names();
    let query = normalized_easing_filter(query);
    let mut rows = Vec::with_capacity(names.len() + EASING_CATEGORIES.len());
    for (category, category_names) in EASING_CATEGORIES {
        let matching = category_names
            .iter()
            .filter_map(|name| {
                let index = names.iter().position(|candidate| candidate == name)?;
                (query.is_empty() || normalized_easing_filter(name).contains(&query))
                    .then_some((index, *name))
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        rows.push(EasingCatalogRowData {
            header: true,
            easing_index: -1,
            name: SharedString::from(category),
            label: SharedString::from(easing_category_label(category)),
            preview_path: SharedString::new(),
        });
        rows.extend(matching.into_iter().map(|(index, name)| {
            let options = easing_options(name, step_frames, amplitude, period, points);
            EasingCatalogRowData {
                header: false,
                easing_index: index as i32,
                name: SharedString::from(name),
                label: SharedString::from(easing_label(name)),
                preview_path: SharedString::from(easing_preview_path_with_steps(&options, 48)),
            }
        }));
    }
    rows
}

fn sync_easing_catalog(window: &EasingConfigWindow, query: &str, curve: &BezierCurve) {
    update_vec_model(
        &window.get_easing_catalog_rows(),
        easing_catalog_rows(
            query,
            window.get_step_frames(),
            window.get_elastic_amplitude(),
            window.get_elastic_period(),
            curve.points(),
        ),
    );
}

fn easing_category_label(category: &str) -> &'static str {
    match category {
        "Basic" => localized("Basic", "基础", "基本"),
        "Standard curves" => localized("Standard curves", "标准曲线", "標準カーブ"),
        "Strong curves" => localized("Strong curves", "强曲线", "強いカーブ"),
        "Bounce and elasticity" => localized("Bounce and elasticity", "回弹与弹性", "反動と弾性"),
        "Special" => localized("Special", "特殊", "特殊"),
        _ => "",
    }
}

fn refresh_easing_translations(window: &EasingConfigWindow) {
    let names = window.get_easing_names();
    let labels = (0..names.row_count())
        .filter_map(|index| names.row_data(index))
        .map(|name| SharedString::from(easing_label(name.as_str())))
        .collect();
    update_vec_model(&window.get_easing_labels(), labels);
    let rows = window.get_easing_catalog_rows();
    let translated_rows = (0..rows.row_count())
        .filter_map(|index| rows.row_data(index))
        .map(|mut row| {
            row.label = if row.header {
                SharedString::from(easing_category_label(row.name.as_str()))
            } else {
                SharedString::from(easing_label(row.name.as_str()))
            };
            row
        })
        .collect();
    update_vec_model(&rows, translated_rows);
}

fn sync_easing_window(
    window: &EasingConfigWindow,
    effect_index: usize,
    param_name: &str,
    point: &KeyframePoint,
) -> BezierCurve {
    let interpolation = if point.interpolation == "bezier" {
        "custom"
    } else {
        point.interpolation.as_str()
    };
    let names = keyframe_interpolation_names();
    let selected_index = names
        .iter()
        .position(|name| *name == interpolation)
        .map_or(0, |index| index as i32);
    let mode_params = point
        .options
        .get("modeParams")
        .and_then(serde_json::Value::as_object);
    let step_frames = mode_params
        .and_then(|params| params.get("stepFrames"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(1)
        .max(1);
    let amplitude = mode_params
        .and_then(|params| params.get("amplitude"))
        .and_then(serde_json::Value::as_f64)
        .map_or(1.0, |value| finite_f32(value, 1.0));
    let period = mode_params
        .and_then(|params| params.get("period"))
        .and_then(serde_json::Value::as_f64)
        .map_or(0.3, |value| finite_f32(value, 0.3));
    let custom_points = point
        .options
        .get("points")
        .and_then(serde_json::Value::as_array)
        .and_then(|points| {
            points
                .iter()
                .map(serde_json::Value::as_f64)
                .collect::<Option<Vec<_>>>()
        })
        .unwrap_or_default();
    let curve = BezierCurve::from_points(&custom_points);
    let controls = curve.first_controls();

    window.set_initializing(true);
    window.set_effect_index(effect_index as i32);
    window.set_param_name(SharedString::from(param_name));
    window.set_keyframe_frame(point.frame);
    window.set_selected_easing_index(selected_index);
    window.set_step_frames(step_frames);
    window.set_elastic_amplitude(amplitude);
    window.set_elastic_period(period);
    window.set_preview_scale(1.0);
    window.set_preview_offset_x(0.0);
    window.set_preview_offset_y(0.0);
    window.set_custom_x1(finite_f32(controls[0], 0.33));
    window.set_custom_y1(finite_f32(controls[1], 0.0));
    window.set_custom_x2(finite_f32(controls[2], 0.66));
    window.set_custom_y2(finite_f32(controls[3], 1.0));
    window.set_initializing(false);
    curve
}

fn sync_easing_curve(window: &EasingConfigWindow, curve: &BezierCurve) {
    let controls = curve.first_controls();
    let initializing = window.get_initializing();
    window.set_initializing(true);
    window.set_custom_x1(finite_f32(controls[0], 0.33));
    window.set_custom_y1(finite_f32(controls[1], 0.0));
    window.set_custom_x2(finite_f32(controls[2], 0.66));
    window.set_custom_y2(finite_f32(controls[3], 1.0));
    window.set_initializing(initializing);
    sync_easing_preview(window, curve);
}

fn invoke_current_easing(window: &EasingConfigWindow) {
    let interpolation = easing_name_at(window.get_selected_easing_index());
    window.invoke_apply_easing(
        SharedString::from(interpolation),
        window.get_step_frames(),
        window.get_elastic_amplitude(),
        window.get_elastic_period(),
        window.get_custom_x1(),
        window.get_custom_y1(),
        window.get_custom_x2(),
        window.get_custom_y2(),
    );
}

fn sync_easing_preview(window: &EasingConfigWindow, curve: &BezierCurve) {
    let interpolation = easing_name_at(window.get_selected_easing_index());
    let options = easing_options(
        interpolation,
        window.get_step_frames(),
        window.get_elastic_amplitude(),
        window.get_elastic_period(),
        curve.points(),
    );
    window.set_preview_path(SharedString::from(easing_preview_path(&options)));
    window.set_tangent_path(SharedString::from(easing_tangent_path(curve)));
    update_vec_model(
        &window.get_curve_handles(),
        curve
            .handles()
            .into_iter()
            .map(|handle| CurveHandleData {
                index: handle.index as i32,
                x: finite_f32(handle.x, 0.0),
                y: finite_f32(handle.y, 0.0),
                anchor: handle.anchor,
                removable: handle.removable,
            })
            .collect(),
    );
}

fn easing_preview_path(options: &serde_json::Value) -> String {
    easing_preview_path_with_steps(options, 128)
}

fn easing_preview_path_with_steps(options: &serde_json::Value, steps: i32) -> String {
    let mut samples = sample_easing_curve(options, steps).into_iter();
    let Some((first_x, first_y)) = samples.next() else {
        return String::new();
    };
    let mut path = format!("M {first_x} {}", 1.0 - first_y);
    for (x, y) in samples {
        path.push_str(&format!(" L {x} {}", 1.0 - y));
    }
    path
}

fn easing_tangent_path(curve: &BezierCurve) -> String {
    let mut path = String::new();
    let mut previous = (0.0, 0.0);
    for points in curve.points().as_chunks::<6>().0 {
        path.push_str(&format!(
            " M {} {} L {} {} M {} {} L {} {}",
            previous.0,
            1.0 - previous.1,
            points[0],
            1.0 - points[1],
            points[4],
            1.0 - points[5],
            points[2],
            1.0 - points[3]
        ));
        previous = (points[4], points[5]);
    }
    path
}

fn easing_options(
    interpolation: &str,
    step_frames: i32,
    amplitude: f32,
    period: f32,
    points: &[f64],
) -> serde_json::Value {
    let interpolation = keyframe_interpolation_names()
        .into_iter()
        .find(|name| *name == interpolation)
        .unwrap_or("none");
    let mut options = serde_json::Map::new();
    options.insert(
        "interp".to_owned(),
        serde_json::Value::String(interpolation.to_owned()),
    );
    if interpolation == "custom" {
        let mut points = if points.len() >= 6 && points.len().is_multiple_of(6) {
            points.to_vec()
        } else {
            vec![0.33, 0.0, 0.66, 1.0, 1.0, 1.0]
        };
        points[0] = points[0].clamp(0.0, 1.0);
        points[2] = points[2].clamp(0.0, 1.0);
        options.insert("points".to_owned(), serde_json::json!(points));
    } else if interpolation == "random" || interpolation == "alternate" {
        options.insert(
            "modeParams".to_owned(),
            serde_json::json!({"stepFrames": step_frames.max(1)}),
        );
    } else if interpolation.contains("elastic") {
        let amplitude = if amplitude.is_finite() {
            amplitude.clamp(0.1, 5.0)
        } else {
            1.0
        };
        let period = if period.is_finite() {
            period.clamp(0.05, 1.0)
        } else {
            0.3
        };
        options.insert(
            "modeParams".to_owned(),
            serde_json::json!({"amplitude": amplitude, "period": period}),
        );
    }
    serde_json::Value::Object(options)
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
    let duration = workspace.timeline_duration();
    main.set_playhead(workspace.playhead() as f32);
    main.set_duration(duration as f32);
    main.set_playing(workspace.is_playing());
    main.set_playback_speed_percent(
        (workspace.playback_speed() * 100.0)
            .round()
            .clamp(10.0, 400.0) as i32,
    );
    main.set_status_text(SharedString::from(workspace.status()));
    timeline.set_playhead(workspace.playhead());
    timeline.set_duration(workspace.timeline_view_duration());
    timeline.set_selected_layer(workspace.selected_layer());
    timeline.set_action_status(SharedString::from(workspace.status()));
}

fn parse_required_i32(value: &str, label: &str, minimum: i32, maximum: i32) -> Result<i32, String> {
    let parsed = value.trim().parse::<i32>().map_err(|_| {
        match CURRENT_UI_LANGUAGE.with(|language| language.get()) {
            UiLanguage::English => format!("Enter an integer for {label}."),
            UiLanguage::SimplifiedChinese => format!("请为{label}输入整数。"),
            UiLanguage::Japanese => format!("{label}には整数を入力してください"),
        }
    })?;
    if (minimum..=maximum).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(match CURRENT_UI_LANGUAGE.with(|language| language.get()) {
            UiLanguage::English => {
                format!("Enter {label} in the range {minimum} to {maximum}.")
            }
            UiLanguage::SimplifiedChinese => {
                format!("请在 {minimum} 到 {maximum} 的范围内输入{label}。")
            }
            UiLanguage::Japanese => {
                format!("{label}は{minimum}から{maximum}の範囲で入力してください")
            }
        })
    }
}

fn parse_required_f64(value: &str, label: &str, minimum: f64, maximum: f64) -> Result<f64, String> {
    let parsed = value.trim().parse::<f64>().map_err(|_| {
        match CURRENT_UI_LANGUAGE.with(|language| language.get()) {
            UiLanguage::English => format!("Enter a number for {label}."),
            UiLanguage::SimplifiedChinese => format!("请为{label}输入数值。"),
            UiLanguage::Japanese => format!("{label}には数値を入力してください"),
        }
    })?;
    if parsed.is_finite() && (minimum..=maximum).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(match CURRENT_UI_LANGUAGE.with(|language| language.get()) {
            UiLanguage::English => {
                format!("Enter {label} in the range {minimum} to {maximum}.")
            }
            UiLanguage::SimplifiedChinese => {
                format!("请在 {minimum} 到 {maximum} 的范围内输入{label}。")
            }
            UiLanguage::Japanese => {
                format!("{label}は{minimum}から{maximum}の範囲で入力してください")
            }
        })
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

/// Selects a persisted language or follows the operating-system locale.
/// English is the source language and therefore also the fallback for an
/// unsupported or unavailable locale.
fn select_bundled_ui_translation(settings: &SettingsStore) -> Result<UiLanguage, String> {
    let language = configured_ui_language(settings);
    slint::select_bundled_translation(language.slint_locale())
        .map_err(|error| error.to_string())?;
    CURRENT_UI_LANGUAGE.with(|current| current.set(language));
    Ok(language)
}

fn configured_ui_language(settings: &SettingsStore) -> UiLanguage {
    match settings
        .value("uiLanguage")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("System")
    {
        "SimplifiedChinese" => UiLanguage::SimplifiedChinese,
        "Japanese" => UiLanguage::Japanese,
        "English" => UiLanguage::English,
        _ => system_ui_language(),
    }
}

fn system_ui_language() -> UiLanguage {
    let locale = sys_locale::get_locale()
        .or_else(|| {
            std::env::var("LC_ALL")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            std::env::var("LC_MESSAGES")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .or_else(|| std::env::var("LANG").ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    ui_language_from_locale(&locale)
}

fn ui_language_from_locale(locale: &str) -> UiLanguage {
    let locale = locale.trim().to_ascii_lowercase();
    if locale.starts_with("zh") {
        UiLanguage::SimplifiedChinese
    } else if locale.starts_with("ja") {
        UiLanguage::Japanese
    } else {
        UiLanguage::English
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_app::object_settings::{
        AudioPluginSettings, KeyframePoint, ObjectControlKind, ObjectControlOption, ObjectEffect,
    };
    use serde_json::json;

    #[test]
    fn window_geometry_uses_qt_keys_and_clamps_invalid_sizes() {
        let fallback = WindowGeometry::new(100, 200, 640, 480);
        let value = json!({
            "x": -120,
            "y": 45,
            "width": 1280,
            "height": 720,
            "maximized": true
        });
        assert_eq!(
            WindowGeometry::from_value(Some(&value), fallback),
            WindowGeometry {
                x: -120,
                y: 45,
                width: 1280,
                height: 720,
                maximized: true,
            }
        );

        let invalid = json!({"width": 0, "height": -5});
        assert_eq!(
            WindowGeometry::from_value(Some(&invalid), fallback),
            WindowGeometry {
                width: 1,
                height: 1,
                ..fallback
            }
        );
        assert_eq!(WindowGeometry::from_value(None, fallback), fallback);
    }

    #[test]
    fn system_settings_choices_and_plugin_paths_match_the_qt_schema() {
        assert_eq!(choice_str(&SYSTEM_THEME_VALUES, 2, 0), "System");
        assert_eq!(
            choice_str(&SYSTEM_UI_LANGUAGE_VALUES, 2, 0),
            "SimplifiedChinese"
        );
        assert_eq!(choice_str(&SYSTEM_EXPORT_VIDEO_CODECS, 1, 2), "hevc_vaapi");
        assert_eq!(choice_str(&SYSTEM_EXPORT_AUDIO_CODECS, 1, 0), "opus");
        assert_eq!(choice_i32(&SYSTEM_AUDIO_BLOCK_SIZES, 5, 4), 8192);
        assert_eq!(choice_f64(&SYSTEM_PREVIEW_RENDER_SCALES, 3, 0), 0.25);
        assert_eq!(choice_str(&SYSTEM_THEME_VALUES, -1, 0), "Dark");
        assert_eq!(choice_i32(&SYSTEM_AUDIO_BLOCK_SIZES, 99, 4), 4096);
        assert_eq!(
            plugin_paths_value(" /plugins/one \n\n/plugins/two "),
            json!(["/plugins/one", "/plugins/two"])
        );
        assert_eq!(SYSTEM_PLUGIN_FORMATS.len(), 11);
        assert_eq!(SYSTEM_SHORTCUT_ROWS.len(), 34);
    }

    #[test]
    fn ui_locale_selection_uses_english_as_the_fallback() {
        assert_eq!(
            ui_language_from_locale("zh-CN"),
            UiLanguage::SimplifiedChinese
        );
        assert_eq!(
            ui_language_from_locale("zh_Hans_CN.UTF-8"),
            UiLanguage::SimplifiedChinese
        );
        assert_eq!(ui_language_from_locale("ja-JP"), UiLanguage::Japanese);
        assert_eq!(ui_language_from_locale("en-US"), UiLanguage::English);
        assert_eq!(ui_language_from_locale("fr-FR"), UiLanguage::English);
        assert_eq!(ui_language_from_locale(""), UiLanguage::English);
    }

    #[test]
    fn effect_metadata_follows_the_selected_ui_language() {
        CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::English));
        assert_eq!(localized_effect_metadata("テキスト"), "Text");
        assert_eq!(localized_effect_metadata("変形/クロップ"), "Transform/Crop");

        CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::SimplifiedChinese));
        assert_eq!(localized_effect_metadata("テキスト"), "文本");
        assert_eq!(localized_effect_metadata("変形/クロップ"), "变形/裁剪");

        CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::Japanese));
        assert_eq!(localized_effect_metadata("テキスト"), "テキスト");
        assert_eq!(localized_effect_metadata("変形/クロップ"), "変形/クロップ");
    }

    #[test]
    fn searchable_context_catalog_preserves_qt_category_paths() {
        let (catalog, _) = EffectCatalog::load();
        let items =
            timeline_context_catalog_items(&catalog, &AudioPluginCatalog::default(), "clipping", 1);
        let clipping = items
            .iter()
            .find(|item| item.id.as_str() == "clipping")
            .expect("clipping effect remains searchable by technical id");
        assert!(clipping.categories.as_str().contains("Transform/Crop"));
    }

    #[test]
    fn codec_selection_matches_the_qt_fallback_contract() {
        let codecs = vec!["libx264".to_owned(), "libx265".to_owned()];
        assert_eq!(selected_codec_index(&codecs, "libx265"), 1);
        assert_eq!(selected_codec_index(&codecs, "missing"), 0);
        assert_eq!(selected_codec_index(&[], "libx264"), -1);
        assert_eq!(selected_codec(&codecs, 1, "fallback"), "libx265");
        assert_eq!(selected_codec(&codecs, -1, "fallback"), "fallback");
        assert_eq!(selected_codec(&codecs, 9, "fallback"), "fallback");
    }

    #[test]
    fn export_frame_gate_rejects_a_project_tab_change() {
        let mut model = ApplicationModel::default();
        model.create_project(ProjectDefaults::default());
        let export_project = model
            .current_project_instance_id()
            .expect("first project exists");
        model.create_project(ProjectDefaults::default());

        let error = match export_workspace_for_frame(&mut model, export_project) {
            Ok(_) => panic!("another project must not render the active export frame"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            "Frame render error: project tab changed during export"
        );
        assert!(model.select_project(0));
        assert!(export_workspace_for_frame(&mut model, export_project).is_ok());
    }

    #[test]
    fn shortcut_parser_preserves_qt_cross_platform_names_and_punctuation() {
        let undo = parse_shortcut("Ctrl+Shift+Z").expect("undo shortcut parses");
        assert!(undo.control);
        assert!(undo.shift);
        assert_eq!(undo.text, "z");

        let settings = parse_shortcut("Alt+Enter").expect("settings shortcut parses");
        assert!(settings.alt);
        assert_eq!(settings.text, char::from(Key::Return).to_string());

        let zoom = parse_shortcut("Ctrl++").expect("plus shortcut parses");
        assert!(zoom.control);
        assert!(zoom.ignore_shift);
        assert_eq!(zoom.text, "+");
        assert!(parse_shortcut("Ctrl+UnknownKey").is_none());
    }

    #[test]
    fn shortcut_matching_uses_slint_command_and_physical_control_semantics() {
        let command_save = ShortcutInput {
            text: "s".to_owned(),
            alt: false,
            control: true,
            shift: false,
            meta: false,
        };
        assert!(shortcut_matches("Ctrl+S", &command_save));

        let physical_control_save = ShortcutInput {
            control: false,
            meta: true,
            ..command_save.clone()
        };
        assert!(shortcut_matches("Meta+S", &physical_control_save));
        if cfg!(target_os = "macos") {
            assert!(shortcut_matches("Command+S", &command_save));
            assert!(!shortcut_matches("Control+S", &command_save));
            assert!(shortcut_matches("Control+S", &physical_control_save));
        } else {
            assert!(shortcut_matches("Control+S", &command_save));
            assert!(!shortcut_matches("Command+S", &command_save));
            assert!(shortcut_matches("Command+S", &physical_control_save));
        }
    }

    #[test]
    fn easing_options_match_the_qt_parameter_contracts() {
        assert_eq!(easing_label("ease_in_out_elastic"), "Elastic Ease in/out");
        assert_eq!(easing_label("ease_out_sine"), "Sine Ease out");
        assert_eq!(easing_label("ease_in_quad"), "Quadratic Ease in");
        assert_eq!(easing_label("ease_out_in_circ"), "Circular Ease out/in");
        assert_eq!(easing_label("ease_in_bounce"), "Bounce Ease in");
        assert_eq!(easing_name_at(0), "none");
        assert_eq!(easing_name_at(-1), "none");

        let random = easing_options("random", 0, 1.0, 0.3, &[0.33, 0.0, 0.66, 1.0, 1.0, 1.0]);
        assert_eq!(random["interp"], "random");
        assert_eq!(random["modeParams"]["stepFrames"], 1);

        let elastic = easing_options(
            "ease_out_elastic",
            1,
            f32::INFINITY,
            0.0,
            &[0.33, 0.0, 0.66, 1.0, 1.0, 1.0],
        );
        assert_eq!(elastic["modeParams"]["amplitude"], 1.0);
        let period = elastic["modeParams"]["period"]
            .as_f64()
            .expect("elastic period is numeric");
        assert!((period - 0.05).abs() < f64::from(f32::EPSILON));

        let custom = easing_options(
            "custom",
            1,
            1.0,
            0.3,
            &[-2.0, -1.0, 3.0, 2.0, 0.5, 0.5, 0.6, 0.6, 0.8, 0.8, 1.0, 1.0],
        );
        assert_eq!(
            custom["points"],
            json!([0.0, -1.0, 1.0, 2.0, 0.5, 0.5, 0.6, 0.6, 0.8, 0.8, 1.0, 1.0])
        );
        assert_eq!(
            easing_options("unknown", 1, 1.0, 0.3, &[0.0; 6])["interp"],
            "none"
        );

        let linear = easing_preview_path(&easing_options(
            "linear",
            1,
            1.0,
            0.3,
            BezierCurve::default().points(),
        ));
        assert!(linear.starts_with("M 0 1"));
        assert!(linear.contains(" L 0.5 0.5"));
        assert!(linear.ends_with(" L 1 0"));

        let mut curve = BezierCurve::default();
        assert!(curve.insert_anchor(0.5, 0.25));
        let tangents = easing_tangent_path(&curve);
        assert!(tangents.contains("M 0 1 L 0.165 0.9175"));
        assert!(tangents.contains("M 0.5 0.75 L 0.33 0.835"));
    }

    #[test]
    fn easing_catalog_matches_the_qt_categories_and_filtering() {
        let curve = BezierCurve::default();
        let rows = easing_catalog_rows("", 1, 1.0, 0.3, curve.points());
        assert_eq!(
            rows.iter()
                .filter(|row| row.header)
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Basic",
                "Standard curves",
                "Strong curves",
                "Bounce and elasticity",
                "Special"
            ]
        );

        let mut actual_names = rows
            .iter()
            .filter(|row| !row.header)
            .map(|row| row.name.as_str().to_owned())
            .collect::<Vec<_>>();
        let mut expected_names = keyframe_interpolation_names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        actual_names.sort();
        expected_names.sort();
        assert_eq!(actual_names, expected_names);
        assert!(rows.iter().filter(|row| !row.header).all(|row| {
            easing_name_at(row.easing_index) == row.name.as_str()
                && row.preview_path.as_str().starts_with("M 0 1")
        }));

        let bounce = easing_catalog_rows("Bo_Un_Ce", 1, 1.0, 0.3, curve.points());
        assert_eq!(bounce.len(), 5);
        assert!(bounce[0].header);
        assert_eq!(bounce[0].label.as_str(), "Bounce and elasticity");
        assert!(
            bounce[1..]
                .iter()
                .all(|row| row.name.as_str().contains("bounce"))
        );
        assert!(easing_catalog_rows("跳ね返り", 1, 1.0, 0.3, curve.points()).is_empty());
    }

    #[test]
    fn easing_catalog_previews_follow_live_mode_parameters() {
        let curve = BezierCurve::default();
        let random_preview = |step_frames| {
            easing_catalog_rows("random", step_frames, 1.0, 0.3, curve.points())[1]
                .preview_path
                .to_string()
        };
        assert_ne!(random_preview(1), random_preview(4));

        let custom_default = easing_catalog_rows("custom", 1, 1.0, 0.3, curve.points())[1]
            .preview_path
            .to_string();
        let mut edited = curve.clone();
        assert!(edited.set_first_controls([0.1, 0.8, 0.9, 0.2]));
        let custom_edited = easing_catalog_rows("custom", 1, 1.0, 0.3, edited.points())[1]
            .preview_path
            .to_string();
        assert_ne!(custom_default, custom_edited);
    }

    #[test]
    fn parameter_picker_helpers_preserve_qt_value_contracts() {
        assert_eq!(
            qt_file_filters("Video Files (*.mp4 *.mov);;Images (*.png *.jpg)"),
            vec![
                (
                    "Video Files".to_owned(),
                    vec!["mp4".to_owned(), "mov".to_owned()]
                ),
                (
                    "Images".to_owned(),
                    vec!["png".to_owned(), "jpg".to_owned()]
                )
            ]
        );
        assert!(qt_file_filters("").is_empty());
        assert_eq!(parse_qt_color("#abc"), [255, 0xaa, 0xbb, 0xcc]);
        assert_eq!(parse_qt_color("#80ff0000"), [0x80, 0xff, 0, 0]);
        assert_eq!(parse_qt_color("invalid"), [255; 4]);
        assert_eq!(format_qt_color(255, 0, 16, 255), "#ff0010");
        assert_eq!(format_qt_color(300, -1, 16, 128), "#80ff0010");
    }

    #[test]
    fn font_picker_filter_is_case_insensitive_and_stable() {
        let families = vec![
            "Arial".to_owned(),
            "Noto Sans CJK JP".to_owned(),
            "Noto Serif".to_owned(),
        ];
        assert_eq!(
            filtered_font_families(&families, "  NOTO ")
                .into_iter()
                .map(|family| family.to_string())
                .collect::<Vec<_>>(),
            vec!["Noto Sans CJK JP", "Noto Serif"]
        );
        assert_eq!(filtered_font_families(&families, "").len(), 3);
    }

    #[test]
    fn zoom_mapping_matches_the_qt_piecewise_scale() {
        assert_eq!(zoom_percent_to_scale(10.0), 0.1);
        assert_eq!(zoom_percent_to_scale(100.0), 1.0);
        assert_eq!(zoom_percent_to_scale(200.0), 4.0);
        assert_eq!(zoom_percent_to_scale(400.0), 10.0);
        assert!((scale_to_zoom_percent(4.0) - 200.0).abs() < f32::EPSILON);
    }

    fn timeline_scroll_input() -> TimelineScrollInput {
        TimelineScrollInput {
            delta_x: 20.0,
            delta_y: 120.0,
            content_x: 500.0,
            viewport_x: -300.0,
            viewport_y: -90.0,
            visible_width: 800.0,
            viewport_width: 3_600.0,
            visible_height: 300.0,
            viewport_height: 3_840.0,
            duration_frames: 3_600.0,
            alt: false,
            control: false,
            shift: false,
            ruler_zoom: false,
            pixels_per_frame: 1.0,
        }
    }

    #[test]
    fn timeline_wheel_matches_qt_axis_and_modifier_routing() {
        let zoom = (10.0, 400.0, 10.0);
        let horizontal = plan_timeline_scroll_with(timeline_scroll_input(), zoom);
        assert_eq!(horizontal.viewport_x, -180.0);
        assert_eq!(horizontal.viewport_y, -90.0);

        let vertical = plan_timeline_scroll_with(
            TimelineScrollInput {
                shift: true,
                ..timeline_scroll_input()
            },
            zoom,
        );
        assert_eq!(vertical.viewport_x, -300.0);
        assert_eq!(vertical.viewport_y, 0.0);
    }

    #[test]
    fn timeline_wheel_zoom_preserves_the_pointer_anchor() {
        let zoom = (10.0, 400.0, 10.0);
        let stepped = plan_timeline_scroll_with(
            TimelineScrollInput {
                control: true,
                ..timeline_scroll_input()
            },
            zoom,
        );
        assert!((stepped.pixels_per_frame - 1.3).abs() < f32::EPSILON);
        assert!((stepped.viewport_x + 450.0).abs() < f32::EPSILON);

        let ruler = plan_timeline_scroll_with(
            TimelineScrollInput {
                ruler_zoom: true,
                ..timeline_scroll_input()
            },
            zoom,
        );
        assert!((ruler.pixels_per_frame - 1.1).abs() < f32::EPSILON);
        assert!((ruler.viewport_x + 350.0).abs() < f32::EPSILON);
    }

    #[test]
    fn object_setting_rows_keep_effect_control_and_option_order() {
        let settings = ObjectSettings {
            clip_id: 7,
            clip_label: "Rectangle".to_owned(),
            audio_plugin_mode: false,
            effects: vec![ObjectEffect {
                index: 1,
                id: "blur".to_owned(),
                name: "Blur".to_owned(),
                enabled: false,
                selected: true,
                removable: true,
                controls: vec![
                    ObjectControl {
                        kind: ObjectControlKind::Header,
                        source_kind: "header".to_owned(),
                        param: None,
                        label: "Quality".to_owned(),
                        minimum: None,
                        maximum: None,
                        step: None,
                        decimals: None,
                        unit: String::new(),
                        filter: String::new(),
                        disabled: false,
                        keyframed: false,
                        value: serde_json::Value::Null,
                        relative_frame: 0,
                        clip_duration: 100,
                        interval_start: 0,
                        interval_end: 100,
                        start_value: serde_json::Value::Null,
                        end_value: serde_json::Value::Null,
                        start_interpolation: "constant".to_owned(),
                        keyframes: Vec::new(),
                        options: Vec::new(),
                    },
                    ObjectControl {
                        kind: ObjectControlKind::Choice,
                        source_kind: "enum".to_owned(),
                        param: Some("mode".to_owned()),
                        label: "Mode".to_owned(),
                        minimum: None,
                        maximum: None,
                        step: None,
                        decimals: None,
                        unit: String::new(),
                        filter: String::new(),
                        disabled: false,
                        keyframed: true,
                        value: json!("high"),
                        relative_frame: 10,
                        clip_duration: 100,
                        interval_start: 0,
                        interval_end: 100,
                        start_value: json!("high"),
                        end_value: json!("high"),
                        start_interpolation: "linear".to_owned(),
                        keyframes: Vec::new(),
                        options: vec![
                            ObjectControlOption {
                                value: json!("low"),
                                label: "Low".to_owned(),
                            },
                            ObjectControlOption {
                                value: json!("high"),
                                label: "High".to_owned(),
                            },
                        ],
                    },
                    ObjectControl {
                        kind: ObjectControlKind::Number,
                        source_kind: "slider".to_owned(),
                        param: Some("size".to_owned()),
                        label: "Size".to_owned(),
                        minimum: Some(0.0),
                        maximum: Some(100.0),
                        step: Some(1.0),
                        decimals: Some(1),
                        unit: "px".to_owned(),
                        filter: String::new(),
                        disabled: false,
                        keyframed: true,
                        value: json!(10.0),
                        relative_frame: 10,
                        clip_duration: 100,
                        interval_start: 0,
                        interval_end: 20,
                        start_value: json!(0.0),
                        end_value: json!(20.0),
                        start_interpolation: "linear".to_owned(),
                        keyframes: vec![
                            KeyframePoint {
                                frame: 0,
                                value: json!(0.0),
                                interpolation: "linear".to_owned(),
                                options: json!({"interp":"linear"}),
                            },
                            KeyframePoint {
                                frame: 20,
                                value: json!(20.0),
                                interpolation: "ease_in".to_owned(),
                                options: json!({"interp":"ease_in"}),
                            },
                        ],
                        options: Vec::new(),
                    },
                ],
            }],
            audio_plugins: Vec::new(),
        };

        let rows = object_settings_rows(&settings);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].row_kind.as_str(), "effect");
        assert_eq!(rows[1].row_kind.as_str(), "header");
        assert_eq!(rows[2].row_kind.as_str(), "choice");
        assert_eq!(rows[2].selected_option, 1);
        assert!(rows[2].keyframed);
        assert_eq!(rows[2].option_labels.row_count(), 2);
        assert_eq!(rows[2].option_labels.row_data(0).as_deref(), Some("Low"));
        assert_eq!(rows[2].option_labels.row_data(1).as_deref(), Some("High"));
        assert_eq!(rows[3].row_kind.as_str(), "number");
        assert!(rows[3].range_mode);
        assert_eq!((rows[3].start_frame, rows[3].end_frame), (0, 20));
        assert_eq!(rows[3].text_value.as_str(), "0.0");
        assert_eq!(rows[3].end_text_value.as_str(), "20.0");
        assert_eq!(rows[4].row_kind.as_str(), "keyframes");
        assert_eq!(rows[4].keyframe_markers.row_count(), 3);
        assert!(!rows[4].keyframe_markers.row_data(0).unwrap().draggable);
        assert!(rows[4].keyframe_markers.row_data(1).unwrap().draggable);
        let virtual_end = rows[4].keyframe_markers.row_data(2).unwrap();
        assert_eq!(virtual_end.frame, 100);
        assert!(virtual_end.virtual_end);
    }

    #[test]
    fn audio_plugin_rows_use_current_values_and_protect_qt_endpoints() {
        let control = ObjectControl {
            kind: ObjectControlKind::Number,
            source_kind: "slider".to_owned(),
            param: Some("0".to_owned()),
            label: "Gain".to_owned(),
            minimum: Some(0.0),
            maximum: Some(1.0),
            step: Some(0.01),
            decimals: Some(3),
            unit: "dB".to_owned(),
            filter: String::new(),
            disabled: false,
            keyframed: true,
            value: json!(0.5),
            relative_frame: 50,
            clip_duration: 100,
            interval_start: 20,
            interval_end: 80,
            start_value: json!(0.2),
            end_value: json!(0.8),
            start_interpolation: "linear".to_owned(),
            keyframes: vec![
                KeyframePoint {
                    frame: 0,
                    value: json!(0.0),
                    interpolation: "linear".to_owned(),
                    options: json!({"interp":"linear"}),
                },
                KeyframePoint {
                    frame: 50,
                    value: json!(0.5),
                    interpolation: "linear".to_owned(),
                    options: json!({"interp":"linear"}),
                },
                KeyframePoint {
                    frame: 100,
                    value: json!(1.0),
                    interpolation: "linear".to_owned(),
                    options: json!({"interp":"linear"}),
                },
            ],
            options: Vec::new(),
        };
        let rows = object_settings_rows(&ObjectSettings {
            clip_id: 9,
            clip_label: "Audio".to_owned(),
            audio_plugin_mode: true,
            effects: Vec::new(),
            audio_plugins: vec![AudioPluginSettings {
                index: 0,
                id: "gain".to_owned(),
                name: "Gain".to_owned(),
                format: "CLAP".to_owned(),
                enabled: true,
                selected: true,
                controls: vec![control],
            }],
        });

        assert_eq!(rows.len(), 3);
        assert!(rows[0].audio_plugin);
        assert!(!rows[0].header_toggle_visible);
        assert_eq!(rows[1].start_frame, 50);
        assert_eq!(rows[1].text_value.as_str(), "0.500");
        assert!(!rows[1].range_mode);
        assert!(rows[1].interactive);
        let markers = &rows[2].keyframe_markers;
        assert_eq!(markers.row_count(), 3);
        assert!(!markers.row_data(0).unwrap().removable);
        assert!(markers.row_data(1).unwrap().removable);
        assert!(!markers.row_data(1).unwrap().draggable);
        assert!(!markers.row_data(2).unwrap().removable);
    }
}
