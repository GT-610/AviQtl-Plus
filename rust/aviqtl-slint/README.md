# AviQtl Slint frontend

This crate is the Rust desktop frontend for AviQtl. Its migration target is the Qt application's
existing operation model, not an AviUtl2-inspired redesign. An experienced AviQtl user must be able
to keep the same window, menu, mouse-button, modifier-key, selection, dialog, and close-confirmation
workflow. Theme styling may change independently.

## Architecture

The Slint frontend is intentionally thin:

- `aviqtl-core` owns project documents, timeline transactions, geometry planning, rendering plans,
  effect metadata, settings schemas, recovery data, and other domain rules.
- `aviqtl-app` owns framework-neutral application and workspace state: open projects, scene and clip
  selection, clipboard commands, undo/redo, transport, media import, missing media, presets, and
  settings persistence.
- `aviqtl-render`, `aviqtl-media`, `aviqtl-audio`, and `aviqtl-carla` retain the renderer, media,
  audio, and plugin work produced during the egui migration.
- `aviqtl-slint` owns native windows, declarative layout, input hit regions, menus, accessibility,
  and translation between Slint models/callbacks and `aviqtl-app` commands.

The egui work is therefore not discarded. Domain and application crates are reused directly, while
the egui implementation and its tests remain a behavior reference for interaction details that are
specific to a retained-mode Slint UI.

## UI rules

- Qt/QML is the behavioral source of truth until a separate product change is approved.
- Keep preview, timeline, object settings, launcher, settings, recovery, package manager, and export
  flows as separately owned native windows where Qt does.
- Do not move business rules into `.slint` files. Slint forwards intent; `aviqtl-app` commits state.
- Keep long-lived `VecModel` instances and update rows in place. Replacing a model during a pointer
  callback can destroy the delegate that owns an active drag or native context menu.
- Assign explicit geometry to small overlay hit regions such as resize handles; implicit placement
  is not accepted for timeline input surfaces.
- Treat `QT_UI_PARITY.md` as the acceptance gate. Compile success and a visually plausible mockup do
  not establish parity.

## Running

From the `rust` directory:

```sh
cargo run -p aviqtl-slint
```

The current validation project exercises the two-window Slint/wgpu integration and real timeline
models:

```sh
cargo run --release -p aviqtl-slint -- --validate-gpu --frames 120
```

For Slint MCP interaction checks, build with debug metadata and enable the feature only on the
command line:

```sh
SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=9315 \
  cargo run -p aviqtl-slint --features slint/mcp -- \
  --validate-gpu --frames 100000
```

## Validation

Before a migration checkpoint:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p aviqtl-slint -- --validate-gpu --frames 120
git diff --check
```

Performance samples must record the active display and refresh rate. Samples taken on different
display configurations are not directly comparable. Functional MCP checks may still be compared
when logical window sizes and scale factors are recorded.
