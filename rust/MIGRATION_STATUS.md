# Rust migration status

Status reviewed on 2026-09-18. The default application is Rust + Slint + wgpu;
the optional Qt/C++ frontend remains available for behavior comparison.

## Implemented ownership

The nine workspace crates cover domain rules, authoritative project/timeline
state, application commands and undo/redo, persistence, desktop presentation,
preview, rendering, export, audio, plugin integration, scripting, and packages.
`BUILD.py` builds and packages `aviqtl-slint` by default. The default executable
does not compile the project's C/C++ implementation.

The [dependency inventory](../audits/2026-09-rust-dependency-boundary.md)
distinguishes external native dependencies from retained application code and
lists shared resources that still reside beside Qt files. Those resources
remain required even when Qt is not built.

The application is unreleased and has no external extension compatibility
requirement. Supporting or converting third-party QML extensions is outside
the migration plan. Native WGSL packages are the Rust extension contract.

## Frontend refactoring checkpoint

The host split reduces `aviqtl-slint/src/main.rs` from 9,446 to 710 lines.
Runtime coordination, lifecycle, dialogs, settings, shortcuts, editor
projection, and callback wiring have explicit module imports. The existing
27 frontend regression tests remain in `src/tests.rs`.

The UI split reduces `ui/app.slint` from 6,207 to 38 lines of exports. Window
layouts, shared controls, theme globals, and data models are separate files.
All 54 declaration bodies and the 36 existing Rust-facing exports were
checked against the original source; component names and behavior are retained.
Slint's generated build output tracks the imported files for incremental builds.

Local Windows validation for this checkpoint:

- Frontend regression tests before and after the Rust split: 27 passed.
- Workspace tests with all features after both splits: 386 passed, 7 ignored.
- Workspace Clippy with all targets/features and warnings denied passed.
- Build helper tests: 11 passed; ABI checker tests: 6 passed; C ABI consistent.
- Rust formatting and documentation link checks passed.

The ignored tests cover four FFmpeg integrations, two Carla integrations, and
one hardware GPU test. They were not explicitly run for this source-only
refactoring. Qt compilation and interactive desktop validation were not run.

## Batch 4 editing model

Four AviUtl editing capabilities are now implemented as framework-neutral model
layers in `aviqtl-core` and `aviqtl-app`, with the design and compatibility rules
recorded in [the batch 4 design document](../docs/batch4-design.md) before any of
it was wired to a window. None of it requires a GPU-backed adapter.

- **Object midpoints** are stored in a clip's extra fields under `midpoints` and
  exposed through the `SetClipMidpoints` command, so undo is the ordinary
  whole-value snapshot. Geometry changes shift them and a split divides them
  across both halves. They are markers only: ExEdit's per-parameter track shear is
  a separate step that needs the desktop suite, so AviUtl midpoint compatibility
  is not claimed.
- **The media bin** is a `mediaBin` document key holding entries and flat groups.
  It never owns the timeline: removing an entry touches no clip, and a missing
  file is reported rather than dropped. Edits go through a new `SetProjectExtras`
  command because `ReplaceDocument` deliberately discards a document's extras.
- **Multi-object property editing** resolves each property across the selection
  into Uniform, Mixed, or Absent, matching effects by catalog id rather than
  index and skipping locked or absent objects instead of failing. It introduces
  no persisted key, so a project edited through it is byte-identical to the same
  edit made one object at a time, which a test asserts rather than assumes.
- **The four trim tools** (ripple, roll, slide, slip) are opt-in per-command
  planners over the existing clip geometry, so no new command variant was
  needed. Preview and commit call the same planner. A ripple or magnetic default
  was deliberately not introduced, because it would break AviUtl's
  layered-subtitle muscle memory.

Preview transform handles, the docked workspace, and every pixel-level
affordance stay in the deferred native GUI suite: handle hit testing and
coordinate mapping need a real window to verify, and free docking changes window
ownership, GPU preview hosting, focus, and shortcut scope.

## Acceptance still pending

The [Slint behavior checklist](aviqtl-slint/QT_UI_PARITY.md) remains the detailed
acceptance record. Implemented paths and unit tests do not establish native
window behavior, hardware audio output, or equivalent rendered output.
Actual desktop operation is intentionally deferred to a later session.

The [September audit](../audits/2026-09-code-audit.md) records earlier test runs
and their environment limitations. Treat those as historical evidence, not
as a claim that every current platform or desktop workflow has been tested.

## Qt retention and retirement

Qt currently serves as the executable behavior reference and a consumer of the
shared Rust C ABI. Keep its optional build and focused tests until these gates
are satisfied:

1. Complete the deferred desktop suite: project save/close ordering, window
   focus, shortcuts/IME, dialogs, timeline and object editing, recovery,
   settings, package operations, and export cancellation/cleanup.
2. Validate production preview/export output, hardware audio, and the supported
   native plugin paths. Record platform-specific failures and resolve them or
   explicitly define the supported release scope.
3. Move shared metadata, fonts, and translation inputs out of Qt-owned paths.
   Update Rust includes, runtime lookup, resource packaging, and inventory
   tests together. Confirm a packaged Rust build finds its resources without
   development checkout fallback.
4. Retain useful behavior regressions in Rust before removing the corresponding
   Qt tests. Remove the Qt build switch, CMake targets, native implementation,
   QML-only assets, Qt CI, and Qt-only dependency setup as coordinated changes.
5. Audit remaining C ABI consumers before removing its exports, header,
   `staticlib` target, or ABI checks. Rust domain behavior remains independently
   owned and tested.

None of these gates requires preserving the old QML extension runtime.
The frontend module split does not retire Qt or change acceptance statuses.
