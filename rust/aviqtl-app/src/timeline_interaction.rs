use aviqtl_rust_core::api::{
    ClipGeometryUpdate, ProjectDocument, plan_clip_delta_move, plan_clip_resize, snap_scene_frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineDragKind {
    Move,
    TrimStart,
    TrimEnd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineDragRequest {
    pub anchor_clip_id: i32,
    pub kind: TimelineDragKind,
    pub delta_pixels: (f32, f32),
    pub pixels_per_frame: f32,
    pub layer_height: f32,
    pub minimum_duration_frames: i32,
    pub ignore_snap: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineDragPlan {
    pub updates: Vec<ClipGeometryUpdate>,
    pub snap_frame: Option<i32>,
}

pub fn plan_timeline_drag(
    document: &ProjectDocument,
    scene_id: i32,
    selected_clip_ids: &[i32],
    request: TimelineDragRequest,
) -> Result<TimelineDragPlan, String> {
    let scene = document
        .scenes
        .iter()
        .find(|scene| scene.id == scene_id)
        .ok_or_else(|| format!("scene #{scene_id} does not exist"))?;
    let anchor = document
        .clips
        .iter()
        .find(|clip| clip.id == request.anchor_clip_id && clip.scene_id == scene_id)
        .ok_or_else(|| {
            format!(
                "clip #{} does not exist in scene #{scene_id}",
                request.anchor_clip_id
            )
        })?;
    let moving_ids = if selected_clip_ids.contains(&request.anchor_clip_id) {
        selected_clip_ids.to_vec()
    } else {
        vec![request.anchor_clip_id]
    };
    let delta_frames = f64::from(request.delta_pixels.0 / request.pixels_per_frame.max(0.05));
    let minimum_duration_frames = request.minimum_duration_frames.max(1);
    let updates = match request.kind {
        TimelineDragKind::Move => {
            let target = snap_scene_frame(
                f64::from(anchor.start) + delta_frames,
                request.ignore_snap,
                scene,
                f64::from(request.pixels_per_frame),
            );
            let delta_layer =
                (request.delta_pixels.1 / request.layer_height.max(1.0)).round() as i32;
            plan_clip_delta_move(
                document,
                scene_id,
                &moving_ids,
                delta_layer,
                target.saturating_sub(anchor.start),
            )
            .map_err(|error| error.to_string())?
        }
        TimelineDragKind::TrimStart => {
            let end = anchor.start.saturating_add(anchor.duration);
            let start = snap_scene_frame(
                f64::from(anchor.start) + delta_frames,
                request.ignore_snap,
                scene,
                f64::from(request.pixels_per_frame),
            )
            .min(end.saturating_sub(minimum_duration_frames));
            let delta_start = start.saturating_sub(anchor.start);
            plan_clip_resize(document, scene_id, &moving_ids, delta_start, -delta_start)
                .map_err(|error| error.to_string())?
        }
        TimelineDragKind::TrimEnd => {
            let original_end = anchor.start.saturating_add(anchor.duration);
            let end = snap_scene_frame(
                f64::from(original_end) + delta_frames,
                request.ignore_snap,
                scene,
                f64::from(request.pixels_per_frame),
            )
            .max(anchor.start.saturating_add(minimum_duration_frames));
            plan_clip_resize(
                document,
                scene_id,
                &moving_ids,
                0,
                end.saturating_sub(original_end),
            )
            .map_err(|error| error.to_string())?
        }
    };
    let anchor_update = updates
        .iter()
        .find(|update| update.clip_id == request.anchor_clip_id)
        .ok_or_else(|| "drag planner omitted the anchor clip".to_owned())?;
    let snap_frame = (!request.ignore_snap && scene.enable_snap).then_some(match request.kind {
        TimelineDragKind::TrimEnd => anchor_update.start.saturating_add(anchor_update.duration),
        TimelineDragKind::Move | TimelineDragKind::TrimStart => anchor_update.start,
    });
    Ok(TimelineDragPlan {
        updates,
        snap_frame,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::TimelineState;

    fn document() -> ProjectDocument {
        TimelineState::from_json(
            br#"{
                "version": 3,
                "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
                "scenes": [{"id": 1, "name": "Root", "duration": 300, "gridMode": "Frame", "gridInterval": 10}],
                "clips": [
                    {"id": 1, "sceneId": 1, "type": "rect", "start": 0, "duration": 20, "layer": 0},
                    {"id": 2, "sceneId": 1, "type": "rect", "start": 25, "duration": 20, "layer": 0},
                    {"id": 3, "sceneId": 1, "type": "rect", "start": 45, "duration": 20, "layer": 1}
                ]
            }"#,
        )
        .expect("drag fixture loads")
        .snapshot()
    }

    #[test]
    fn selected_clips_move_together_and_preserve_relative_layout() {
        let plan = plan_timeline_drag(
            &document(),
            1,
            &[1, 2],
            TimelineDragRequest {
                anchor_clip_id: 1,
                kind: TimelineDragKind::Move,
                delta_pixels: (5.0, 30.0),
                pixels_per_frame: 1.0,
                layer_height: 30.0,
                minimum_duration_frames: 5,
                ignore_snap: true,
            },
        )
        .expect("move plans");
        assert_eq!(plan.updates.len(), 2);
        assert_eq!(plan.updates[0].layer, 1);
        assert_eq!(plan.updates[1].start - plan.updates[0].start, 25);
    }

    #[test]
    fn move_uses_scene_grid_snapping_unless_shift_is_held() {
        let snapped = plan_timeline_drag(
            &document(),
            1,
            &[1],
            TimelineDragRequest {
                anchor_clip_id: 1,
                kind: TimelineDragKind::Move,
                delta_pixels: (6.0, 30.0),
                pixels_per_frame: 1.0,
                layer_height: 30.0,
                minimum_duration_frames: 5,
                ignore_snap: false,
            },
        )
        .expect("snapped move plans");
        assert_eq!(snapped.updates[0].start, 10);
        assert_eq!(snapped.snap_frame, Some(10));

        let unsnapped = plan_timeline_drag(
            &document(),
            1,
            &[1],
            TimelineDragRequest {
                anchor_clip_id: 1,
                kind: TimelineDragKind::Move,
                delta_pixels: (6.0, 30.0),
                pixels_per_frame: 1.0,
                layer_height: 30.0,
                minimum_duration_frames: 5,
                ignore_snap: true,
            },
        )
        .expect("unsnapped move plans");
        assert_eq!(unsnapped.updates[0].start, 6);
        assert_eq!(unsnapped.snap_frame, None);
    }

    #[test]
    fn trims_preserve_the_opposite_edge_and_minimum_duration() {
        let start = plan_timeline_drag(
            &document(),
            1,
            &[1],
            TimelineDragRequest {
                anchor_clip_id: 1,
                kind: TimelineDragKind::TrimStart,
                delta_pixels: (5.0, 0.0),
                pixels_per_frame: 1.0,
                layer_height: 30.0,
                minimum_duration_frames: 5,
                ignore_snap: true,
            },
        )
        .expect("start trim plans");
        assert_eq!(start.updates[0].start, 5);
        assert_eq!(start.updates[0].duration, 15);

        let end = plan_timeline_drag(
            &document(),
            1,
            &[1],
            TimelineDragRequest {
                anchor_clip_id: 1,
                kind: TimelineDragKind::TrimEnd,
                delta_pixels: (-50.0, 0.0),
                pixels_per_frame: 1.0,
                layer_height: 30.0,
                minimum_duration_frames: 8,
                ignore_snap: true,
            },
        )
        .expect("end trim plans");
        assert_eq!(end.updates[0].start, 0);
        assert_eq!(end.updates[0].duration, 8);
    }
}
