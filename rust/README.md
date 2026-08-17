# AviQtl Rust core

The Rust workspace is the portable domain foundation of AviQtl. New validation, normalization,
planning, interpolation, serialization, catalog, and DSP rules belong here unless they require a
native framework object to perform their work. C++ must not keep a second implementation of a
Rust-owned rule.

## Ownership boundary

Rust owns the behavior that can be expressed with platform-neutral data:

- project, settings, preset, package, effect, script, plugin, and keyframe documents;
- project migrations, defaults, schema normalization, and deterministic serialization;
- authoritative project/timeline state, reversible edit patches, scene and clip mutation, timeline
  edit planning, selection, scene settings, snapping, duration limits, and ID allocation;
- keyframe normalization, mutation, easing names, interpolation, typed numeric evaluation, and
  batch evaluation;
- audio resampling, mixing, metering, and batch-mix policy;
- media time/duration calculations, permission lookup, recovery identifier validation, package
  safety decisions, and catalog ordering/filtering.

Qt/C++ is an adapter layer. It converts `QString`, `QVariant`, `QJson*`, and QObject state to the
fixed-layout or JSON inputs accepted by Rust, then publishes the result to QML and native
subsystems. It also retains operations that inherently require native framework handles:

- QML, QObject ownership, signals, `QUndoStack`, translations, dialogs, and window lifecycle;
- filesystem path canonicalization, atomic Qt file writes, networking, ZIP deployment, and
  platform directories;
- FFmpeg decoder/encoder contexts and frame ownership;
- QRhi, QImage, QVideoFrame, shaders, and render-thread resources;
- Carla process discovery, plugin instances, and host callbacks;
- Lua VM execution and other third-party C/C++ object lifecycles.

`DocumentModel` is a Qt render projection of the current timeline. It exists to publish QObject
change notifications and provide Qt-shaped input to the baking adapter; it is not an independent
project source of truth. Project-document rules and persisted ownership live in Rust.

The C++ ECS is likewise a native render/audio projection cache. Its POD components are produced by
Rust timeline baking and consumed directly by the QRhi/QML and Carla/Qt audio boundaries. It must
not acquire project editing, validation, or serialization rules.

When adding behavior, prefer one of the existing Rust document or fixed-layout batch APIs. A C++
implementation is appropriate only when the operation needs a native handle or is presentation
logic. Conversion loops at the ABI edge are adapters, not alternative domain implementations.

Scene and clip edits should be expressed as targeted timeline-state requests so Rust can validate
the authoritative state and produce reversible patches without rebuilding the whole project
document. The `replace_document` request remains a compatibility path for project load/reset and
for native effect, plugin, or keyframe projection changes that have not yet moved to targeted Rust
state mutations.

## C ABI contract

`core/include/rust_core_abi.hpp` is the single raw C ABI surface shared by the C++ application and `aviqtl-rust-core`.

- Callers must compare `aviqtl_core_abi_version()` with `AVIQTL_RUST_CORE_ABI_VERSION` before relying on the ABI and may inspect `aviqtl_core_capabilities()` for optional operations.
- The boundary contains only fixed-layout scalar fields, pointers, and lengths. Qt types never cross it.
- Every byte and fixed-layout buffer is owned by the caller. The timeline-state API is the sole
  documented exception: Rust returns an opaque handle from `aviqtl_timeline_state_create`, and the
  caller must release it exactly once with `aviqtl_timeline_state_destroy`. The handle never exposes
  Rust memory and all JSON output still uses caller-owned buffers.
- A null pointer is accepted only when its matching length is zero. Non-empty buffers must be correctly aligned and valid for the duration of the call.
- Mutable output ranges must not overlap any input or other output range. Violations return `AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS` where the operation has a status result.
- Adding or reordering fields, changing a function signature, or changing a discriminant requires an ABI version increment. Additive functions or capabilities that leave existing layouts intact may use a new capability bit.

## Toolchain and validation

The workspace uses Rust edition 2024. The repository `rust-toolchain.toml` pins rustup to Rust
1.97.1, and the crate's minimum supported Rust version matches that toolchain. Changes to the core
must pass:

```sh
rustup run 1.97.1 cargo fmt --all --check
rustup run 1.97.1 cargo test --workspace
rustup run 1.97.1 cargo clippy --workspace --all-targets -- -D warnings
```

The C++ adapters and consumers must then pass the normal CMake build and complete CTest suite.
