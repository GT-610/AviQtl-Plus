# GPUI-CE 0.2.2 assessment

Environment: macOS arm64, Rust 1.98.0 Homebrew stable, native Metal renderer, release profile.

## Implemented slice

- project, preview, inspector, and full-width timeline regions;
- viewport-only element construction for the shared 10,000-clip / 300-layer dataset;
- ruler, layer grid, clip labels, selection, scripted zoom/pan/scroll, and playhead;
- dynamic effect controls and two UTF-16-aware CJK text-input probes;
- optional second native preview window;
- a caller-provided bi-planar `CVPixelBuffer` imported by GPUI-CE's Metal renderer through
  `CVMetalTextureCache`, without a GPU-to-CPU readback.

The adapter and shared workload use `#![forbid(unsafe_code)]`. Framework types remain inside the
adapter, and the authoritative dataset and workload contain no GPUI state.

## Automated result

The native animation-frame driver completed 2,400 frames and closed normally. Of 2,160 measured
frames:

| Metric | Result |
| --- | ---: |
| UI/model preparation CPU p50 / p95 / p99 | 185.000 / 309.541 / 338.417 us |
| UI/model preparation CPU maximum | 588.375 us |
| Visible query CPU p95 | 14.375 us |
| Frame interval p50 / p95 / p99 | 16.666 / 17.630 / 17.679 ms |
| Frame interval maximum | 17.698 ms |
| Frames over 16.67 ms | 813 |
| Standalone release binary | 6,892,936 bytes |
| Peak resident set | 94,584,832 bytes |
| Process CPU in 40.73 s | 10.44 s user + 0.78 s system |

The exact machine-readable result is in `gpui-macos.json`. Preparation CPU ends when the GPUI
element tree has been constructed, so it excludes retained layout and Metal drawing. Cross-framework
comparison should use process CPU and presented-frame intervals instead. The final driver uses
`Window::request_animation_frame()`, backed by the macOS `CVDisplayLink`; it has no application-side
target interval and follows the active display. Two additional 180-frame runs both closed normally
in 3.15-3.16 seconds, with frame-interval p95 of 17.331-17.384 ms.

## Current evidence

- Preview interoperability: GPUI-CE's public macOS surface accepts `CVPixelBuffer`, which is a strong
  zero-readback path for VideoToolbox-backed decoding. Unlike the Linux/FreeBSD implementation, the
  macOS public API does not accept an application-owned wgpu texture. Connecting AviQtl's existing
  compositor output would therefore require a Metal/CoreVideo bridge or an upstream API extension.
- Visible-range scaling: passes. Only shared-query results become GPUI elements.
- Multi-window path: implemented with a second native window and disabled during measurement.
- CJK path: UTF-16 conversion and grapheme navigation have focused tests, and editable controls are
  wired to GPUI's platform input handler. The command-line binary is not registered as an automation-
  addressable macOS app, so a live IME composition pass remains unverified.
- Safety and maintenance: handwritten code forbids unsafe Rust, but GPUI-CE 0.2.2 transitively uses
  `block 0.1.6`. Rust 1.98 reports its uninhabited Objective-C block static as future-incompatible and
  states that it will become a hard error in a future Rust release.
- Surface/device recovery: the native Metal renderer recreates drawable resources around window
  changes, but it does not expose an application-level recovery contract comparable to wgpu surface
  error handling. This remains an integration risk to test in a production prototype.
