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
| Model update CPU p50 / p95 / p99 | 55.000 / 78.875 / 120.416 us |
| Model update CPU maximum | 179.000 us |
| Visible query CPU p95 | 2.084 us |
| Frame interval p50 / p95 / p99 | 16.671 / 16.973 / 17.147 ms |
| Frame interval maximum | 21.330 ms |
| Frames over 16.67 ms | 1,083 |
| Release binary | 16,546,712 bytes |
| Peak resident set (180-frame resource run) | 125,157,376 bytes |
| Process CPU in 3.21 s resource run | 1.27 s user + 0.25 s system |

The exact machine-readable result is in `slint-macos.json`. Model-update CPU excludes Slint's
retained-scene layout and rendering, while egui's prepare CPU includes rebuilding paint primitives;
process CPU and presented-frame intervals are therefore the fairer cross-framework signals.

## Current evidence

- External GPU texture integration: passes without readback, but the public entry point is behind
  `unstable-wgpu-29` and trails the experiment's egui/wgpu 30 stack by one wgpu major version.
- Visible-range scaling: passes. Only the shared query result is installed in the `VecModel`.
- Surface recovery: the current FemtoVG-wgpu renderer retries `Outdated`, `Suboptimal`, and `Lost`
  surface acquisition after reconfiguration, and skips occluded or timed-out frames.
- CJK/IME path: system-font text and line-edit controls are implemented; composition still needs a
  manual input pass before the final recommendation.
- Multi-window path: a second native Slint `Window` shares the imported preview image and is disabled
  during automated measurements.
