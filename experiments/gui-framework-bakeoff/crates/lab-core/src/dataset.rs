use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DatasetSpec {
    pub clip_count: usize,
    pub layer_count: usize,
}

impl Default for DatasetSpec {
    fn default() -> Self {
        Self {
            clip_count: 10_000,
            layer_count: 300,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub id: u32,
    pub layer: u32,
    pub start_frame: i64,
    pub duration_frames: u32,
    pub color: [u8; 4],
}

impl Clip {
    pub fn end_frame(&self) -> i64 {
        self.start_frame + i64::from(self.duration_frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub first_frame: f64,
    pub pixels_per_frame: f32,
    pub first_layer: usize,
    pub width_px: f32,
    pub height_px: f32,
    pub layer_height_px: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            first_frame: 0.0,
            pixels_per_frame: 2.0,
            first_layer: 0,
            width_px: 1280.0,
            height_px: 720.0,
            layer_height_px: 28.0,
        }
    }
}

impl Viewport {
    pub fn last_frame(self) -> f64 {
        self.first_frame + f64::from(self.width_px / self.pixels_per_frame.max(0.01))
    }

    pub fn visible_layer_count(self) -> usize {
        (self.height_px / self.layer_height_px.max(1.0)).ceil() as usize + 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibleClip {
    pub id: u32,
    pub layer: u32,
    pub x_px: f32,
    pub y_px: f32,
    pub width_px: f32,
    pub height_px: f32,
    pub color: [u8; 4],
}

#[derive(Debug)]
pub struct TimelineDataset {
    spec: DatasetSpec,
    layers: Vec<Vec<Clip>>,
    total_frames: i64,
}

impl TimelineDataset {
    pub fn synthetic(spec: DatasetSpec) -> Self {
        assert!(spec.layer_count > 0, "the dataset must contain a layer");
        let mut layers = vec![Vec::new(); spec.layer_count];
        let mut next_start = vec![0_i64; spec.layer_count];
        let mut rng = DeterministicRng::new(0x4156_4951_544c_3241);
        let mut total_frames = 0_i64;

        for index in 0..spec.clip_count {
            let layer = index % spec.layer_count;
            let gap = i64::from(rng.range(0, 18));
            let duration = rng.range(24, 360);
            let start_frame = next_start[layer] + gap;
            let clip = Clip {
                id: u32::try_from(index + 1).unwrap_or(u32::MAX),
                layer: u32::try_from(layer).unwrap_or(u32::MAX),
                start_frame,
                duration_frames: duration,
                color: color_for(index as u64),
            };
            next_start[layer] = clip.end_frame();
            total_frames = total_frames.max(clip.end_frame());
            layers[layer].push(clip);
        }

        Self {
            spec,
            layers,
            total_frames,
        }
    }

    pub fn spec(&self) -> DatasetSpec {
        self.spec
    }

    pub fn total_frames(&self) -> i64 {
        self.total_frames
    }

    pub fn query_visible(&self, viewport: Viewport, output: &mut Vec<VisibleClip>) {
        output.clear();
        if viewport.width_px <= 0.0
            || viewport.height_px <= 0.0
            || viewport.pixels_per_frame <= 0.0
            || viewport.layer_height_px <= 0.0
        {
            return;
        }

        let first_frame = viewport.first_frame.floor() as i64;
        let last_frame = viewport.last_frame().ceil() as i64;
        let last_layer = viewport
            .first_layer
            .saturating_add(viewport.visible_layer_count())
            .min(self.layers.len());

        for (visible_row, layer_index) in (viewport.first_layer..last_layer).enumerate() {
            let layer = &self.layers[layer_index];
            let first_index = layer.partition_point(|clip| clip.end_frame() <= first_frame);
            for clip in &layer[first_index..] {
                if clip.start_frame >= last_frame {
                    break;
                }
                let clipped_start = clip.start_frame.max(first_frame);
                let clipped_end = clip.end_frame().min(last_frame);
                let x_px = ((clipped_start as f64 - viewport.first_frame)
                    * f64::from(viewport.pixels_per_frame)) as f32;
                let width_px =
                    ((clipped_end - clipped_start) as f32 * viewport.pixels_per_frame).max(1.0);
                output.push(VisibleClip {
                    id: clip.id,
                    layer: clip.layer,
                    x_px,
                    y_px: visible_row as f32 * viewport.layer_height_px,
                    width_px,
                    height_px: (viewport.layer_height_px - 3.0).max(1.0),
                    color: clip.color,
                });
            }
        }
    }

    pub fn clip(&self, layer: usize, index: usize) -> Option<&Clip> {
        self.layers.get(layer).and_then(|clips| clips.get(index))
    }
}

fn color_for(index: u64) -> [u8; 4] {
    let mixed = index.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17);
    [
        70 + (mixed as u8 % 150),
        70 + ((mixed >> 8) as u8 % 150),
        70 + ((mixed >> 16) as u8 % 150),
        255,
    ]
}

struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    fn range(&mut self, start: u32, end: u32) -> u32 {
        debug_assert!(start < end);
        start + self.next() % (end - start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_dataset_is_deterministic_and_sorted() {
        let first = TimelineDataset::synthetic(DatasetSpec {
            clip_count: 1_000,
            layer_count: 30,
        });
        let second = TimelineDataset::synthetic(first.spec());
        assert_eq!(first.total_frames(), second.total_frames());
        for layer in 0..30 {
            for index in 0..33 {
                assert_eq!(first.clip(layer, index), second.clip(layer, index));
            }
            let clips = &first.layers[layer];
            assert!(
                clips
                    .windows(2)
                    .all(|pair| pair[0].end_frame() <= pair[1].start_frame)
            );
        }
    }

    #[test]
    fn query_only_returns_visible_layers_and_frames() {
        let dataset = TimelineDataset::synthetic(DatasetSpec {
            clip_count: 1_000,
            layer_count: 30,
        });
        let viewport = Viewport {
            first_frame: 100.0,
            pixels_per_frame: 1.0,
            first_layer: 5,
            width_px: 500.0,
            height_px: 84.0,
            layer_height_px: 28.0,
        };
        let mut visible = Vec::new();
        dataset.query_visible(viewport, &mut visible);
        assert!(!visible.is_empty());
        assert!(
            visible
                .iter()
                .all(|clip| (5..=8).contains(&(clip.layer as usize)))
        );
        assert!(visible.iter().all(|clip| clip.x_px < viewport.width_px));
        assert!(visible.iter().all(|clip| clip.x_px + clip.width_px > 0.0));
    }

    #[test]
    fn invalid_viewport_produces_no_items() {
        let dataset = TimelineDataset::synthetic(DatasetSpec::default());
        let mut visible = vec![VisibleClip {
            id: 1,
            layer: 0,
            x_px: 0.0,
            y_px: 0.0,
            width_px: 1.0,
            height_px: 1.0,
            color: [0; 4],
        }];
        dataset.query_visible(
            Viewport {
                width_px: 0.0,
                ..Viewport::default()
            },
            &mut visible,
        );
        assert!(visible.is_empty());
    }
}
