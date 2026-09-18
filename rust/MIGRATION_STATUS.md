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
