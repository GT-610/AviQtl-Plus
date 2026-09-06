# wgpu 30 compatibility probe

This probe was run on Apple M5 / Metal with the Slint migration at commit `ad59f92`. It used an
isolated copy of the worktree so that the supported wgpu 29 frontend baseline remained unchanged.

## Results

- Changing only `aviqtl-render` from wgpu 29 to wgpu 30.0.1 compiled without source changes.
- All 31 `aviqtl-render` tests passed.
- All 244 workspace tests other than the Slint binary passed with the renderer on wgpu 30.
- Strict Clippy passed for every workspace target other than the intentionally incompatible Slint
  binary.
- A real offscreen compositor render completed on Apple M5 / Metal. Reading the 640 x 360 RGBA
  target back produced checksum `61979074`; the uncaptured wgpu error list was empty.

The standalone probe explicitly enabled wgpu's `metal` feature. In the current application this
backend feature is enabled transitively by Slint, but a renderer-only executable has no backend
unless it selects one itself.

## Current frontend boundary

Slint 1.17.1 exposes `unstable-wgpu-29`, `slint::wgpu_29`, `require_wgpu_29`, and
`to_wgpu_29_texture`. It does not expose a wgpu 30 equivalent. Compiling the Slint frontend with
only `aviqtl-render` upgraded therefore fails at exactly two integration points:

1. Constructing `Compositor` with Slint's wgpu 29 `Device` and `TextureFormat`.
2. Rendering with Slint's wgpu 29 `Device`, `Queue`, and `Texture`.

These are expected type-identity failures from having wgpu 29 and 30 in the same dependency graph,
not renderer API or shader failures.

wgpu 30 also adds `apply_limit_buckets` to `RequestAdapterOptions`. Using
`..Default::default()` after AviQtl's explicit adapter preferences is sufficient.

## Upgrade and rollback plan

Keep the production Slint frontend on wgpu 29 until Slint exposes a matching wgpu 30 integration.
At that point:

1. Update `aviqtl-render` and the Slint unstable-wgpu feature together.
2. Rename the Slint bridge calls and module imports to their wgpu 30 equivalents.
3. Add the `RequestAdapterOptions` default tail.
4. Run the full workspace suite, strict Clippy, the offscreen checksum probe, and the shared-device
   Metal validation.

The renderer rollback is one dependency-version change. The frontend rollback is limited to the
Slint wgpu feature/module and the three bridge names, so retaining wgpu 29 as the current baseline
does not create a costly fork.
