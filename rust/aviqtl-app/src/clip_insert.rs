//! Placement rules shared by everything that inserts clips from outside the project.
//!
//! Object-file and fragment imports both add a block of clips that were authored somewhere
//! else, and both need the same answer to the same question: what happens when the block lands
//! on frames that already hold objects? Laying it on top would leave two clips sharing a layer
//! and a frame, which the renderer resolves by stacking order rather than by intent.

use aviqtl_rust_core::api::{ClipDocument, ProjectDocument};

/// Moves `clips` forward as one block until none of them overlaps a clip already in the scene.
///
/// The block keeps its internal layout, the way a drag-move of a selection does: only the whole
/// set moves, never one clip on its own. Returns how many frames it moved, so the caller can
/// say so in its report.
pub fn shift_past_existing(
    document: &ProjectDocument,
    scene_id: i32,
    clips: &mut [ClipDocument],
) -> i32 {
    if clips.is_empty() {
        return 0;
    }
    let mut total_shift: i32 = 0;
    // A shift can expose a new collision further along, so repeat until the block is clear.
    // Each pass moves the block past at least one existing clip, so the number of clips in
    // the scene bounds the passes; the fixed 64 it used to be was reachable, and a layer
    // with more than 64 objects left the import overlapping.
    let passes = document
        .clips
        .iter()
        .filter(|existing| existing.scene_id == scene_id)
        .count()
        .saturating_add(1);
    for _ in 0..passes {
        let mut shift = 0;
        for clip in clips.iter() {
            let clip_end = clip.start.saturating_add(clip.duration);
            for existing in document.clips.iter().filter(|existing| {
                existing.scene_id == scene_id
                    && existing.layer == clip.layer
                    && existing.id != clip.id
            }) {
                let existing_end = existing.start.saturating_add(existing.duration);
                if clip.start < existing_end && existing.start < clip_end {
                    shift = shift.max(existing_end.saturating_sub(clip.start));
                }
            }
        }
        if shift == 0 {
            break;
        }
        for clip in clips.iter_mut() {
            clip.start = clip.start.saturating_add(shift);
        }
        total_shift = total_shift.saturating_add(shift);
    }
    total_shift
}

/// The note an import appends when [`shift_past_existing`] moved its block.
pub fn shift_note(frames: i32) -> String {
    format!("moved the import {frames} frame(s) forward to avoid overlapping existing objects")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::{ProjectSettings, SceneDocument};
    use serde_json::Map;

    fn clip(id: i32, start: i32, duration: i32, layer: i32) -> ClipDocument {
        ClipDocument {
            id,
            scene_id: 1,
            clip_type: "text".to_owned(),
            start,
            duration,
            layer,
            clip_by_upper_object: false,
            params: Map::new(),
            audio_plugins: Vec::new(),
            effects: Vec::new(),
            extra: Default::default(),
        }
    }

    fn project(clips: Vec<ClipDocument>) -> ProjectDocument {
        ProjectDocument {
            version: 3,
            settings: ProjectSettings {
                width: 1920,
                height: 1080,
                fps: 60.0,
                sample_rate: 48_000,
                extra: Default::default(),
            },
            scenes: vec![SceneDocument {
                id: 1,
                name: "Root".to_owned(),
                width: 1920,
                height: 1080,
                fps: 60.0,
                start: 0,
                duration: 600,
                nested_duration: 0,
                locked_layers: Vec::new(),
                hidden_layers: Vec::new(),
                grid_mode: "none".to_owned(),
                grid_bpm: 120.0,
                grid_offset: 0.0,
                grid_interval: 10,
                grid_subdivision: 4,
                enable_snap: true,
                magnetic_snap_range: 10,
                extra: Default::default(),
            }],
            clips,
            extra: Default::default(),
        }
    }

    #[test]
    fn free_space_leaves_the_block_alone() {
        let document = project(vec![clip(1, 100, 50, 0)]);
        let mut block = vec![clip(2, 0, 40, 0)];
        assert_eq!(shift_past_existing(&document, 1, &mut block), 0);
        assert_eq!(block[0].start, 0);
    }

    #[test]
    fn a_collision_moves_the_whole_block_by_the_same_amount() {
        let document = project(vec![clip(1, 0, 30, 0)]);
        // The second clip is on another layer, so only the first one collides; both still have
        // to move, or the block would be sheared apart.
        let mut block = vec![clip(2, 20, 10, 0), clip(3, 25, 10, 1)];
        let shift = shift_past_existing(&document, 1, &mut block);
        assert_eq!(
            shift, 10,
            "the obstacle ends at frame 30, the block starts at 20"
        );
        assert_eq!(block[0].start, 30);
        assert_eq!(block[1].start, 35, "relative layout is preserved");
    }

    #[test]
    fn a_clip_on_another_layer_or_scene_does_not_block() {
        let document = project(vec![clip(1, 20, 30, 5)]);
        let mut block = vec![clip(2, 0, 40, 0)];
        assert_eq!(shift_past_existing(&document, 1, &mut block), 0);
        assert_eq!(shift_past_existing(&document, 7, &mut block), 0);
    }

    #[test]
    fn a_chain_of_obstacles_is_cleared_in_one_call() {
        // 0..10 and 10..20 back to back: a single jump to 10 would land on the second one.
        let document = project(vec![clip(1, 0, 10, 0), clip(2, 10, 10, 0)]);
        let mut block = vec![clip(3, 0, 30, 0)];
        let shift = shift_past_existing(&document, 1, &mut block);
        assert_eq!(shift, 20);
        assert_eq!(block[0].start, 20);
    }

    #[test]
    fn more_obstacles_than_the_old_pass_limit_are_all_cleared() {
        // The loop used to give up after 64 passes. Each pass can only clear the obstacles
        // the block is touching, so a layer holding more than 64 one-frame-apart objects
        // left the import sitting on top of the ones it never got to.
        let mut obstacles = Vec::new();
        for index in 0..200 {
            obstacles.push(clip(index + 1, index, 1, 0));
        }
        let document = project(obstacles);
        let mut block = vec![clip(1_000, 0, 10, 0)];
        let shift = shift_past_existing(&document, 1, &mut block);
        assert_eq!(shift, 200, "the block ends up past the last obstacle");
        assert_eq!(block[0].start, 200);
    }

    #[test]
    fn a_clip_does_not_collide_with_itself() {
        let block = vec![clip(1, 0, 40, 0)];
        let mut moving = block.clone();
        let document = project(block);
        assert_eq!(shift_past_existing(&document, 1, &mut moving), 0);
    }
}
