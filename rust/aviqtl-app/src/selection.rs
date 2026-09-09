use aviqtl_rust_core::api::{ClipDocument, MAX_TIMELINE_LAYER, ProjectDocument};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipSelection {
    ids: Vec<i32>,
    primary: Option<i32>,
    preview_ids: Vec<i32>,
    selected_layer: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionBox {
    pub frame_a: i32,
    pub frame_b: i32,
    pub layer_a: i32,
    pub layer_b: i32,
    pub additive: bool,
}

impl ClipSelection {
    pub fn ids(&self) -> &[i32] {
        &self.ids
    }

    pub fn primary(&self) -> Option<i32> {
        self.primary
    }

    pub fn selected_layer(&self) -> i32 {
        self.selected_layer
    }

    pub fn set_selected_layer(&mut self, layer: i32) {
        self.selected_layer = layer.clamp(0, MAX_TIMELINE_LAYER);
    }

    pub fn is_selected(&self, clip_id: i32) -> bool {
        self.ids.contains(&clip_id)
    }

    pub fn is_visually_selected(&self, clip_id: i32) -> bool {
        if self.preview_ids.is_empty() {
            self.is_selected(clip_id)
        } else {
            self.preview_ids.contains(&clip_id)
        }
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.primary = None;
        self.preview_ids.clear();
    }

    pub fn replace(&mut self, ids: impl IntoIterator<Item = i32>) {
        self.ids = unique_non_negative(ids);
        self.primary = self.ids.first().copied();
        self.preview_ids.clear();
    }

    pub fn click_clip(&mut self, clip_id: i32, control: bool) {
        if clip_id < 0 {
            self.clear();
            return;
        }
        if !control {
            self.replace([clip_id]);
            return;
        }
        if let Some(index) = self.ids.iter().position(|selected| *selected == clip_id) {
            self.ids.remove(index);
            self.primary = self.ids.first().copied();
        } else {
            // Qt's TimelineService prepends a Ctrl-clicked clip before applying the selection,
            // making it both the clipboard-first item and the primary settings object.
            self.ids.insert(0, clip_id);
            self.primary = Some(clip_id);
        }
        self.preview_ids.clear();
    }

    pub fn preview_box(
        &mut self,
        document: &ProjectDocument,
        scene_id: i32,
        selection_box: SelectionBox,
    ) {
        let min_frame = selection_box.frame_a.min(selection_box.frame_b);
        let max_frame = selection_box.frame_a.max(selection_box.frame_b);
        let min_layer = selection_box.layer_a.min(selection_box.layer_b);
        let max_layer = selection_box.layer_a.max(selection_box.layer_b);
        let mut ids = if selection_box.additive {
            self.ids.clone()
        } else {
            Vec::new()
        };
        for clip in document
            .clips
            .iter()
            .filter(|clip| clip.scene_id == scene_id)
        {
            let clip_end = clip.start.saturating_add(clip.duration);
            let clip_last_layer = clip.layer.saturating_add(control_layer_count(clip));
            if clip.start < max_frame
                && min_frame < clip_end
                && clip_last_layer >= min_layer
                && clip.layer <= max_layer
                && !ids.contains(&clip.id)
            {
                ids.push(clip.id);
            }
        }
        self.preview_ids = ids;
    }

    pub fn finish_preview(&mut self) {
        let preview = std::mem::take(&mut self.preview_ids);
        self.replace(preview);
    }

    pub fn cancel_preview(&mut self) {
        self.preview_ids.clear();
    }

    pub fn reconcile(&mut self, document: &ProjectDocument, scene_id: i32) {
        let valid = document
            .clips
            .iter()
            .filter(|clip| clip.scene_id == scene_id)
            .map(|clip| clip.id)
            .collect::<BTreeSet<_>>();
        self.ids.retain(|clip_id| valid.contains(clip_id));
        self.preview_ids.retain(|clip_id| valid.contains(clip_id));
        if self
            .primary
            .is_none_or(|primary| !self.ids.contains(&primary))
        {
            self.primary = self.ids.first().copied();
        }
    }
}

fn unique_non_negative(ids: impl IntoIterator<Item = i32>) -> Vec<i32> {
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| *id >= 0 && seen.insert(*id))
        .collect()
}

