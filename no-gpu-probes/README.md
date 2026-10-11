# No-GPU probes

Standalone probes that isolate what a GPU-less Windows host can and cannot run for the
Slint frontend. Each one is a self-contained Cargo crate with its own `[workspace]`, so they
never join the `rust/` workspace build and never need `cargo build --workspace` to succeed.

They exist because "the Slint frontend crashes on a machine with no GPU" was reported here,
and the failure is not reproducible from the application build alone. These probes split the
problem into layers so each claim is measured, not inferred.

## What was concluded

Every layer tested individually **works** on a host that exposes only the Microsoft Basic
Render Driver (WARP, `DeviceType::Cpu`):

| Probe | Layer under test | Result |
| --- | --- | --- |
| `slint-software-probe` | Slint with `renderer-software` | Window created, event loop ran, no GPU needed |
| `wgpu-dx12-probe` | wgpu 29 texture upload and read-back on WARP | 0 mismatched bytes |
| `wgpu-shader-probe` | wgpu 29 + WGSL pipeline on WARP | Device, shader module, pipeline all created |
| `fv-wgpu-probe` | Real winit window + surface + pass + present on WARP | 5 frames presented |
| `fv-canvas-probe` | femtoVG `Canvas` + `WGPURenderer` + glyph atlas | 5 frames presented |
| `slint-wgpu-probe` | Slint's own wgpu 29 backend selection | Selection fails without `SLINT_WGPU_CPU` |

The application itself (`rust/aviqtl-slint`) still fails on such a host: it requires a
GPU-backed wgpu adapter and then exits `0xC0000005`. The failure is in how Slint's
`Manual` wgpu configuration combines these layers, which none of these probes
reproduces in isolation. See the companion documentation for the full record.

## Running them

They need network access to fetch their dependencies through the crates.io mirror. With a
working mirror configured:

```powershell
cd no-gpu-probes\wgpu-shader-probe
cargo run
```

Each probe prints `STAGE ...` lines as it advances and a final `RESULT ...` line. Read the
stage list first if you are chasing where a new failure appears.

## Notes

- These are throwaway diagnostics, not shipped code. They are kept for reproducibility, not
  for the build.
- `slint-wgpu-probe` may resolve to a different Slint minor than the application pins, and
  Slint's `renderer-femtovg-wgpu` feature moved to an internal `wgpu_30` module after
  1.17.1. Check what it resolves to before trusting a negative result from it.
- `fv-canvas-probe` renders real text via `arial.ttf`; it will not draw glyphs if that font
  is absent, but it still completes the non-text path.
