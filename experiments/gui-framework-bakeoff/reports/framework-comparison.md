# Rust GUI framework comparison and recommendation

Date: 2026-09-05

Base revision: `main` at `9f46dc2`

Compared versions: egui/eframe 0.36.1, Slint 1.17.1, and GPUI-CE 0.2.2, built with
Homebrew Rust 1.98.0 stable.

## Decision

Use **egui + wgpu** for AviQtl's Rust UI migration.

The intended architecture is not "put every object into a generic egui widget tree." Keep the Rust
business core authoritative, virtualize timeline content to the visible range, use egui for the
editor shell and interaction model, and retain direct wgpu rendering for the preview and any
timeline regions that benefit from batching. This preserves the strongest result in this experiment
without making immediate-mode layout responsible for the entire video pipeline.

## Protocol

All GUI runs used the same deterministic workload:

- 10,000 clips across 300 layers;
- viewport-only timeline queries;
- scripted playback, horizontal pan, zoom, vertical scroll, and playhead movement;
- project, preview, inspector, and full-width timeline regions;
- clip selection, CJK text-entry controls, and an auxiliary-window path;
- 2,400 frames with 240 warm-up frames for the primary sample;
- two additional 180-frame runs with 18 warm-up frames for repeatability.

The final runs were serial, with only the built-in 60 Hz display attached. The macOS session was
unlocked and the display was configured not to sleep. No adapter used a fixed target frame interval:
egui requested continuous repaint, Slint scheduled the next update after the previous presentation,
and GPUI-CE used its native animation-frame request backed by `CVDisplayLink`.

This visibility condition matters. A locked or occluded macOS window can lose its display-link
callbacks and stall a frame-counted benchmark. Results collected in that state were discarded.

## Primary results

| Framework | Presented interval p95 / p99 / max | Whole-process CPU | Peak RSS | Release binary | Normal dependency packages |
| --- | ---: | ---: | ---: | ---: | ---: |
| egui | 16.890 / 17.101 / 17.745 ms | 6.32 CPU-s / 40.12 s (15.8% of one core) | 158.3 MiB | 10.52 MiB | 146 |
| Slint | 17.085 / 17.316 / 20.989 ms | 17.80 CPU-s / 40.27 s (44.2% of one core) | 121.3 MiB | 15.78 MiB | 281 |
| GPUI-CE | 17.602 / 17.622 / 17.658 ms | 10.74 CPU-s / 40.17 s (26.7% of one core) | 88.9 MiB | 6.57 MiB | 343 |

Binary sizes are from sequential standalone `cargo build --release -p <adapter>` builds. A single
workspace build can unify optional features across members and is therefore not a stable size
comparison for independently selected GUI stacks.

The adapter-local preparation measurements are not directly comparable. egui's measurement includes
rebuilding its immediate-mode paint input, Slint's measurement ends after model updates and excludes
retained-scene layout/rendering, and GPUI-CE's ends after element-tree construction. Whole-process
CPU and presented intervals are the stronger cross-framework signals.

The 16.67 ms threshold is too close to the exact period of a nominal 60 Hz display to use its count
as a ranking metric; sub-millisecond scheduler jitter moves many otherwise healthy frames across the
line. Tail percentiles and maximum intervals are more informative here.

## Repeatability

All six short runs exited automatically:

| Framework | Run | Real time | Presented interval p95 / max | Peak RSS |
| --- | ---: | ---: | ---: | ---: |
| egui | 1 | 3.15 s | 16.985 / 17.304 ms | 156.9 MiB |
| egui | 2 | 3.12 s | 16.941 / 17.403 ms | 157.0 MiB |
| Slint | 1 | 3.20 s | 17.010 / 17.452 ms | 119.5 MiB |
| Slint | 2 | 3.20 s | 17.066 / 17.449 ms | 120.0 MiB |
| GPUI-CE | 1 | 3.14 s | 17.178 / 17.677 ms | 88.0 MiB |
| GPUI-CE | 2 | 3.15 s | 16.746 / 17.677 ms | 87.7 MiB |

## Engineering comparison

