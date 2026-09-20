//! Shared stable-model updates and project/transport projection.

use crate::localization::localized_effect_metadata;
use crate::object_settings::finite_f32;
use crate::{
    LayerData, MainWindow, MissingMediaData, ProjectTabData, SceneTabData, TimelineClipData,
    TimelineWindow,
};
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::{ApplicationModel, MAX_TIMELINE_LAYERS};
use slint::{Color, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn setting_string(settings: &SettingsStore, key: &str, fallback: &str) -> String {
    settings
        .value(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

pub(super) fn sync_weak_windows(
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
) {
    if let (Some(main), Some(timeline)) = (main.upgrade(), timeline.upgrade()) {
        sync_windows(&main, &timeline, &model.borrow());
    }
}

pub(super) fn sync_transport_weak(
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
) {
    if let (Some(main), Some(timeline)) = (main.upgrade(), timeline.upgrade()) {
        sync_transport(&main, &timeline, &model.borrow());
    }
}

pub(super) fn sync_windows(main: &MainWindow, timeline: &TimelineWindow, model: &ApplicationModel) {
    sync_project_tabs(main, model);

    let Some(workspace) = model.current_workspace() else {
        update_vec_model(&main.get_missing_media(), Vec::new());
        main.set_missing_media_dialog_visible(false);
        update_vec_model(&timeline.get_scene_tabs(), Vec::new());
        update_vec_model(&timeline.get_clips(), Vec::new());
        update_vec_model(&timeline.get_layers(), Vec::new());
        timeline.set_playhead(0);
        timeline.set_duration(0);
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
        .map(|clip| {
            let style = timeline_clip_style(&clip.clip_type);
            let has_clip_color = style.is_some();
            let (clip_color, dark_text) = style.unwrap_or_default();
            TimelineClipData {
                id: clip.id,
                label: SharedString::from(localized_effect_metadata(&clip.label).into_owned()),
                clip_type: SharedString::from(clip.clip_type),
                clip_color,
                has_clip_color,
                dark_text,
                waveform: ModelRc::new(VecModel::<f32>::default()),
                start: clip.start,
                duration: clip.duration,
                layer: clip.layer,
                audio: clip.audio,
                locked: clip.locked,
                control_layer_count: clip.control_layer_count,
                clip_by_upper_object: clip.clip_by_upper_object,
                selected: clip.selected,
                primary: clip.primary,
            }
        })
        .collect::<Vec<_>>();
    update_vec_model(&timeline.get_clips(), clips);
    let selected_layer = workspace.selected_layer();
    let scene = workspace.selected_scene_document();
    let layers = (0..timeline.get_maximum_layers().clamp(1, MAX_TIMELINE_LAYERS))
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

fn timeline_clip_style(clip_type: &str) -> Option<(Color, bool)> {
    let [red, green, blue] = match clip_type {
        "audio" => [0xd0, 0x30, 0x30],
        "counter" | "flare" | "lens_flare_object" | "pie_shape" | "polygon_shape"
        | "radial_lines" | "star" | "track_line" => [0x3b, 0x82, 0xf6],
        _ => return None,
    };
    let luma = 0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue);
    Some((Color::from_rgb_u8(red, green, blue), luma > 0.6 * 255.0))
}

pub(super) fn sync_project_tabs(main: &MainWindow, model: &ApplicationModel) {
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
}

pub(super) fn update_vec_model<T: Clone + 'static>(model: &ModelRc<T>, rows: Vec<T>) {
    let Some(model) = model.as_any().downcast_ref::<VecModel<T>>() else {
        eprintln!("UI list was replaced with a non-VecModel; skipping sync");
        return;
    };
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

pub(super) fn sync_transport(
    main: &MainWindow,
    timeline: &TimelineWindow,
    model: &ApplicationModel,
) {
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
    main.set_frame_counter_text(SharedString::from(frame_counter_text(
        workspace.playhead(),
        duration,
    )));
    timeline.set_playhead(workspace.playhead());
    timeline.set_duration(workspace.timeline_view_duration());
    timeline.set_selected_layer(workspace.selected_layer());
    timeline.set_action_status(SharedString::from(workspace.status()));
    if let Some(scene) = workspace.scene_settings(workspace.selected_scene()) {
        timeline.set_grid_mode(SharedString::from(scene.grid_mode));
        timeline.set_grid_fps(finite_f32(scene.fps, 60.0));
        timeline.set_grid_bpm(finite_f32(scene.grid_bpm, 120.0));
        timeline.set_grid_offset(finite_f32(scene.grid_offset, 0.0));
        timeline.set_grid_interval(scene.grid_interval.max(1));
        timeline.set_grid_subdivision(scene.grid_subdivision.max(1));
    }
}

/// Format the transport frame counter the way Qt does.
///
/// Qt pads the current frame to the total's digit count and lets the label size
/// itself, so the counter never loses leading digits and keeps a stable width
/// while the numbers change (`MainWindow.qml:1035-1049`).
pub(super) fn frame_counter_text(playhead: i32, duration: i32) -> String {
    let total = duration.max(0);
    let current = playhead.max(0);
    let width = total.to_string().len();
    format!("{current:0>width$} / {total}")
}
