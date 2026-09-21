# Slint UI parity audit — Qt vs Rust/Slint frontend

Baseline: `cfc2b137` on `main`. Scope: functional and interaction differences between the retained
Qt/QML frontend (`ui/qml/`) and the new Slint frontend (`rust/aviqtl-slint/`). Per the project's
acceptance rule, colors/typography/spacing may differ; commands, mouse buttons, modifier keys,
selection rules, window ownership, dialog order, and confirmation behavior may not.

Verification performed on this host: `cargo check -p aviqtl-slint --all-targets` (clean),
`cargo test -p aviqtl-slint` (27/27 pass), and the documented GPU gate
(`--validate-gpu --frames 120`) on a **release** build (passes, prints `SLINT_WGPU_COMPATIBILITY`).
The **debug** build fails that same gate (see finding S4).

Severity: **High** = a Qt workflow is lost or behaves differently; **Medium** = extra steps or
misleading UI; **Low** = cosmetic or wording only.

---

## High severity

### S1. Timeline/clip context menu lost its inline search field
- Qt: `ui/qml/timeline/TimelineView.qml:1242-1263` puts a `TextField` inside the context menu
  (`placeholderText: "エフェクト/オブジェクトを検索..."`). It is index 0 of the menu
  (`TimelineView.qml:1133` keeps `count > 1`), rebuilds the object/effect/plugin list as you type
  (`TimelineView.qml:1142-1169`), and is shown for both `timeline` and `clip` targets.
- Slint: `rust/aviqtl-slint/ui/timeline.slint:966-1051` is a native `ContextMenuArea` with **no**
  `LineEdit`. The search wiring is still present but unreachable:
  - `timeline.slint:76` declares `context-search-query` and `:220` resets it — it is never bound to
    any input and never read.
  - `filter-context-catalog` is only ever called with an empty query (`timeline.slint:223`);
    `on_filter_context_catalog` (`src/callbacks.rs:1507`) therefore only ever fills the full list.
  - The old `TimelineContextRow` / `context-catalog-page` components no longer exist anywhere.
- History: commit `1a68e3f1` ("Cascade timeline catalog context menus") replaced the accessible
  `PopupWindow` — which had `context-search := LineEdit` wired to
  `filter-context-catalog(value, …)` — with `ContextMenuArea` and dropped the search field.
- Documentation drift: `rust/aviqtl-slint/QT_UI_PARITY.md:202-211` still states the menus "use an
  accessible Slint `PopupWindow`" and that the popup "focuses the same search field immediately,
  swaps the command list for Rust-filtered object/effect/audio-plugin results while typing". That is
  no longer true of the code.
- Impact: the Qt search path for inserting one of 61 effects / 17 objects / audio plugins is gone.
  Users must navigate nested menus instead of typing. This is a workflow regression, not a styling
  choice, and the acceptance doc was not updated to record the decision.

### S2. Layer-header left click no longer toggles visibility
- Qt: `ui/qml/timeline/LayerHeader.qml:95-101` — left click calls
  `setLayerVisible(layerIndex, !isVisible)` **and** sets `selectedLayer = layerIndex`.
- Slint: `rust/aviqtl-slint/ui/timeline-items.slint:117-120` routes left-down to
  `root.activate(layer.index)`; `timeline.slint:575` forwards to `layer-activated`;
  `src/callbacks.rs:1875-1880` now calls only `workspace.select_layer(layer)`.
- History: commit `cfc2b137` "fix(slint): keep layer activation to selection only" removed
  `workspace.toggle_layer_visibility(layer)` from that handler.
- Documentation drift: `QT_UI_PARITY.md:48` still records
  "Layer left click | Toggle visibility and select that layer | **parity**".
- Impact: hiding/showing a layer — the most common timeline gesture in Qt — now requires a
  right-click menu item or the `Ctrl+H` shortcut. Nothing in the Slint header (the `●`/`○`
  indicator at `timeline-items.slint:71-74`) is clickable.

