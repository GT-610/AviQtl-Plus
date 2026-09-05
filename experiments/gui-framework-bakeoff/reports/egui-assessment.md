# egui 0.36.1 assessment

Environment: macOS arm64, Rust 1.98.0 Homebrew stable, eframe wgpu 30 backend, release profile.

## Implemented slice

- resizable project, preview, inspector, and timeline panels;
- viewport-only drawing for the shared 10,000-clip / 300-layer dataset;
- ruler, layer grid, clip labels, selection, scripted zoom/pan/scroll, and playhead;
- dynamic inspector controls and CJK text-edit probes;
- optional native auxiliary preview window;
- application-created `wgpu::Texture` registered as an egui texture without a GPU-to-CPU readback.

The adapter and shared workload both use `#![forbid(unsafe_code)]`. Framework types remain inside
the adapter. The authoritative dataset and workload contain no egui state.

## Automated result

The 2,400-frame run completed and closed normally. Of 2,160 measured frames:

| Metric | Result |
| --- | ---: |
| UI prepare CPU p50 / p95 / p99 | 453.333 / 603.709 / 829.917 us |
| UI prepare CPU maximum | 2,191.708 us |
| Visible query CPU p95 | 14.708 us |
| Frame interval p50 / p95 / p99 | 16.658 / 16.938 / 17.153 ms |
| Frames over 16.67 ms | 975 |
| Release binary | 11,027,848 bytes |
| Peak resident set (180-frame resource run) | 165,609,472 bytes |
| Process CPU in 3.14 s resource run | 0.29 s user + 0.13 s system |

The exact machine-readable result is in `egui-macos.json`. The corrected full-width timeline run
was paced at the display's 60 Hz refresh rate. Counts just above 16.67 ms primarily represent
normal scheduling jitter; p95 and maximum intervals are the more useful tail-latency signals.

## Current evidence

- External GPU texture integration: passes. The preview is owned by adapter-side wgpu code and is
  sampled by egui directly; no readback path is required.
- Visible-range scaling: passes. Only queried clips are converted to egui paint primitives.
- Surface recovery: upstream `egui-wgpu` marks suboptimal surfaces for reconfiguration and handles
  reconfigure/recreate/skip actions for unsuccessful surface acquisition.
- CJK glyph path: implemented with a macOS system fallback font. Interactive IME behavior still
  requires a manual input pass before the final recommendation.
- Multi-window path: implemented through an immediate native viewport, disabled during benchmarks
  so it cannot distort the main-window result.