| Area | egui | Slint | GPUI-CE |
| --- | --- | --- | --- |
| Preview/compositor interop | Direct registration of an application-owned wgpu texture; no readback | Direct import works, but through `unstable-wgpu-29` and a framework-owned device/queue | Excellent `CVPixelBuffer` path for VideoToolbox, but no public application-owned wgpu texture path on macOS |
| Runtime result | Lowest whole-process CPU and best p95/p99 tail in this sample | Highest whole-process CPU and the only primary sample above 20 ms maximum | Best memory and binary size, but a consistently wider p95 presentation interval |
| Surface recovery | wgpu integration has explicit reconfigure/recreate/skip handling | FemtoVG-wgpu retries lost/outdated/suboptimal surfaces | Native renderer recovery contract is less explicit at the application boundary |
| Text and IME | Built-in text editing plus a system CJK fallback font | Built-in controls and system font handling | Required substantial custom UTF-16 and grapheme-aware input code in the experiment |
| Toolchain health | No future-incompatibility warning in the tested graph | No future-incompatibility warning, but GPU import is unstable and one wgpu major behind | `block 0.1.6` emits a Rust 1.98 future-incompatibility warning that is documented to become a hard error |
| Dependency surface | Smallest tested normal dependency closure | Middle | Largest, despite the smallest binary |
| License | MIT OR Apache-2.0 | GPL-3.0-only or commercial/royalty-free alternatives | Apache-2.0 |

Slint's GPL-3.0 option is compatible with this AGPLv3 project when the combined work is distributed
under AGPLv3, so licensing is not a reason to reject Slint. The decision is driven by runtime and
integration evidence.

## Why egui wins for AviQtl

1. **Performance:** it used materially less total CPU than both alternatives while delivering the
   strongest p95 and p99 presentation intervals. Its higher resident memory is real, but it is a
   more manageable trade than sustained UI-thread/render overhead in a video editor.
2. **GPU fit:** AviQtl can hand an application-owned wgpu texture to egui without a GPU-to-CPU copy.
   This is the cleanest continuation of a Rust-native compositor and avoids coupling the compositor
   to a framework-private device version.
3. **Stability:** the tested egui/wgpu path has explicit surface error recovery and produced no
   future Rust incompatibility warning. GPUI-CE's current warning and macOS texture boundary are
   unacceptable risks for the primary UI foundation under the project's stability-first criteria.
4. **Safety:** all handwritten egui experiment code forbids unsafe Rust. More importantly, choosing
   the direct wgpu path avoids introducing a new Metal/CoreVideo bridge solely to cross the GUI
   boundary.
5. **Scale:** immediate mode is not itself a blocker for a large editor. The experiment kept 10,000
   clips in the domain model while only constructing visible paint work, and remained comfortably
   inside the 60 Hz budget. Production code must preserve that virtualization and batch especially
   dense timeline drawing instead of instantiating every clip as an always-live UI object.

## Why not the alternatives now

**Slint** is the strongest declarative option and has good built-in controls, CJK handling, and a
viable GPLv3 licensing path. In this workload it consumed about 2.8 times egui's whole-process CPU,
had the largest binary, and exposed the required texture bridge only through an unstable wgpu-29
API. Those are poor trade-offs for a migration whose priorities are performance and stability.

**GPUI-CE** achieved the best memory and binary results, and its native `CVPixelBuffer` path is
attractive for decoded video. It is not ready to be AviQtl's foundation today: the tested dependency
graph already warns of a future Rust hard error, macOS lacks a public application-owned wgpu texture
surface, and text input required much more application code. Re-evaluate it if those three issues are
resolved upstream; do not build the migration around anticipated fixes.

## Limits of this experiment

- Live IME composition could not be manually exercised because these command-line GUI binaries are
  not automation-addressable macOS app bundles. UTF-16/grapheme behavior has focused tests only in
  the GPUI-CE adapter.
- The experiment did not run Windows or Linux backends.
- The preview proves zero-readback texture/surface integration, but does not run AviQtl's complete
  decoder, effect graph, compositor, or export pipeline.
- Device-loss recovery, accessibility workflows, multi-hour playback, and repeated monitor hot-plug
  need production-prototype soak tests before the Qt UI is retired.

These limits do not change the selection: egui has the best evidence-backed path to the next
prototype, while the remaining unknowns are validation work rather than known architectural
blockers.
