# wgpu 30 compatibility probe

This probe was run on Apple M5 / Metal with the Slint migration at commit `3e2fd92`. It used an
isolated copy of the worktree so that the supported wgpu 29 frontend baseline remained unchanged.

## Results

- Changing `aviqtl-render`, `aviqtl-preview`, and `aviqtl-export` from wgpu 29 to wgpu 30.0.1
  required one source compatibility change: `BufferSlice::get_mapped_range()` now returns a
  `Result`, which the preview readback path propagates as a string error.
- All 297 workspace tests other than the Slint binary passed with the application GPU stack on
  wgpu 30.
- Strict Clippy passed for every workspace target other than the intentionally incompatible Slint
  binary.
- A real offscreen compositor render completed on Apple M5 / Metal. Reading the 640 x 360 RGBA
  target back produced checksum `61979074`; the uncaptured wgpu error list was empty.

The standalone probe explicitly enabled wgpu's `metal` feature. In the current application this
backend feature is enabled transitively by Slint, but a renderer-only executable has no backend
unless it selects one itself.

## Current frontend boundary

Slint 1.17.1 exposes `unstable-wgpu-29`, `slint::wgpu_29`, `require_wgpu_29`, and
`to_wgpu_29_texture`. It does not expose a wgpu 30 equivalent. The current dependency graph
therefore contains both Slint's wgpu 29.0.4 and the application stack's wgpu 30.0.1. Compiling the
production Slint frontend reports seven errors across these integration boundaries:

1. Passing Slint's wgpu 29 `Device` and `Queue` into the wgpu 30 `PreviewSurface` constructor.
2. Importing the preview surface's wgpu 30 `Texture` into `slint::Image`, both during startup and
   after the surface is recreated.
3. Comparing Slint's retained wgpu 29 texture with the preview surface's wgpu 30 texture.
4. Passing Slint's wgpu 29 `Device` and `Queue` into the wgpu 30 `ExportManager` constructor.

These are expected type-identity failures from having wgpu 29 and 30 in the same dependency graph,
not renderer, preview, export, shader, or Metal backend failures.

wgpu 30 also adds `apply_limit_buckets` to `RequestAdapterOptions`. Using
`..Default::default()` after AviQtl's explicit adapter preferences is sufficient.

## Upgrade and rollback plan

Keep the production Slint frontend on wgpu 29 until Slint exposes a matching wgpu 30 integration.
At that point:

1. Update `aviqtl-render` and the Slint unstable-wgpu feature together.
2. Update `aviqtl-preview` and `aviqtl-export` in the same change because they now retain the shared
   GPU types supplied by Slint.
3. Rename the Slint bridge calls and module imports to their wgpu 30 equivalents.
4. Propagate the `BufferSlice::get_mapped_range()` result and add the `RequestAdapterOptions`
   default tail.
5. Run the full workspace suite, strict Clippy, the offscreen checksum probe, and the shared-device
   Metal validation.

The application-stack rollback is three dependency-version changes plus the small readback API
adjustment. The frontend rollback remains limited to Slint's wgpu feature/module and bridge calls,
so retaining wgpu 29 as the current production baseline does not create a costly fork. A manual
cross-version bridge would instead add duplicate-device or texture-copy complexity and should not
be introduced while Slint lacks matching public wgpu 30 types.
