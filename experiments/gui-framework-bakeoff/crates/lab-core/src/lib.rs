#![forbid(unsafe_code)]

mod dataset;
mod metrics;
mod workload;

pub use dataset::{Clip, DatasetSpec, TimelineDataset, Viewport, VisibleClip};
pub use metrics::{
    BenchmarkReport, FrameMetrics, MetricsCollector, RunMetadata, rustc_version, write_report,
};
pub use workload::{BenchmarkConfig, ScriptedWorkload};
