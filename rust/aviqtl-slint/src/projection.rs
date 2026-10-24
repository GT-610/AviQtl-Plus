//! Shared stable-model updates and project/transport projection.

use crate::localization::{localized, localized_effect_metadata, status_line};
use crate::object_settings::finite_f32;
use crate::{
    LayerData, MainWindow, MediaBinEntryData, MediaBinGroupData, MissingMediaData, ProjectTabData,
    SceneTabData, TimelineClipData, TimelineWindow,
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
        // The timeline window's copy of the menu greys out the same item.
        timeline.set_has_missing_media(false);
        update_vec_model(&timeline.get_scene_tabs(), Vec::new());
        update_vec_model(&timeline.get_clips(), Vec::new());
        update_vec_model(&timeline.get_layers(), Vec::new());
        update_vec_model(&timeline.get_media_entries(), Vec::new());
        update_vec_model(&timeline.get_media_groups(), Vec::new());
        timeline.set_playhead(0);
        timeline.set_duration(0);
        timeline.set_selected_clip_count(0);
        return;
    };
    let missing_media = workspace.missing_media();
    let missing_media = missing_media
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
    timeline.set_has_missing_media(has_missing_media);
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
                midpoints: ModelRc::new(VecModel::from(workspace.clip_midpoints(clip.id))),
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
    sync_media_bin(timeline, workspace);
    sync_transport(main, timeline, model);
}

pub(super) fn sync_media_bin(timeline: &TimelineWindow, workspace: &aviqtl_app::WorkspaceModel) {
    let bin = workspace.media_bin();
    let query = timeline.get_media_filter().trim().to_lowercase();
    let filter = timeline.get_media_group_filter();
    let entries = bin
        .entries()
        .iter()
        .filter(|entry| {
            (filter == -2
                || (filter == -1
                    && bin
                        .bins()
                        .iter()
                        .all(|group| !group.entry_ids.contains(&entry.id)))
                || bin
                    .bins()
                    .iter()
                    .any(|group| group.id == filter && group.entry_ids.contains(&entry.id)))
                && (query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.path.to_lowercase().contains(&query))
        })
        .map(|entry| {
            let group = bin
                .bins()
                .iter()
                .find(|group| group.entry_ids.contains(&entry.id));
            MediaBinEntryData {
                id: entry.id,
                path: SharedString::from(&entry.path),
                name: SharedString::from(&entry.name),
                // A stable key, not a label: the panel translates it, so the kind column reads
                // the same language as the rest of the window.
                kind: SharedString::from(match entry.kind {
                    aviqtl_app::media_import::MediaImportKind::Video => "video",
                    aviqtl_app::media_import::MediaImportKind::Audio => "audio",
                    aviqtl_app::media_import::MediaImportKind::Image => "image",
                }),
                group_id: group.map_or(-1, |group| group.id),
                group_name: SharedString::from(group.map_or("", |group| group.name.as_str())),
                group_index: group
                    .and_then(|group| {
                        bin.bins()
                            .iter()
                            .position(|candidate| candidate.id == group.id)
                    })
                    .map_or(-1, |index| index as i32),
                duration_frames: entry.duration_frames,
                in_frame: entry.in_frame,
                out_frame: entry.out_frame,
                selected: timeline.get_selected_media_id() == entry.id,
            }
        })
        .collect();
    update_vec_model(&timeline.get_media_entries(), entries);
    update_vec_model(
        &timeline.get_media_groups(),
        bin.bins()
            .iter()
            .map(|group| MediaBinGroupData {
                id: group.id,
                name: SharedString::from(&group.name),
            })
            .collect(),
    );
    // The filter combo shows two synthetic rows before the real groups. A removed group would
    // otherwise leave the box pointing at a stale index, so reset to "All media" when the saved
    // filter no longer exists.
    let filter_still_exists =
        filter == -2 || filter == -1 || bin.bins().iter().any(|group| group.id == filter);
    if !filter_still_exists {
        timeline.set_media_group_filter(-2);
        timeline.set_selected_media_id(-1);
    }
    // The filter combo shows two synthetic rows before the real groups, so the host owns the
    // whole list: a translated label cannot be produced from Slint's `@tr` for a row whose text
    // comes from Rust, and keeping the two halves in one place is what stops them disagreeing.
    let all_media = localized("All media", "全部媒体", "すべてのメディア");
    let ungrouped = localized("Ungrouped", "未分组", "未分類");
    let names = std::iter::once(all_media.to_owned())
        .chain(std::iter::once(ungrouped.to_owned()))
        .chain(bin.bins().iter().map(|group| group.name.clone()))
        .map(SharedString::from)
        .collect::<Vec<_>>();
    // Replaced wholesale rather than updated in place: a combo box caches the label it shows and
    // does not notice rows added to an existing model, so a fresh model is what makes the filter
    // box show its first entry instead of staying blank.
    timeline.set_media_group_names(ModelRc::from(Rc::new(VecModel::from(names))));
    // The combo's index is derived here rather than kept in the panel: an empty model at startup
    // leaves a widget holding index -1, and only the host knows which index the saved filter has.
    let group_index = if filter == -2 {
        0
    } else if filter == -1 {
        1
    } else {
        bin.bins()
            .iter()
            .position(|group| group.id == filter)
            .map_or(0, |index| index as i32 + 2)
    };
    timeline.set_media_group_index(group_index);
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
    let speed_percent = (workspace.playback_speed() * 100.0)
        .round()
        .clamp(10.0, 400.0) as i32;
    main.set_playback_speed_percent(speed_percent);
    timeline.set_selected_clip_count(workspace.selected_clip_count() as i32);
    main.set_playback_speed_multiplier(SharedString::from(speed_multiplier_text(speed_percent)));
    main.set_status_text(status_line(workspace.status(), workspace.status_message()));
    main.set_frame_counter_text(SharedString::from(frame_counter_text(
        workspace.playhead(),
        duration,
    )));
    timeline.set_playhead(workspace.playhead());
    timeline.set_duration(workspace.timeline_view_duration());
    timeline.set_selected_layer(workspace.selected_layer());
    if let Some(scene) = workspace.scene_settings(workspace.selected_scene()) {
        timeline.set_grid_mode(SharedString::from(scene.grid_mode));
        timeline.set_grid_fps(finite_f32(scene.fps, 60.0));
        timeline.set_grid_bpm(finite_f32(scene.grid_bpm, 120.0));
        timeline.set_grid_offset(finite_f32(scene.grid_offset, 0.0));
        timeline.set_grid_interval(scene.grid_interval.max(1));
        timeline.set_grid_subdivision(scene.grid_subdivision.max(1));
    }
}

/// Format the playback speed the way Qt's SpinBox does.
///
/// Qt renders the transport speed as a multiplier with one decimal, e.g. `1.0x`
/// (`MainWindow.qml:1143-1145`). Slint's `SpinBox` only edits an integer and has
/// no text formatter, so the box keeps the percent value and this multiplier is
/// projected alongside it.
pub(super) fn speed_multiplier_text(percent: i32) -> String {
    format!("{:.1}x", f64::from(percent) / 100.0)
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