### S3. Clip "Add Effect" / audio-plugin menus are flat, not categorized, and cannot be searched
- Qt builds nested category submenus for effects (`buildEffectMenu`, `TimelineView.qml:1084-1106`)
  and per-category submenus for audio plugins (`buildAudioPluginMenu`, `1108-1130`), each with
  icons; the search field flattens results only while it has text.
- Slint renders one `Menu` titled "Add Effect" whose items are a flat list with the category
  prefixed to the label: `timeline.slint:1043-1049`
  (`item.categories + " / " + item.name`). Audio plugins use the same flat projection
  (`src/object_settings.rs:650-660`).
- Impact: with 44 built-in effects plus packages, a single flat menu is significantly harder to
  navigate than Qt's category tree. Object insertion is unaffected —
  `timeline.slint:974-983` still uses the nested `object-catalog-menu-categories`.

### S4. Debug-build acceptance gate crashes at shutdown
- `cargo run -p aviqtl-slint -- --validate-gpu --frames 120` (the command documented in
  `rust/aviqtl-slint/README.md:168-170`) exits **6/6 runs** with code `2173` and never prints the
  `SLINT_WGPU_COMPATIBILITY` summary. Two earlier runs also produced `-1073741819`
  (`0xC0000005`, access violation).
- The crash is in teardown, not startup: with `--frames 100000` the process stays alive and renders
  normally for 18 s. It only dies once the frame limit hides every window
  (`src/main.rs:673-704`) and the event loop unwinds toward `stats.print_and_validate()`
  (`src/main.rs:725-727`).
- The **release** build is unaffected: two consecutive runs exit `0` and print the expected JSON
  with `wgpu_errors: []`.
- Impact: the documented dev validation procedure is unusable in a debug build, so a contributor
  following the README sees a hard crash and no gate result. Not user-facing, but it hides the
  project's own acceptance signal. (Note: `main.rs:673-704` also never calls
  `slint::quit_event_loop()`; the loop only ends because hiding the last window closes it, which is
  a fragile way to terminate.)

---

## Medium severity

### M1. Audio clips get a "Browse effect catalog…" entry Qt does not show
- Qt adds the clipping item, a separator, and "Browse effect catalog..." only inside
  `if (!Workspace.currentTimeline.isAudioClip(targetClipId))` (`TimelineView.qml:1205-1219`).
- Slint gates only the clipping item on `context-target-kind == 1`; the browse entry is
  `if root.context-target-kind > 0` (`timeline.slint:1038-1041`), so audio clips (`kind == 2`,
  assigned at `timeline.slint:733`) also get it, and it opens the object-settings effect picker.

### M2. Clip context-menu separator order differs for visual clips
- Qt visual clip: Cut, Copy, **sep**, Clip-by-upper-object, **sep**, Browse catalog, **sep**, Add
  Effect (`TimelineView.qml:1201-1218`).
- Slint visual clip: Cut, Copy, sep, Clip-by-upper-object, Browse catalog, sep, Add Effect
  (`timeline.slint:1023-1049` — the `kind == 1` separator at line 1031 sits before the clipping
  item, and none follows it).