fn control_layer_count(clip: &ClipDocument) -> i32 {
    clip.effects
        .iter()
        .find(|effect| {
            matches!(
                effect.id.as_str(),
                "GroupControl" | "camera_control" | "camera"
            )
        })
        .and_then(|effect| effect.params.get("layerCount"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|layers| i32::try_from(layers).ok())
        .or_else(|| {
            clip.extra
                .get("groupLayerCount")
                .and_then(serde_json::Value::as_i64)
                .and_then(|layers| i32::try_from(layers).ok())
        })
        .unwrap_or_default()
        .max(0)
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
                "scenes": [{"id": 1, "name": "Root", "duration": 300}],
                "clips": [
                    {"id": 1, "sceneId": 1, "type": "rect", "start": 0, "duration": 20, "layer": 0},
                    {"id": 2, "sceneId": 1, "type": "rect", "start": 10, "duration": 20, "layer": 2,
                     "effects": [{"id": "GroupControl", "name": "Group", "enabled": true,
                                  "params": {"layerCount": 2}}]},
                    {"id": 3, "sceneId": 1, "type": "rect", "start": 40, "duration": 10, "layer": 6}
                ]
            }"#,
        )
        .expect("selection fixture loads")
        .snapshot()
    }

    #[test]
    fn ctrl_click_matches_qt_primary_and_clipboard_order() {
        let mut selection = ClipSelection::default();
        selection.click_clip(1, false);
        selection.click_clip(2, true);
        assert_eq!(selection.ids(), [2, 1]);
        assert_eq!(selection.primary(), Some(2));

        selection.click_clip(2, true);
        assert_eq!(selection.ids(), [1]);
        assert_eq!(selection.primary(), Some(1));
    }

    #[test]
    fn right_button_box_selection_uses_half_open_frames_and_control_layers() {
        let document = document();
        let mut selection = ClipSelection::default();
        selection.preview_box(
            &document,
            1,
            SelectionBox {
                frame_a: 19,
                frame_b: 21,
                layer_a: 0,
                layer_b: 1,
                additive: false,
            },
        );
        assert_eq!(selection.preview_ids, vec![1]);

        selection.preview_box(
            &document,
            1,
            SelectionBox {
                frame_a: 19,
                frame_b: 21,
                layer_a: 4,
                layer_b: 4,
                additive: false,
            },
        );
        assert_eq!(selection.preview_ids, vec![2]);

        selection.finish_preview();
        assert_eq!(selection.ids(), [2]);
        assert_eq!(selection.primary(), Some(2));
    }

    #[test]
    fn additive_box_keeps_existing_order_before_new_hits() {
        let document = document();
        let mut selection = ClipSelection::default();
        selection.replace([3]);
        selection.preview_box(
            &document,
            1,
            SelectionBox {
                frame_a: 0,
                frame_b: 31,
                layer_a: 0,
                layer_b: 4,
                additive: true,
            },
        );
        selection.finish_preview();
        assert_eq!(selection.ids(), [3, 1, 2]);
        assert_eq!(selection.primary(), Some(3));
    }

    #[test]
    fn reconcile_drops_clips_from_other_scenes_and_promotes_the_first_survivor() {
        let mut document = document();
        let mut selection = ClipSelection::default();
        selection.replace([2, 1, 99]);
        document.clips.retain(|clip| clip.id != 2);
        selection.reconcile(&document, 1);
        assert_eq!(selection.ids(), [1]);
        assert_eq!(selection.primary(), Some(1));
    }
}
