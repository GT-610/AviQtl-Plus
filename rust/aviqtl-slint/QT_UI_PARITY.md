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
| New project | Ctrl+N raises the independent non-modal launcher before creating a tab | foundation |
| Open project | Owned chooser and Qt replace-or-new-tab lifecycle | foundation |
| Save / Save As | In-place fallback, suffix, overwrite, cancellation, and deferred close behavior | foundation |
| Export | Settings, planning, progress, cancellation, cleanup, and close interception | foundation |
| Missing media | Conditional command, type-safe replacements, and project updates | foundation |
| Quit | Visit dirty projects in tab order and complete Save/Discard/Cancel before exit | foundation |
| Scene tabs | Left switches, right opens settings, root cannot close, plus creates | foundation |
| Timeline focus | Window-scoped editor shortcuts; text inputs suppress editor shortcuts | foundation |
| Timeline skimmer | Snapped hover target, Shift bypass, command and import advancement | foundation |
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
| Layer shortcuts | Ctrl+L/Ctrl+H and Alt+arrows | foundation |
| File drop | Insert media at indicated frame/layer with Qt import rules | missing |
| Timeline navigation | Qt wheel axes, anchored zoom, and both draggable scrollbars | foundation |
| Playback controls | Seek, frame counter, previous/play/next, speed, and end-frame behavior | foundation |
| Object settings | Metadata order, pickers, keyframes, easing, and sidebar placement | foundation |
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
- The preview no longer uses the animated validation placeholder. A GUI-neutral `aviqtl-preview`
  crate now plans the active project frame, resolves project-relative media, decodes media and
  generated objects off the UI thread, preserves nested scenes, frame buffers, upper-object masks,
  cameras, blend/crop/transform data, and all renderer-backed visual effects, then composites into
  the Slint-imported wgpu texture.
- Preview invalidation follows a stable project-instance ID plus document revision, selected scene,
  and playhead. Timeline edits, project settings, Undo, and Redo advance the revision; Slint redraws
  only after a decoded frame batch is ready. Automated planning, decoding, and state invalidation
  tests pass, while rendered-output parity remains part of the deferred native GUI suite.
- The Qt-shaped export window refreshes runtime codec availability, maps video, image-sequence,
  quality, audio, and range settings into a GUI-neutral `aviqtl-export` request, pauses transport,
  rejects project-tab changes, renders through the production preview/wgpu path, and reports
  progress and ETA. Worker tests cover request units, completion events, video and image-sequence
  cancellation, and removal of partial outputs. Native modality, chooser behavior, close interception,
  and a user-observed output comparison remain in the deferred GUI suite.
- Real project, scene, 128-layer, clip, selection, and transport models reach Slint.
- Main and timeline `FocusScope`s resolve all 34 configurable Qt shortcut actions from the live
  settings store. Editor shortcuts remain window-scoped and use the active timeline skimmer only
  when the timeline window receives the key. Static Slint menu accelerators were removed so they
  cannot continue firing after the user changes a binding; native shortcut labels remain part of
  the deferred interaction review.
- The timeline skimmer uses the same scene snapping service as Qt, supports Shift bypass, and
  advances after Paste or Duplicate without discarding the clip selection. A one-shot leave timer
  prevents transitions between a clip body, resize handles, and the timeline background from
  briefly hiding the shared skimmer target.
- Timeline wheel routing follows Qt: the dominant axis scrolls horizontally, Shift scrolls
  vertically, Alt/Ctrl applies configured stepped zoom around the pointer, the ruler uses 1.1/0.9
  anchored zoom, and the layer header scrolls the shared vertical viewport. Pure Rust tests cover
  modifier routing, clamping, scale conversion, and pointer-anchor preservation; native touchpad
  direction and scrollbar interaction remain in the deferred GUI suite.
- Primary/additive selection, right-button box selection, multi-clip move, and multi-clip resize have
  framework-neutral tests and Slint MCP interaction checks.