### M3. Frame counter is clipped to 40px
- Slint: `main.slint:169-175` fixes the counter to `width: 40px`, sets no `font-size` override and
  no `overflow` (Slint's `Text` default is `clip`). `round(playhead) + " / " + round(duration)` is
  11 characters in monospace at the default size — roughly 90px — so the value is cut off whenever
  the duration is more than a few frames.
- Qt: `MainWindow.qml:1035-1049` uses no fixed width, `font.pixelSize: 12`, and zero-pads the
  current frame to the total's digit count (`String(cur).padStart(digits, "0")`), which also keeps
  the column width stable during playback.

### M4. Playback-speed unit differs
- Qt renders the speed SpinBox as a multiplier via `textFromValue` → `"1.0x"`
  (`MainWindow.qml:1128-1149`).
- Slint shows a raw 10–400 integer plus a separate `"%"` label (`main.slint:201-219`).
- Semantics match (10–400 %, step 10, disabled while playing); only the displayed unit changes,
  so a user reading either UI side by side sees different numbers for the same state.

### M5. Menu bars have no icons and no shortcut labels
- Qt menu items are `Common.IconMenuItem` with Remix icons
  (`MainWindow.qml:1381-1513`) and a right-aligned shortcut label bound to
  `action.shortcutText` (`ui/qml/common/IconMenuItem.qml:52-62`), which follows the user's
  rebindings. The timeline/layer/clip menus do the same.
- Slint uses plain `MenuItem` (`main.slint:62-102`), so every command loses both its icon and its
  shortcut hint. `QT_UI_PARITY.md:124` records that static accelerators were deliberately removed,
  but the loss of the shortcut *label* is not recorded there and makes the 34 configurable bindings
  undiscoverable from the UI.

### M6. Project/scene tab strips cannot scroll
- Qt wraps both strips in a `ScrollView` so many tabs remain reachable
  (`MainWindow.qml:827-917` for projects, `TimelineWindow.qml:129-137` for scenes).
- Slint uses plain `HorizontalLayout`s (`main.slint:126-140`, `timeline.slint:391-404`) whose
  children are `min-width: 100px` / `max-width: 220px` (`timeline-items.slint:10-13`). Past a
  handful of scenes the tabs overflow the window with no way to reach the rest.

### M7. Four UI strings are missing from the zh_CN / ja_JP catalogs
Extracted every `@tr("…")` literal from `rust/aviqtl-slint/ui/*.slint` (368 unique) and diffed
against `translations/zh_CN/LC_MESSAGES/aviqtl-slint.po` (372 msgid, incl. 7 `msgctxt` entries).
These four have no entry, so they render in English under Chinese and Japanese:
- `Add Effect` (`timeline.slint:1044`)
- `Add Object` (`timeline.slint:975`)
- `Audio Level` (`object-settings.slint:183`)
- `Audio unavailable` (`main.slint:194`)
(Context-prefixed forms such as `confirm-save`, `timeline-cut`, `edit-menu-cut`, `launcher-width`,
`settings-height`, `save-project` are present as `msgctxt` entries and are fine.)

---

## Low severity / cosmetic

- **Context-menu icons are gone.** Qt's `IconMenuItem` shows an icon per command
  (`TimelineView.qml:1044`, `1072`, `1096`, `1120`; `LayerHeader.qml:380-488`). Slint's
  `ContextMenuArea` `Menu` supports text only. The parity doc allows decoration differences, so
  this is recorded for completeness.
- **Save-confirmation wording.** Qt's dialog body is a fixed message
  (`MainWindow.qml:710`); Slint prefixes the project name (`main.slint:366`). Title and
  Save/Discard/Cancel order match.
- **Missing-media dialog is an inline overlay.** Qt opens a real non-modal `Dialog`
  (`MainWindow.qml:620-642`, `modal: false`); Slint renders an in-window `FocusScope` panel
  (`main.slint:255-335`). Same list, Replace button, and Close; ownership differs from the
  separately-owned-window rule the README states.
- **Preview zoom control is a custom edit-plus-menu.** Slint places a `LineEdit` and a preset
  dropdown *inside* a `ContextMenuArea` hit region (`timeline.slint:415-462`). It works, but
  nesting an input inside a context-menu area is unusual and worth a manual check for right-click
  interference.

---

## Verified as matching (no action needed)

- **Menu structure and command ownership.** File / Edit / Settings / View / Tools match
  command-for-command, including separators and the Manage-Missing-Media enablement rule
  (`MainWindow.qml:1377-1517` vs `main.slint:62-102`).
- **All 34 shortcut actions.** `src/shortcuts.rs:236-356` binds exactly the 34 actions Qt defines in
  `SystemSettingsWindow.qml:21+`, with identical default sequences and the same editor-only scoping
  Qt expresses through `Qt.WindowShortcut` on MainWindow + TimelineWindow
  (`MainWindow.qml:1231-1341`, `TimelineWindow.qml:353-427`).
- **Settings keys.** All 46 persisted keys written by `ui/qml/settings/*.qml` are handled in
  `rust/aviqtl-slint/src/settings.rs`; the 9 tabs, the Reload/Apply/OK/Close draft workflow, and the
  plain-text shortcut editor all match (`system-settings.slint:216-241`, `563-573`).
- **Export dialog.** Format, image sequence, CRF/bitrade, all five x264 presets, profile, audio
  codec/bitrate, and range are all present (`export.slint:140-179`).
- **Package Manager.** All six tabs (Effects, Objects, MOD, Installed, Application, Repository) and
  the repository sync/add/upgrade-all commands match (`PackageManagerWindow.qml:99-104` vs
  `package-manager.slint:44`).
- **Object-settings control types.** Qt's `ControlLoader.qml:146-175` types
  (float/number/slider/spinner, int/integer/**scene_id**, color, bool, path/file, string/text,
  enum/combo, font, header) are all covered; `aviqtl-app/src/object_settings.rs:562` maps
  `scene_id` to Integer, and `object-controls.slint:337-804` renders every kind including an
  explicit `unsupported` fallback. Effect presets (save/load/delete) exist in both.
- **Scene tabs.** Left-click switches, right-click opens settings, the root scene has no close
  button (`TimelineWindow.qml:171-180,197` vs `timeline-items.slint:32-52`).

---

## Resolution status

| Finding | Status | Where |
| --- | --- | --- |
| S1 context-menu search | **Fixed** | `e1825338` — search popup alongside the cascading native menu |
| S2 layer left-click toggle | **Fixed** | `002d33e6` — split into select + visibility-toggled callbacks |
| S3 flat effect/plugin menu | **Fixed** | `86640059` — one submenu per category, as Qt does |
| S4 debug teardown crash | **Open (not this crate)** | `b0a85b1e` made the exit explicit; the crash is inside Slint's event-loop teardown. Recorded under Known issues in `QT_UI_PARITY.md`. |
| M1 audio-clip browse entry | **Fixed** | `93ddc5dc` |
| M2 clip menu separators | **Fixed** | `93ddc5dc` |
| M3 frame counter clipping | **Fixed** | `68d59018` |
| M4 speed unit (`100 %` vs `1.0x`) | **Fixed** | Same commit — multiplier projected beside the native percent box |
| M5 menu icons / shortcut labels | **Open, accepted** | Slint's `MenuItem` exposes neither `icon` nor `shortcut`; matching Qt needs a custom menu surface replacing std-widgets. |
| M6 tab strips cannot scroll | **Fixed** | Scroll-strip commit on this branch — both strips wrapped in a horizontal `ScrollView` with a pinned add button |
| M7 four untranslated strings | **Fixed** | `23099b90` — both catalogs now cover all 372 UI strings |
| Doc drift in `QT_UI_PARITY.md` | **Fixed** | `444f2a5a` |

Verification after the fixes: `cargo fmt --all -- --check` clean,
`cargo test --workspace` 397 passed / 7 ignored / 0 failed, and the release
`--validate-gpu --frames 120` gate passes and prints `SLINT_WGPU_COMPATIBILITY`.

Two findings remain open and are not defects in reach of this branch: the debug-only
teardown crash sits inside Slint's winit/wgpu event loop (S4), and menu icons plus
shortcut labels need a custom menu surface because Slint's `MenuItem` exposes
neither (M5).

## Suggested order of work

1. ~~Restore the inline context-menu search (S1)~~ — done in `e1825338`.
2. ~~Restore layer left-click visibility toggle (S2)~~ — done in `002d33e6`.
3. ~~Categorize the clip "Add Effect" / plugin submenu (S3)~~ — done in `86640059`.
4. ~~Fix the frame counter width (M3)~~ — done in `68d59018`.
5. ~~Make the validation teardown explicit (S4)~~ — done in `b0a85b1e`. The residual debug
   crash is in Slint's winit/wgpu teardown; re-check it when the dependency is next updated.
6. Remaining: none actionable in this branch. M5 needs a custom menu surface, and S4's teardown
   crash is in Slint itself; re-check both when the dependency is next updated.
