//! Export window commands and production export runtime coordination.
use crate::dialogs::show_and_redraw;
use crate::localization::{UiLanguage, current_ui_language, localized};
use crate::playback::PreviewSourceKey;
use crate::projection::{setting_string, sync_transport_weak};
use crate::{ExportWindow, MainWindow, TimelineWindow};
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::{ApplicationModel, WorkspaceModel};
use aviqtl_export::{
    ExportEvent, ExportImageFormat, ExportManager, ExportMode, ExportProject, ExportRequest,
    ExportRuntimeSettings, available_audio_encoders, available_video_encoders, is_software_codec,
    nearest_audio_bitrate, valid_export_path,
};
use aviqtl_preview::{PlannedPreview, PreviewPlanner};
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub(super) const VIDEO_CODECS: [(&str, &str); 14] = [
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

pub(super) const AUDIO_CODECS: [(&str, &str); 5] = [
    ("AAC", "aac"),
    ("Opus", "libopus"),
    ("MP3", "libmp3lame"),
    ("FLAC (Lossless)", "flac"),
    ("PCM 16-bit", "pcm_s16le"),
];

pub(super) const PRESET_VALUES: [&str; 5] = ["ultrafast", "fast", "medium", "slow", "veryslow"];

pub(super) const PROFILE_VALUES: [&str; 4] = ["", "baseline", "main", "high"];

pub(super) const AUDIO_BITRATES: [i32; 5] = [96, 128, 192, 256, 320];

#[derive(Default)]
pub(super) struct ExportCodecState {
    pub(super) video_values: Vec<String>,
    pub(super) audio_values: Vec<String>,
}

#[derive(Default)]
pub(super) struct ExportPlannerRuntime {
    pub(super) planner: Option<PreviewPlanner>,
    pub(super) source_key: Option<PreviewSourceKey>,
}

impl ExportPlannerRuntime {
    pub(super) fn reset(&mut self) {
        self.planner = None;
        self.source_key = None;
    }

    pub(super) fn build(
        &mut self,
        project_instance_id: u64,
        workspace: &WorkspaceModel,
        frame: i32,
        effect_catalog: &EffectCatalog,
    ) -> Option<PlannedPreview> {
        let source_key = PreviewSourceKey {
            project_instance_id,
            document_revision: workspace.document_revision(),
            project_path: workspace.project().path.clone(),
        };
        if self.source_key.as_ref() != Some(&source_key) {
            if let Some(planner) = self.planner.as_mut() {
                planner.rebuild(workspace.document(), workspace.project().path.as_deref());
                planner.set_native_definitions(effect_catalog.native_definitions());
            } else {
                let mut planner =
                    PreviewPlanner::new(workspace.document(), workspace.project().path.as_deref());
                planner.set_native_definitions(effect_catalog.native_definitions());
                self.planner = Some(planner);
            }
            self.source_key = Some(source_key);
        }
        self.planner.as_mut().and_then(|planner| {
            planner.build(workspace.document(), workspace.selected_scene(), frame)
        })
    }
}

pub(super) fn initialize_export_draft(window: &ExportWindow, settings: &SettingsStore) {
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
pub(super) fn install_export_callbacks(
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

pub(super) fn export_workspace_for_frame(
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

pub(super) fn update_export(
    export: &slint::Weak<ExportWindow>,
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
    manager: &Rc<RefCell<ExportManager>>,
    planner: &Rc<RefCell<ExportPlannerRuntime>>,
    effect_catalog: &Rc<RefCell<EffectCatalog>>,
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
                    .build(
                        request.project_instance_id,
                        workspace,
                        request.frame,
                        &effect_catalog.borrow(),
                    )
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

pub(super) fn selected_codec_index(values: &[String], selected: &str) -> i32 {
    if values.is_empty() {
        return -1;
    }
    values
        .iter()
        .position(|value| value == selected)
        .unwrap_or(0) as i32
}

pub(super) fn selected_codec<'a>(values: &'a [String], index: i32, fallback: &'a str) -> &'a str {
    usize::try_from(index)
        .ok()
        .and_then(|index| values.get(index))
        .map(String::as_str)
        .unwrap_or(fallback)
}

fn export_progress_label(progress: aviqtl_export::ExportProgressPlan) -> String {
    let language = current_ui_language();
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
