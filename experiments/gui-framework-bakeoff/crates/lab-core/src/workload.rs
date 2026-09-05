use std::path::PathBuf;

use crate::{DatasetSpec, Viewport};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkConfig {
    pub frames: usize,
    pub warmup_frames: usize,
    pub dataset: DatasetSpec,
    pub report_path: Option<PathBuf>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            frames: 2_400,
            warmup_frames: 240,
            dataset: DatasetSpec::default(),
            report_path: None,
        }
    }
}

impl BenchmarkConfig {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut config = Self::default();
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--frames" => config.frames = parse_usize("--frames", args.next())?,
                "--warmup" => {
                    config.warmup_frames = parse_usize("--warmup", args.next())?;
                }
                "--clips" => {
                    config.dataset.clip_count = parse_usize("--clips", args.next())?;
                }
                "--layers" => {
                    config.dataset.layer_count = parse_usize("--layers", args.next())?;
                }
                "--report" => {
                    let path = args
                        .next()
                        .ok_or_else(|| "--report requires a path".to_owned())?;
                    config.report_path = Some(PathBuf::from(path));
                }
                "--help" | "-h" => return Err(Self::usage().to_owned()),
                unknown => return Err(format!("unknown argument: {unknown}\n{}", Self::usage())),
            }
        }

        if config.frames == 0 {
            return Err("--frames must be greater than zero".to_owned());
        }
        if config.warmup_frames >= config.frames {
            return Err("--warmup must be smaller than --frames".to_owned());
        }
        if config.dataset.clip_count == 0 || config.dataset.layer_count == 0 {
            return Err("--clips and --layers must be greater than zero".to_owned());
        }
        Ok(config)
    }

    pub const fn usage() -> &'static str {
        "usage: --frames N --warmup N --clips N --layers N [--report PATH]"
    }
}

fn parse_usize(flag: &str, value: Option<String>) -> Result<usize, String> {
    value
        .ok_or_else(|| format!("{flag} requires a value"))?
        .parse::<usize>()
        .map_err(|_| format!("{flag} requires a positive integer"))
}

#[derive(Clone, Debug)]
pub struct ScriptedWorkload {
    frame_index: usize,
    total_frames: i64,
    layer_count: usize,
    viewport: Viewport,
    playhead_frame: f64,
}

impl ScriptedWorkload {
    pub fn new(total_frames: i64, layer_count: usize) -> Self {
        Self {
            frame_index: 0,
            total_frames,
            layer_count,
            viewport: Viewport::default(),
            playhead_frame: 0.0,
        }
    }

    pub fn resize(&mut self, width_px: f32, height_px: f32) {
        self.viewport.width_px = width_px.max(1.0);
        self.viewport.height_px = height_px.max(1.0);
    }

    pub fn advance(&mut self) {
        let cycle = self.frame_index % 1_200;
        let phase = cycle as f64 / 1_200.0;
        self.playhead_frame = (self.playhead_frame + 1.0) % self.total_frames.max(1) as f64;

        self.viewport.pixels_per_frame = if cycle < 300 {
            0.35 + cycle as f32 / 300.0 * 3.65
        } else if cycle < 600 {
            4.0 - (cycle - 300) as f32 / 300.0 * 3.65
        } else {
            1.25
        };

        let visible_frames = self.viewport.last_frame() - self.viewport.first_frame;
        let max_first_frame = (self.total_frames as f64 - visible_frames).max(0.0);
        self.viewport.first_frame = if cycle < 600 {
            phase * max_first_frame
        } else {
            self.playhead_frame.min(max_first_frame)
        };

        let visible_layers = self.viewport.visible_layer_count();
        let max_first_layer = self.layer_count.saturating_sub(visible_layers);
        self.viewport.first_layer = if max_first_layer == 0 {
            0
        } else {
            (cycle * 3 / 5) % (max_first_layer + 1)
        };
        self.frame_index += 1;
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn playhead_frame(&self) -> f64 {
        self.playhead_frame
    }

    pub fn frame_index(&self) -> usize {
        self.frame_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_explicit_configuration() {
        let config = BenchmarkConfig::parse(
            [
                "--frames",
                "900",
                "--warmup",
                "90",
                "--clips",
                "12000",
                "--layers",
                "400",
                "--report",
                "result.json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .expect("valid configuration");
        assert_eq!(config.frames, 900);
        assert_eq!(config.warmup_frames, 90);
        assert_eq!(config.dataset.clip_count, 12_000);
        assert_eq!(config.dataset.layer_count, 400);
        assert_eq!(config.report_path, Some(PathBuf::from("result.json")));
    }

    #[test]
    fn workload_stays_inside_dataset_bounds() {
        let mut workload = ScriptedWorkload::new(5_000, 300);
        for _ in 0..5_000 {
            workload.advance();
            let viewport = workload.viewport();
            assert!(viewport.first_frame >= 0.0);
            assert!(viewport.last_frame() <= 5_001.0);
            assert!(viewport.first_layer < 300);
        }
    }
}
