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
| UI prepare CPU p50 / p95 / p99 | 453.042 / 590.709 / 734.083 us |
| UI prepare CPU maximum | 2,503.250 us |
| Visible query CPU p95 | 13.917 us |
| Frame interval p50 / p95 / p99 | 16.659 / 16.945 / 17.148 ms |
| Frame interval maximum | 17.584 ms |
| Frames over 16.67 ms | 963 |
| Standalone release binary | 11,027,848 bytes |
| Peak resident set | 166,428,672 bytes |
| Process CPU in 40.80 s | 4.27 s user + 2.16 s system |

The exact machine-readable result is in `egui-macos.json`. eframe's continuous repaint path followed
the unlocked built-in display's 60 Hz presentation rate without an application-side frame timer.
Three additional 180-frame runs closed normally in 2.84-3.16 seconds, with frame-interval p95 of
16.902-17.027 ms. Two short samples contained isolated 21.779 ms and 32.345 ms maximum intervals;
the full 2,160-frame measured sample did not reproduce them. Counts just above 16.67 ms primarily
represent normal scheduling jitter; p95 and p99 are more useful than a single short-run maximum.

## Current evidence

- External GPU texture integration: passes. The preview is owned by adapter-side wgpu code and is
  sampled by egui directly; no readback path is required.
- Visible-range scaling: passes. Only queried clips are converted to egui paint primitives.
- Surface recovery: upstream `egui-wgpu` marks suboptimal surfaces for reconfiguration and handles
  reconfigure/recreate/skip actions for unsuccessful surface acquisition.
- CJK glyph path: implemented with a macOS system fallback font. Live IME composition was not
  manually verified in this command-line experiment.
- Multi-window path: implemented through an immediate native viewport, disabled during benchmarks
  so it cannot distort the main-window result.
