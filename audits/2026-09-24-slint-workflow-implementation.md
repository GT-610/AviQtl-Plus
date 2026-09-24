# Slint workflow implementation (2026-09-24)

Base: `origin/main` at `634bf26`. Branch: `feat/slint-aviutl-workflow`.
This implements the first three delivery batches of the
[UI research](2026-09-24-slint-ui-aviutl-research.zh-CN.md), with the explicit limits below.
The fourth batch requires separate editing-model and window-architecture design.

## Delivered behavior

| Area | Result | Implementation boundary |
| --- | --- | --- |
| Parameter editing | Labeled audio controls; one undo entry per slider/scrub/text gesture; cancel restores the original value | Application-owned continuous transaction, compacted adjacent clip replacements, history-limit regression coverage |
| Numeric entry | Standard LineEdit with a separate drag grip; Shift for 0.1 sensitivity; Enter/blur commit, Escape cancels | Metadata bounds remain authoritative; no invented soft-range semantics |
| Object settings | Hide/resize the effect sidebar, fold effect groups, two-level numeric rows, show keyframe rows only when animated | Preserves start/end values and standard Slider, TextEdit, CheckBox, and ComboBox controls |
| Motion | Standard context menu for no movement, linear, quadratic ease-in/out/in-out; advanced curve window retained | Uses the existing easing commands and undo grouping |
| Menus and transport | Standard menu icons and configured accelerators; standard playback buttons | Menu dispatch consumes accelerators once; input focus suppresses project accelerators in the main window |
| Shortcut preferences | Search, record, clear, and reject conflicting/invalid bindings | Retains the existing 34 actions and cross-platform key parser; no new AviUtl shortcut preset claimed |
| Timeline navigation | Snap/skimming toggles, fit all/selection, return to previous zoom, visible hover/playhead target | Snap is scene-owned; skimming is a persisted preference |
| Layer header | Ctrl/Command-click selects the layer contents | Plain click retains visibility-toggle-and-select behavior |
| Drag feedback | Planned anchor geometry, start/end/duration/delta, snap destination line, invalid-placement hint | Preview and commit use the same read-only planner; multi-selection commands retain existing validation |
| Effect selection | Standard searchable list, favorites, recent effects, keyboard selection/add/cancel | Effects and audio plugins use separate preference namespaces; selection change closes the picker; scan/translation refreshes project matching IDs and rows together |
| Workspaces | Editing/animation/audio arrangements, save/restore layout, bring windows to current monitor | Retains three independent windows; small monitors may require overlap to preserve minimum control sizes |
| Languages | New strings translated into Simplified Chinese and Japanese | English remains the source/fallback language |

The drag feedback end is the exclusive boundary (`start + duration`). The highlighted destination
uses the existing scene grid snap planner, not newly introduced clip-edge snapping. Only the
anchor clip receives a live geometry outline; the whole selected group is validated and committed.

## Deliberate compatibility changes

The Qt parity checklist remains useful for ownership, commands, and close/save semantics. This
work deliberately extends interaction behavior where the research called for it: continuous
parameter history, live text preview, Ctrl layer selection, shortcut recording, and workspace
presets. Standard Slint controls do not imply each control is a Win32 native widget.

Workspace restore clamps the saved arrangement onto the current monitor; it is a recovery-oriented
layout preset, not exact multi-monitor reconstruction or docking. Favorites, recent effects, and
the saved layout are application preferences; project serialization is unchanged.

## Deferred scope

Object-level AviUtl midpoints, preview transform handles, media bins/source in-out points,
multi-object property editing, ripple/roll/slip/slide tools, and integrated docking remain the
fourth batch. Command execution search, editable timecode, markers, range looping, and thumbnail
caches are also not included. Shortcut search currently filters configuration rows only.

The full desktop acceptance matrix still requires Chinese IME composition, all supported DPI
levels/themes, physical multi-monitor changes, screen-reader traversal, and platform-specific
menu behavior. Automated model coverage is not a substitute for these checks.

## Validation

Commands used the repository's local MSVC/FFmpeg build environment.

- `cargo fmt --manifest-path rust/Cargo.toml --all -- --check`: passed.
- `cargo test --manifest-path rust/Cargo.toml --workspace`: **401 passed, 0 failed, 7 ignored**.
  Includes 124 application tests and 37 Slint frontend tests. Ignored tests retain their existing
  hardware/media-fixture prerequisites.
- `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings`: passed.
- Translation inventory: all 39 new Slint source strings have Chinese and Japanese translations.
- `git diff --check`: passed.
- Release compilation succeeded. `cargo run --release --manifest-path rust/Cargo.toml -p aviqtl-slint -- --validate-gpu --frames 120`
  then exited with status 1 during GPU initialization: **No suitable graphics adapter found**;
  DX12/Vulkan drivers could not be loaded and GL reported no adapters. No editor window opened.
  The final integer-scrub rounding change was compiled by the full workspace tests and Clippy.

Desktop automation was initialized, but editor interaction could not begin because GPU startup
failed. No claim is made for pointer/focus/IME/DPI acceptance, screenshot review, successful GPU
rendering, or a performance improvement. Re-run the GPU command and the research's desktop task
matrix on a supported graphics environment before declaring desktop acceptance complete.

