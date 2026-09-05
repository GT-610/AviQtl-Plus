# AviQtl GUI framework bake-off

This local-only experiment compares egui, Slint, and GPUI-CE with a workload that resembles the
future AviUtl2-style editor. It is deliberately separate from the production Rust workspace so the
framework dependency graphs and experimental toolchain requirements do not affect AviQtl releases.

The comparison prioritizes runtime performance, stability, and safety. Implementation effort and
source-code size are recorded for context but do not contribute to the selection score.

## Shared workload

- 300 timeline layers and 10,000 deterministic clips;
- viewport-only clip queries with horizontal pan, zoom, vertical scroll, and playback;
- a preview area and a dynamic effect inspector in each GUI implementation;
- scripted runs that close automatically and emit comparable JSON reports;
- interactive runs for IME, focus, accessibility, docking, and multi-window checks.

The GUI implementations must use `aviqtl-gui-lab-core` unchanged. Framework-specific state must not
leak into the dataset, workload, or metrics model.

## Commit plan

1. Add the shared workload, metrics schema, headless baseline, and experiment protocol.
2. Add the egui implementation and its measured report.
3. Add the Slint implementation and its measured report.
4. Add the GPUI-CE implementation and its measured report.
5. Add stability/safety checks, the final comparison report, and the recommendation.

Each framework commit must remain independently revertible. This branch is not intended to be
pushed or merged as the production UI migration.

## Toolchain

Use the currently installed stable Rust toolchain only. The initial experiment environment uses
`rustc 1.98.0`; no additional Rust versions are downloaded for the bake-off.

## Baseline

Run the framework-independent query/update baseline in release mode:

```sh
cargo run --release --manifest-path experiments/gui-framework-bakeoff/Cargo.toml \
  -p aviqtl-gui-lab-driver -- --frames 2400 --warmup 240 \
  --report experiments/gui-framework-bakeoff/reports/headless-macos.json
```

Reports contain frame CPU percentiles, query percentiles, visible-item counts, the dataset shape,
and environment metadata. GPU timing and resident-memory sampling are added by GUI adapters where
the framework exposes reliable hooks.