- Open replaces only a clean pathless placeholder; Save, Save As, tab close, and application quit
  share a tested framework-neutral lifecycle state machine. Dirty projects are visited in tab order,
  and save failure or chooser cancellation stops the deferred close action.
- Recovery snapshots use the same Rust metadata contract as Qt and are written by a GUI-neutral
  background worker. Every project owns an independent recovery ID; newer generations replace old
  snapshots, claimed entries disappear from the launcher-owned recovery window, and successful
  Save, tab close, or application quit removes the corresponding snapshots.
- A recovered snapshot opens as a dirty, pathless project. Its original project URL is displayed
  for context but is never installed as the save target, so the first Save always follows the Qt
  Save As path and cannot silently overwrite the original file.
- Project settings now load the active project's width, height, frame rate, and sample rate, apply
  them directly as a dirty non-undoable project-property change like Qt, and leave the model
  untouched on Cancel. Scene creation and editing preserve every Qt field, including duration,
  grid mode, BPM, offset, interval, subdivision, and magnetic snapping. A newly created scene keeps
  Qt's two-step Add Scene then Update Scene Settings undo order.
- The first system-settings slice preserves the Qt draft workflow: opening or Reload copies the
  persisted values, Apply saves without closing, OK saves and closes, and Close discards the draft.
  Applying settings immediately updates quit confirmation, automatic recovery enablement and
  interval, undo limits for existing and future projects, and launcher/new-scene defaults.
- The Slint frontend uses native `rfd` open/save dialogs. Its default Linux backend is the Rust
  XDG portal path rather than GTK, so the chooser does not restore a Qt or GTK build dependency.
- Clip, timeline, and layer context menus use Slint `ContextMenuArea`; native macOS menus are not
  included in Slint window snapshots and require the deferred native interaction suite.
- The object-settings window now projects the selected clip's real effect stack and metadata-defined
  controls in source order. Slint forwards Ctrl/Shift selection, right-click selection and deletion,
  enable toggles, bounded numeric edits, booleans, strings, paths, colors, fonts, static choices, and
  live scene choices to the GUI-neutral workspace. Existing keyframe tracks project the Qt current
  interval with independent start/end values while retaining typed-value metadata. Numeric, integer,
  and color controls expose the mini track: click seeks, double-click adds with scene-grid snapping,
  non-zero real points drag between their neighbors, and right-click removes them; frame zero and a
  virtual duration endpoint remain fixed. The clip context command and settings button open a
  searchable live effect catalog, additions append the catalog-defined effect, and the sidebar drag
  handle reorders either one effect or the current Ctrl/Shift selection while preserving a leading
  transform. The same right-click surface now saves, lists, loads, and deletes effect presets through
  the shared `PresetStore`, including parameters, keyframe tracks, and enabled state. Sidebar enable toggles
  apply to the current multi-selection like Qt, right-click deletion removes the selected group,
  and the object-settings-window Delete shortcut uses that same selection while retaining a leading
  transform. The right-side delete button remains single-effect scoped. Easing configuration now
  opens a dedicated window backed by the Rust-owned interpolation catalog and immediately
  applies type changes, random/alternate step frames, elastic amplitude/period, and the first
  custom Bezier control pair. The Qt curve preview, multi-segment point manipulation,
  category-tree presentation, and native file/color/font pickers remain incomplete and therefore
  keep this area at `foundation`.
- Slint list models remain stable during pointer callbacks so delegates are not destroyed while a
  drag or context menu is active.

## Deferred native GUI suite

Run this only in the later user-requested task on an unlocked desktop. Cover project/scene close
ordering, F3/F4 focus and raise behavior, all native menu items, file chooser cancellation, export
and its close interception, recovery-window interaction, settings persistence, drag-and-drop, zoom/wheel behavior,
IME and shortcut scope, application close confirmation, object/effect/audio-plugin workflows,
keyframes and easing, Package Manager, About links, and VoiceOver output.
