use std::{
    fs, io,
    path::Path,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::DatasetSpec;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RunMetadata {
    pub framework: String,
    pub framework_version: String,
    pub renderer: String,
    pub platform: String,
    pub rustc: String,
    pub profile: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameMetrics {
    pub frame_cpu: Duration,
    pub query_cpu: Duration,
    pub frame_interval: Option<Duration>,
    pub visible_clips: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Percentiles {
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub generated_unix_ms: u128,
    pub metadata: RunMetadata,
    pub dataset: DatasetSpec,
    pub total_frames: usize,
    pub warmup_frames: usize,
    pub measured_frames: usize,
    pub frame_cpu: Percentiles,
    pub query_cpu: Percentiles,
    pub frame_interval: Option<Percentiles>,
    pub frames_over_16_67ms: Option<usize>,
    pub average_visible_clips: f64,
    pub maximum_visible_clips: usize,
}

#[derive(Debug)]
pub struct MetricsCollector {
    metadata: RunMetadata,
    dataset: DatasetSpec,
    total_frames: usize,
    warmup_frames: usize,
    seen_frames: usize,
    frame_cpu_ns: Vec<u64>,
    query_cpu_ns: Vec<u64>,
    frame_interval_ns: Vec<u64>,
    visible_clips: Vec<usize>,
}

impl MetricsCollector {
    pub fn new(
        metadata: RunMetadata,
        dataset: DatasetSpec,
        total_frames: usize,
        warmup_frames: usize,
    ) -> Self {
        let measured_capacity = total_frames.saturating_sub(warmup_frames);
        Self {
            metadata,
            dataset,
            total_frames,
            warmup_frames,
            seen_frames: 0,
            frame_cpu_ns: Vec::with_capacity(measured_capacity),
            query_cpu_ns: Vec::with_capacity(measured_capacity),
            frame_interval_ns: Vec::with_capacity(measured_capacity),
            visible_clips: Vec::with_capacity(measured_capacity),
        }
    }

    pub fn record(&mut self, sample: FrameMetrics) {
        if self.seen_frames >= self.warmup_frames {
            self.frame_cpu_ns.push(duration_ns(sample.frame_cpu));
            self.query_cpu_ns.push(duration_ns(sample.query_cpu));
            if let Some(interval) = sample.frame_interval {
                self.frame_interval_ns.push(duration_ns(interval));
            }
            self.visible_clips.push(sample.visible_clips);
        }
        self.seen_frames += 1;
    }

    pub fn is_complete(&self) -> bool {
        self.seen_frames >= self.total_frames
    }

    pub fn finish(mut self) -> BenchmarkReport {
        assert!(self.is_complete(), "the benchmark run is incomplete");
        self.frame_cpu_ns.sort_unstable();
        self.query_cpu_ns.sort_unstable();
        self.frame_interval_ns.sort_unstable();
        let visible_total: usize = self.visible_clips.iter().sum();
        let average_visible_clips = if self.visible_clips.is_empty() {
            0.0
        } else {
            visible_total as f64 / self.visible_clips.len() as f64
        };

        BenchmarkReport {
            schema_version: 2,
            generated_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            metadata: self.metadata,
            dataset: self.dataset,
            total_frames: self.total_frames,
            warmup_frames: self.warmup_frames,
            measured_frames: self.frame_cpu_ns.len(),
            frame_cpu: percentiles(&self.frame_cpu_ns),
            query_cpu: percentiles(&self.query_cpu_ns),
            frame_interval: (!self.frame_interval_ns.is_empty())
                .then(|| percentiles(&self.frame_interval_ns)),
            frames_over_16_67ms: (!self.frame_interval_ns.is_empty()).then(|| {
                self.frame_interval_ns
                    .partition_point(|duration| *duration <= 16_670_000)
                    .abs_diff(self.frame_interval_ns.len())
            }),
            average_visible_clips,
            maximum_visible_clips: self.visible_clips.into_iter().max().unwrap_or(0),
        }
    }
}

pub fn write_report(path: &Path, report: &BenchmarkReport) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    fs::write(path, encoded)
}

pub fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unknown rustc".to_owned())
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn percentiles(sorted_ns: &[u64]) -> Percentiles {
    Percentiles {
        p50_us: percentile(sorted_ns, 0.50) / 1_000.0,
        p95_us: percentile(sorted_ns, 0.95) / 1_000.0,
        p99_us: percentile(sorted_ns, 0.99) / 1_000.0,
        max_us: sorted_ns.last().copied().unwrap_or(0) as f64 / 1_000.0,
    }
}

fn percentile(sorted_ns: &[u64], quantile: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let rank = (sorted_ns.len() as f64 * quantile.clamp(0.0, 1.0)).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted_ns.len() - 1);
    sorted_ns[index] as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> RunMetadata {
        RunMetadata {
            framework: "test".to_owned(),
            framework_version: "1".to_owned(),
            renderer: "headless".to_owned(),
            platform: "test".to_owned(),
            rustc: "rustc test".to_owned(),
            profile: "test".to_owned(),
        }
    }

    #[test]
    fn warmup_samples_are_excluded() {
        let mut collector = MetricsCollector::new(metadata(), DatasetSpec::default(), 4, 2);
        for micros in [100, 200, 300, 400] {
            collector.record(FrameMetrics {
                frame_cpu: Duration::from_micros(micros),
                query_cpu: Duration::from_micros(micros / 10),
                frame_interval: Some(Duration::from_micros(micros * 100)),
                visible_clips: micros as usize,
            });
        }
        let report = collector.finish();
        assert_eq!(report.measured_frames, 2);
        assert_eq!(report.frame_cpu.p50_us, 300.0);
        assert_eq!(report.frame_cpu.max_us, 400.0);
        assert_eq!(
            report.frame_interval.expect("frame interval").p50_us,
            30_000.0
        );
        assert_eq!(report.frames_over_16_67ms, Some(2));
        assert_eq!(report.average_visible_clips, 350.0);
    }
}
