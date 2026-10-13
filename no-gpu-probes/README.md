# No-GPU probes

Standalone probes that isolate what a GPU-less Windows host can and cannot run for the
Slint frontend. Each one is a self-contained Cargo crate with its own `[workspace]`, so they
never join the `rust/` workspace build and never need `cargo build --workspace` to succeed.

They exist because the Slint frontend died with `0xC0000005` on a host whose only graphics
adapter is `Microsoft Basic Render Driver` (WARP), and the failure could not be attributed
from the application build alone. These probes split the problem into layers so each claim is
measured rather than inferred.

## Conclusion

**AviQtl requires a GPU-backed wgpu adapter. It does not ship a CPU or software renderer.**

The frontend now detects the absence of a GPU-backed adapter before it initializes wgpu and
exits with an actionable message naming the adapters it found, instead of crashing. The root
cause was measured, not guessed:

| Layer | Result on a WARP-only host |
| --- | --- |
| Slint software renderer (`slint-software-probe`) | Window and event loop run; no GPU required |
| wgpu device, texture, shader module, render pipeline, readback (`compositor-cpu-probe`) | All succeed; readback returns matching bytes |
| Slint's own wgpu backend selection (`slint-wgpu-probe`) | Fails without `SLINT_WGPU_CPU`, per Slint 1.17.1 |
| **AviQtl's own compositor shader (`app-shader-probe`, `shader-size-probe`, `shader-fragment-probe`)** | **Fails: FXC cannot compile it, and past a size threshold the process dies with `0xC0000005`** |

The last row is the real constraint, and it is specific to AviQtl's renderer rather than to
Slint or wgpu in general. The compositor shader in `aviqtl-render` is roughly 55 KB and wraps a
41-case `switch` inside a loop whose trip count is not a compile-time constant. FXC — the
compiler WARP's DX12 backend falls back to — must fully unroll it and cannot.

Measured, one child process per data point, keeping only the first *N* switch cases:

| Switch cases kept | Shader size | Outcome |
| --- | --- | --- |
| 1–16 | 15–22 KB | Compiles |
| 17–19 | 23 KB | FXC error `X3511`: gradient instruction in a loop with varying iteration, unable to unroll |
| 20–41 (all cases) | 26–55 KB | Hard crash, `0xC0000005` |

Three candidate workarounds were measured and rejected:

- Capping the effect-strip loop with a compile-time bound does not help; a cap of 1 still crashes,
  so the failure is driven by total shader size, not by loop trip count.
- Replacing `textureSample` with `textureSampleLevel(..., 0.0)` everywhere removes the gradient
  instruction and does make cases 17–19 compile, but cases 20+ still crash or hang.
- Loading a newer DXC alongside FXC changes nothing: with `dxcompiler.dll` 1.8 present the
  adapter is created successfully and the same crash follows. An older `dxcompiler.dll` shipped
  with the 10.0.19041 Windows SDK is worse: loading it makes DX12 report no adapter at all.

On a host with real GPU hardware the same shader compiles, because the DX12 backend uses DXC
rather than FXC. This is why the failure never appeared during earlier GPU-backed validation.

## Why no software renderer

AviQtl deliberately reports an unsupported host instead of degrading. This follows the shipped
behavior of comparable applications rather than an internal guess:

- Adobe documents GPU memory requirements for Premiere (4 GB minimum on Windows) and removed the
  "Mercury Playback Engine Software Only" option in 25.2, because "software rendering can cause
  poor performance and rendering in 8-bit, which can result in image quality issues"
  (https://helpx.adobe.com/premiere-pro/kb/software-rendering-update.html).
- DaVinci Resolve requires a discrete GPU with sufficient VRAM
  (https://www.blackmagicdesign.com/products/davinciresolve).
- Kdenlive is useful evidence for where a CPU path *can* be honest: it states that "timeline
  playback uses the CPU" and its official troubleshooting guide recommends disabling OpenGL
  (https://userbase.kde.org/Archive:Kdenlive/Manual/Troubleshooting_and_Common_Problems,
  https://docs.kdenlive.org/getting_started/installation.html). That works because Kdenlive
  limits GPU use to optional render profiles. AviQtl's preview, export, and MSAA compositor are
  all GPU-only, so a CPU path would be a second implementation of the renderer whose output could
  diverge from the Qt baseline the project uses for acceptance.

`renderer-software` remains available in the pinned Slint 1.17.1 and the probes keep that
layer documented, but the application does not expose it, because selecting it still leaves the
preview compositor unable to produce a frame.

## Running them

They need network access to fetch their dependencies through the crates.io mirror. With a
working mirror configured:

```powershell
cd no-gpu-probes\shader-fragment-probe
cargo run -- 41
```

Each probe prints `STAGE`/`RESULT` lines as it advances. Read the stage list first if you are
chasing where a new failure appears. The shader probes read the compositor's WGSL directly out
of `rust/aviqtl-render/src/lib.rs`, so they need no preparation step.

A hard access violation cannot be caught in-process, so each variant under test must run in its
own child process — that is why these probes take the case count as an argument rather than
looping internally.

## Notes

- These are throwaway diagnostics, not shipped code. They are kept for reproducibility, not
  for the build.
- `slint-wgpu-probe` may resolve to a different Slint minor than the application pins, and
  Slint's `renderer-femtovg-wgpu` feature moved to an internal `wgpu_30` module after
  1.17.1. Check what it resolves to before trusting a negative result from it.
- `fv-canvas-probe` renders real text via `arial.ttf`; it will not draw glyphs if that font
  is absent, but it still completes the non-text path.