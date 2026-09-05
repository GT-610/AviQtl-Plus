#![forbid(unsafe_code)]

use std::{env, hint::black_box, time::Instant};

use aviqtl_gui_lab_core::{
    BenchmarkConfig, FrameMetrics, MetricsCollector, RunMetadata, ScriptedWorkload,
    TimelineDataset, rustc_version, write_report,
};

fn main() {
    let config = BenchmarkConfig::parse(env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let dataset = TimelineDataset::synthetic(config.dataset);
    let mut workload = ScriptedWorkload::new(dataset.total_frames(), config.dataset.layer_count);
    let mut visible = Vec::with_capacity(512);
    let mut metrics = MetricsCollector::new(
        RunMetadata {
            framework: "shared-headless-baseline".to_owned(),
            framework_version: env!("CARGO_PKG_VERSION").to_owned(),
            renderer: "none".to_owned(),
            platform: format!("{}-{}", env::consts::OS, env::consts::ARCH),
            rustc: rustc_version(),
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .to_owned(),
        },
        config.dataset,
        config.frames,
        config.warmup_frames,
    );

    while !metrics.is_complete() {
        let frame_start = Instant::now();
        workload.advance();
        let query_start = Instant::now();
        dataset.query_visible(workload.viewport(), &mut visible);
        let query_cpu = query_start.elapsed();
        black_box(&visible);
        metrics.record(FrameMetrics {
            frame_cpu: frame_start.elapsed(),
            query_cpu,
            frame_interval: None,
            visible_clips: visible.len(),
        });
    }

    let report = metrics.finish();
    if let Some(path) = config.report_path.as_deref() {
        write_report(path, &report).unwrap_or_else(|error| {
            eprintln!("failed to write {}: {error}", path.display());
            std::process::exit(1);
        });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialize benchmark report")
    );
}
