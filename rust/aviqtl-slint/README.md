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
- `aviqtl-preview` reuses the egui migration's production frame planner, asynchronous media
  decoding, nested-scene handling, and wgpu compositor without depending on a GUI framework.
- `aviqtl-render`, `aviqtl-media`, `aviqtl-audio`, and `aviqtl-carla` retain the lower-level
  renderer, media, audio, and plugin work produced during the egui migration.
- `aviqtl-slint` owns native windows, declarative layout, input hit regions, menus, accessibility,
  and translation between Slint models/callbacks and `aviqtl-app` commands.

Open and Save As use `rfd` platform dialogs. On Linux its default backend is the XDG portal rather
than GTK, preserving the migration goal of not reintroducing a large native GUI build dependency.
Save/Discard/Cancel sequencing remains in `aviqtl-app`; the native chooser only returns a path.

Project recovery follows the same split. `aviqtl-app` owns recovery IDs, metadata validation,
periodic scheduling, a single background file worker, stale cleanup, and Save/Close/Quit cleanup.
Slint only presents the launcher-owned recovery list and forwards Recover or Discard. Recovering
always creates a dirty pathless project, so Save opens Save As and never overwrites the original.

Settings follow the same ownership boundary. `SettingsStore` remains the persisted source of truth;
Slint windows only hold drafts and forward Apply, Reload, OK, or Close. Runtime settings update the
application model immediately after a successful atomic save, including quit confirmation,
automatic recovery, recovery interval, undo limits, and new-project defaults. Project settings stay
outside the undo stack as in Qt, while scene creation retains Qt's separate add and settings-update
undo steps.

The egui work is therefore not discarded. Domain and application crates are reused directly, while
production preview code has been extracted into a GUI-neutral crate for Slint and the future export
path. The remaining egui implementation and its tests stay a behavior reference for interaction
details that are specific to a retained-mode Slint UI.

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

The current validation project exercises the two-window Slint/wgpu integration, real timeline
models, and the production preview planning/decoding/compositing bridge:

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

The isolated wgpu 30 renderer and Metal probe is recorded in
[`WGPU_30_COMPATIBILITY.md`](WGPU_30_COMPATIBILITY.md). The renderer already passes on wgpu 30;
the production frontend remains on 29 until Slint exposes the same wgpu version for device and
texture sharing.
