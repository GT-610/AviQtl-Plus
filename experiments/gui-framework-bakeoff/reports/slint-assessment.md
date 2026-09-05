# Slint 1.17.1 assessment

Environment: macOS arm64, Rust 1.98.0 Homebrew stable, FemtoVG-wgpu 29 renderer, release profile.

## Implemented slice

- project, imported-texture preview, dynamic inspector, and full-width timeline regions;
- viewport-only declarative rectangles for the shared 10,000-clip / 300-layer dataset;
- clip selection, scripted zoom/pan/scroll, playhead, CJK line edits, and effect-property updates;
- optional second native preview window;
- an application-created `wgpu::Texture`, allocated from Slint's own `Device` and `Queue`, imported
  into the scene as a `slint::Image` without a GPU-to-CPU readback.

Framework types remain inside the adapter and the authoritative shared model remains unchanged.
Handwritten adapter code uses `#![deny(unsafe_code)]`. It cannot use `forbid` because the `slint!`
macro's generated vtable glue contains its own `allow(unsafe_code)` boundary.

## Automated result

The 2,400-frame run completed and closed normally. Of 2,160 measured frames:

| Metric | Result |
| --- | ---: |
| Model update CPU p50 / p95 / p99 | 55.166 / 80.709 / 122.042 us |
| Model update CPU maximum | 167.833 us |
| Visible query CPU p95 | 2.042 us |
| Frame interval p50 / p95 / p99 | 16.673 / 17.085 / 17.316 ms |
| Frame interval maximum | 20.989 ms |
| Frames over 16.67 ms | 1,100 |
| Standalone release binary | 16,548,648 bytes |
| Peak resident set | 127,172,608 bytes |
| Process CPU in 40.27 s | 15.18 s user + 2.62 s system |

The exact machine-readable result is in `slint-macos.json`. Model-update CPU excludes Slint's
retained-scene layout and rendering, while egui's prepare CPU includes rebuilding paint primitives;
process CPU and presented-frame intervals are therefore the fairer cross-framework signals. The
adapter starts each model update only after the prior frame has been presented, using a zero-delay
single-shot event-loop task rather than a fixed-rate timer. Two additional 180-frame runs both
closed normally in 3.20 seconds, with frame-interval p95 of 17.010-17.066 ms.

## Current evidence

- External GPU texture integration: passes without readback, but the public entry point is behind
  `unstable-wgpu-29` and trails the experiment's egui/wgpu 30 stack by one wgpu major version.
- Visible-range scaling: passes. Only the shared query result is installed in the `VecModel`.
- Surface recovery: the current FemtoVG-wgpu renderer retries `Outdated`, `Suboptimal`, and `Lost`
  surface acquisition after reconfiguration, and skips occluded or timed-out frames.
- CJK/IME path: system-font text and line-edit controls are implemented; live composition was not
  manually verified in this command-line experiment.
- Multi-window path: a second native Slint `Window` shares the imported preview image and is disabled
  during automated measurements.
