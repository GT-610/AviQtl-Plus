# AviQtl Slint frontend

This crate is the Rust desktop frontend for AviQtl. Its migration target is the Qt application's
existing operation model, not an AviUtl2-inspired redesign. An experienced AviQtl user must be able
to keep the same window, menu, mouse-button, modifier-key, selection, dialog, and close-confirmation
workflow. Theme styling may change independently.

The completed behavior migration is followed by a native-presentation pass. Qt remains the source
of truth for commands, ownership, selection, editing, and confirmation semantics, but it is no
longer a pixel-layout template. Custom Slint surfaces derive their colors and spacing from the
active standard-widget style, use platform selection and focus colors, keep tabs content-sized,
and give toolbars, status bars, cards, and modal surfaces enough room for the host platform. This
avoids forcing Qt-specific density and visual hierarchy onto Slint while preserving the workflows
that existing users rely on.

## Architecture

The Slint frontend is intentionally thin:

- `aviqtl-core` owns project documents, timeline transactions, geometry planning, rendering plans,
  effect metadata, settings schemas, recovery data, and other domain rules.
- `aviqtl-app` owns framework-neutral application and workspace state: open projects, scene and clip
  selection, clipboard commands, undo/redo, transport, media import, missing media, presets, and
  settings persistence.
- `aviqtl-preview` reuses the egui migration's production frame planner, asynchronous media
  decoding, nested-scene handling, and wgpu compositor without depending on a GUI framework.
- `aviqtl-export` owns GUI-neutral export jobs, decoded-frame handoff, wgpu composition/readback,
  image-sequence output, video/audio encoding, progress, cancellation, and partial-output cleanup.
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
automatic recovery, recovery interval, undo limits, new-project defaults, timeline skimming and
dimensions, the configured 1-512 layer limit, object-settings sidebar placement, the Dark/Light/System
theme, System/English/Simplified Chinese/Japanese interface language, preview render scale, and
preview MSAA. Project settings stay outside the undo stack as in Qt,
while scene creation retains Qt's separate add and settings-update undo steps.

The system-settings window exposes Qt's nine categories and persists the same scalar, plugin-path,
and 34-shortcut keys. Plugin and shortcut rows are stable host-owned models; applying a draft also
refreshes timeline zoom bounds and the live shortcut resolver without rebuilding the window.
`previewRenderScale` renders into a smaller physical wgpu target while keeping the scene's logical
coordinates and camera, and `previewMsaaSamples` selects a real 1x/2x/4x/8x multisample compositor
with resolve for fixed-function and complex blend paths, including nested scenes. Export deliberately
remains single-sampled and full-resolution, matching the Qt separation between preview quality and
output quality.

English is the source language for Slint `@tr` strings and the fallback for unsupported locales.
The System language preference uses the operating system's preferred locale, maps Chinese locales
to Simplified Chinese and Japanese locales to Japanese, and otherwise selects English. The bundled
`zh_CN` and `ja_JP` gettext catalogs update live after Apply or OK. Built-in effect and object
metadata remains owned by the shared Qt JSON definitions; the Slint build extracts the matching
English and Simplified Chinese translations from the existing Qt TS catalogs and translates that
metadata only when projecting it into UI models.

`bakeStrategy` and `onDemandPrefetchFrames` remain persisted for Qt settings compatibility, but they
do not pretend to control a copied Qt `BakeController`: the Rust `SceneRenderPlan` caches typed clip
and effect metadata when the document revision changes and evaluates the requested frame directly.
There is currently no separate per-frame bake cache for those two controls to tune.

Catalog inventory tests read the shipped Qt metadata and require all 44 built-in effects and all 17
built-in objects to reach an explicit production preview route. Transform, clipping, and blend-layer
handling are asserted separately because they belong to geometry/crop/compositor planning rather
than the visual-effect pass.

Missing-media handling follows that boundary as well. `aviqtl-app` resolves project-relative media,
validates type-safe replacements, and commits relinks through the normal timeline command stack so
dirty state, revision tracking, Undo, and Redo remain consistent. Slint only presents the conditional
File-menu entry, missing-media status, manager, and filtered native replacement chooser.

Window placement is stored under Qt's existing `windowGeometry_*` keys. The Slint host restores and
persists logical position, logical size, and maximized state for each migrated editor window without
letting unopened hidden windows erase an older saved geometry.

Custom timeline and editor surfaces declare accessibility roles, names, descriptions, selection or
value state, and supported actions explicitly. Standard Slint widgets keep their native semantics,
and form controls receive labels instead of relying on adjacent visual text. Native VoiceOver
traversal remains part of the deferred unlocked-desktop suite.

The egui work is therefore not discarded. Domain and application crates are reused directly, while
production preview and export code have been extracted into GUI-neutral crates for Slint. The
remaining egui implementation and its tests stay a behavior reference for interaction details that
are specific to a retained-mode Slint UI.

The export window keeps the Qt draft and close workflow: it refreshes available codecs when opened,
uses project settings and the active scene range, pauses playback before rendering, rejects project
tab changes during a job, reports frame progress and ETA, confirms cancellation, and removes partial
video or image-sequence output after cancellation. Slint imports the same shared wgpu device used by
the preview path, so exported frames are composed by the production renderer before CPU readback and
encoding.

## UI rules

- Qt/QML is the behavioral source of truth until a separate product change is approved.
- Keep preview, timeline, object settings, launcher, settings, recovery, package manager, and export
  flows as separately owned native windows where Qt does.
- Do not move business rules into `.slint` files. Slint forwards intent; `aviqtl-app` commits state.
- Keep long-lived `VecModel` instances and update rows in place. Replacing a model during a pointer
  callback can destroy the delegate that owns an active drag or native context menu.
- Prefer the active Slint `Palette` and `StyleMetrics` for custom surfaces. Add a semantic token to
  `AppTheme` when an editor-specific color is needed instead of scattering platform-independent
  literals through the UI.
- Let tabs size to their content, use standard widgets for ordinary form controls, and reserve
  custom pointer surfaces for editor interactions that standard widgets cannot express.
- Assign explicit geometry to small overlay hit regions such as resize handles; implicit placement
  is not accepted for timeline input surfaces.
- Treat `QT_UI_PARITY.md` as the acceptance gate. Compile success and a visually plausible mockup do
  not establish parity.

## Running

From the `rust` directory:

```sh
cargo run -p aviqtl-slint
```

The Rust + Slint executable is validated with Cargo directly; repository-level
packaging for it is not part of this change (`BUILD.py` on this branch still
targets the Qt/CMake application — the `--frontend` switch arrives in the
stacked build-system change):

```sh
cargo run -p aviqtl-slint
```

Runtime effects, objects, plugins, effect packages, and repository metadata
are resolved from the shared resource directories during development.

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
