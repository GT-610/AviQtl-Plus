# Qt UI behavior parity gate for Slint

The Slint GUI is accepted only when an existing Qt user can operate it without relearning the
editor. Colors, typography, and widget decoration may differ. Commands, mouse buttons, modifier
keys, selection rules, window ownership, dialog order, and confirmation behavior may not.

This checklist is adapted from the egui migration's completed behavior research. Qt/QML remains the
behavioral source of truth. The status column was reset for the Slint frontend; an egui result is
evidence and reusable test coverage, not automatic Slint parity.

Status meanings: `missing` has no Slint path, `partial` has a path with different or incomplete
behavior, `foundation` has a tested framework-neutral model or an incomplete presentation, and
`parity` matches Qt in automated tests plus the available Slint MCP interaction checks. Native
macOS menu-item operation, focus ownership, IME, accessibility, file choosers, and close handling
still require the separately scheduled Computer Use suite.

| Area | Qt behavior contract | Slint status |
| --- | --- | --- |
| Application shell | Launcher without a project; preview as main window; timeline and object settings as distinct F3/F4 windows | foundation |
| Main menu | Preserve File, Edit, Settings, View, and Tools ordering and command ownership | foundation |
| Project workspace | Active project tab owns timeline, selection, history, paths, recovery, and dirty state | foundation |
| New project | Ctrl+N raises the independent non-modal launcher before creating a tab | partial |
| Open project | Owned chooser and Qt replace-or-new-tab lifecycle | foundation |
| Save / Save As | In-place fallback, suffix, overwrite, cancellation, and deferred close behavior | foundation |
| Export | Settings, planning, progress, cancellation, cleanup, and close interception | missing |
| Missing media | Conditional command, type-safe replacements, and project updates | foundation |
| Quit | Visit dirty projects in tab order and complete Save/Discard/Cancel before exit | foundation |
| Scene tabs | Left switches, right opens settings, root cannot close, plus creates | partial |
| Timeline focus | Window-scoped editor shortcuts; text inputs suppress editor shortcuts | missing |
| Timeline skimmer | Snapped hover target, Shift bypass, command and import advancement | missing |
| Primary selection | Left click replaces selection and makes the clip primary | parity |
| Additive selection | Ctrl+left toggles; newly added clip becomes primary | parity |
| Empty click | Select target layer and clear clip selection | foundation |
| Box selection | Right drag previews intersections; Ctrl adds; release commits | parity |
| Clip context menu | Right click selects an unselected clip and exposes the Qt command order | foundation |
| Timeline context menu | Searchable object catalog plus Undo/Redo/Paste/settings | partial |
| Clip drag | 3 px threshold; unselected anchor selects first; selected groups move together | parity |
| Collision and snap | Rust planner resolves collisions; Shift bypasses grid/magnetic snapping | foundation |
| Clip resize | Both handles resize all selected clips by one delta and preserve opposite edges | parity |
| Clip double click | Raise object settings without changing command meaning | foundation |
| Clipboard | Qt selection order; Cut one undo step; Paste preserves layout and advances target | foundation |
| Duplicate / Split | Current edit-target behavior and one grouped transaction | foundation |
| Layer left click | Toggle visibility and select that layer | parity |
| Layer context menu | Insert/shift ranges, lock, visibility, show/hide all, owned dialogs | foundation |
| Layer shortcuts | Ctrl+L/Ctrl+H and Alt+arrows | missing |
| File drop | Insert media at indicated frame/layer with Qt import rules | missing |
| Timeline navigation | Qt wheel axes, anchored zoom, and both draggable scrollbars | partial |
| Playback controls | Seek, frame counter, previous/play/next, speed, and end-frame behavior | foundation |
| Object settings | Metadata order, pickers, keyframes, easing, and sidebar placement | missing |
| Effect selection | Ctrl/Shift selection, single/multi reorder, enable, and scoped Delete | foundation |
| Effect menus and presets | Search/category insertion and preset save/apply/delete | foundation |
| Audio plugins | Search, order, enable, parameters, keyframes, restore, and Carla formats | foundation |
| Settings windows | Separate project, scene, and system apply/cancel ownership | foundation |
| Recovery | Launcher-owned non-modal recovery, recover/discard rules, empty close | foundation |
| Package Manager | Repository sync, search, lifecycle, permissions, rollback, and update notices | foundation |
| Preview and export rendering | Every Qt built-in object/effect produces the same functional output | partial |
| Accessibility | Equivalent names, roles, descriptions, focus, and VoiceOver reachability | missing |

## Current verified Slint foundation

- Main and timeline windows share one wgpu 29 device and imported texture on Metal.
- Real project, scene, 128-layer, clip, selection, and transport models reach Slint.
- Primary/additive selection, right-button box selection, multi-clip move, and multi-clip resize have
  framework-neutral tests and Slint MCP interaction checks.
- Open replaces only a clean pathless placeholder; Save, Save As, tab close, and application quit
  share a tested framework-neutral lifecycle state machine. Dirty projects are visited in tab order,
  and save failure or chooser cancellation stops the deferred close action.
- The Slint frontend uses native `rfd` open/save dialogs. Its default Linux backend is the Rust
  XDG portal path rather than GTK, so the chooser does not restore a Qt or GTK build dependency.
- Clip, timeline, and layer context menus use Slint `ContextMenuArea`; native macOS menus are not
  included in Slint window snapshots and require the deferred native interaction suite.
- Slint list models remain stable during pointer callbacks so delegates are not destroyed while a
  drag or context menu is active.

## Deferred native GUI suite

Run this only in the later user-requested task on an unlocked desktop. Cover project/scene close
ordering, F3/F4 focus and raise behavior, all native menu items, file chooser cancellation, export
and its close interception, recovery, settings persistence, drag-and-drop, zoom/wheel behavior,
IME and shortcut scope, application close confirmation, object/effect/audio-plugin workflows,
keyframes and easing, Package Manager, About links, and VoiceOver output.
