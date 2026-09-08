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
| Timeline context menu | Searchable object catalog plus Undo/Redo/Paste/settings | foundation |
| Clip drag | 3 px threshold; unselected anchor selects first; selected groups move together | parity |
| Collision and snap | Rust planner resolves collisions; Shift bypasses grid/magnetic snapping | foundation |
| Clip resize | Both handles resize all selected clips by one delta and preserve opposite edges | parity |
| Clip double click | Raise object settings without changing command meaning | foundation |
| Clipboard | Qt selection order; Cut one undo step; Paste preserves layout and advances target | foundation |
| Duplicate / Split | Current edit-target behavior and one grouped transaction | foundation |
| Layer left click | Toggle visibility and select that layer | parity |
| Layer context menu | Insert/shift ranges, lock, visibility, show/hide all, owned dialogs | foundation |
| Layer shortcuts | Ctrl+L/Ctrl+H and Alt+arrows | foundation |
| File drop | Insert media at indicated frame/layer with Qt import rules | foundation |
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
| Accessibility | Equivalent names, roles, descriptions, focus, and VoiceOver reachability | foundation |

## Current verified Slint foundation

- Main and timeline windows share one wgpu 29 device and imported texture on Metal.
- The Tools menu About action now raises a fixed-size independent window centered on the preview,
  projects the Cargo package version and Qt fallback codename, preserves the AGPL notice, and opens
  the same public project page through the platform browser. The link has an explicit accessibility
  action in addition to pointer activation; native focus, browser launch, and first-paint checks
  remain in the deferred suite.
- The Package Manager now projects the same Effect, Object, MOD, Installed, Application, and
  Repository tabs from a Rust-owned model. Repository synchronization runs off the Slint event loop,
  requires credential-free HTTPS across every redirect, limits metadata responses to 16 MiB, keeps
  successful repositories when another fails, resolves relative catalog references, and persists
  provenance-preserving SHA-256 cache files. Installation verifies catalog-supplied metadata and
  archive hashes, limits downloads to 256 MiB, rejects encrypted, ZIP64, traversal, duplicate, and
  symbolic-link archive entries, caps extraction at 10,000 entries and 1 GiB, and uses same-volume
  staging, backup, atomic `installed.json` replacement, and rollback. Removal uses the inverse
  transaction, Upgrade All continues after individual failures, application updates retain Qt's
  restart notification, and effect/object completion reloads the shared catalog and menus without a
  restart. The MOD permission window preserves Qt's 13 permission rows, All Allow/All Deny,
  Cancel/OK behavior, and shared-settings persistence. Automated tests cover partial repository
  synchronization, install/remove, unsafe archives, rollback, and preservation of other plugins'
  grants. Native macOS CUA now covers the 650x450 six-tab layout, search editing and clearing,
  live repository synchronization, the non-destructive repository controls, all 13 permission rows,
  All Allow/All Deny, and Cancel without persistence.
- The preview no longer uses the animated validation placeholder. A GUI-neutral `aviqtl-preview`
  crate now plans the active project frame, resolves project-relative media, decodes media and
  generated objects off the UI thread, preserves nested scenes, frame buffers, upper-object masks,
  cameras, blend/crop/transform data, and all renderer-backed visual effects, then composites into
  the Slint-imported wgpu texture.
- Qt catalog inventory tests require every one of the 44 shipped effects and 17 shipped objects to
  reach an explicit production preview route. Transform, clipping, and blend-layer routing are
  asserted independently because they are consumed by geometry, crop, and compositor planning.
- Preview render scale keeps logical scene coordinates and camera projection while reducing the
  physical wgpu target. Preview MSAA uses real 2x/4x/8x multisample attachments and resolves both
  fixed-function and complex blend paths, including nested scenes. Export remains full-resolution
  and single-sampled, as Qt's preview-quality settings do not alter export output.
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
- Real project, scene, configured 1-512-layer, clip, selection, and transport models reach Slint.
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
- Timeline file drop uses Slint's public winit 0.30 event hook rather than platform-specific window
  code. Hover coordinates are converted through the live Slint scroll viewport, Shift bypasses the
  scene snap service, and a drop advances subsequent files from the previous import result. The
  framework-neutral workspace reuses the egui migration's proven Qt rules for extension and duration
  probing, collision-free placement, image/audio object construction, linked video/audio pairs, and
  per-file undo groups. Automated tests cover mixed sequential imports and complete undo; native
  Finder/Explorer/file-manager hover, cancellation, multi-file ordering, and drop feedback remain in
  the deferred GUI suite.
- The timeline background menu now preserves Qt's built-in object grouping and command order. Paste
  and object insertion use the right-clicked frame and layer rather than the playhead, with frame
  snapping performed by the Rust workspace. The timeline-owned object catalog supports live
  multi-field search, category filtering, selection, double-click insertion, and explicit
  Add/Cancel actions. Its compact-window render, automatic search focus, Escape dismissal, category
  navigation, selection, double-click insertion, and explicit Add/Cancel paths have Slint interaction
  checks. Object construction reuses catalog defaults, collision avoidance, locked-layer rejection,
  scene-object targeting, primary selection, and one-step Undo. Native background-menu navigation and
  pointer feel still require the deferred interaction review because the available Computer Use
  right-click action does not deliver the separate right-button release used to distinguish a click
  from Qt-compatible right-drag box selection.
- Primary/additive selection, right-button box selection, multi-clip move, and multi-clip resize have
  framework-neutral tests and Slint MCP interaction checks.
- Open replaces only a clean pathless placeholder; Save, Save As, tab close, and application quit
  share a tested framework-neutral lifecycle state machine. Dirty projects are visited in tab order,
  and save failure or chooser cancellation stops the deferred close action.
- Missing project media is resolved against the project directory and exposed through the same
  conditional File-menu action and non-modal manager as Qt. Replacement choosers are filtered by
  media type, and the framework-neutral relink command validates the new file before committing a
  dirty, revisioned, one-step Undo/Redo transaction.
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
- System settings preserve Qt's nine pages and draft workflow: opening or Reload copies the
  persisted values, Apply saves without closing, OK saves and closes, and Close discards the draft.
  General, performance, timeline, appearance, new-project, export, decode/audio, plugin, and all 34
  shortcut values keep the Qt keys and ranges. Plugin and shortcut rows use stable host-owned
  models. Applying settings immediately updates quit confirmation, automatic recovery enablement
  and interval, undo limits for existing and future projects, launcher/new-scene defaults, timeline
  skimming, track/header/ruler and hit-area dimensions, the configured 1-512 layer limit, object-settings
  sidebar placement, Dark/Light/System theme selection across every independent window, preview
  render scale/MSAA, zoom bounds, and the live shortcut resolver; restart-scoped decoder/audio values
  stay persisted. `bakeStrategy` and `onDemandPrefetchFrames` are retained as compatible persisted
  keys, while the Rust typed render planner caches complete clip/effect metadata per document revision
  and evaluates requested frames directly instead of reproducing Qt's separate per-frame bake cache.
- The layer menu preserves Qt's complete command order for one-row and multi-row insertion, current
  and explicit-range shifts in either direction, locking, per-layer visibility, and show/hide all.
  Multi-row and range edits remain single Rust-owned undoable transactions.
- Main, timeline, project, object, scene, system, easing, Package Manager, and About windows restore
  and persist Qt-compatible `windowGeometry_*` keys using logical coordinates, logical dimensions,
  and maximized state. A hidden window that was never opened does not overwrite its saved geometry.
- The Slint frontend uses native `rfd` open/save dialogs. Its default Linux backend is the Rust
  XDG portal path rather than GTK, so the chooser does not restore a Qt or GTK build dependency.
- Timeline and clip mouse context menus use an accessible Slint `PopupWindow`, because Slint 1.17's
  native `ContextMenuArea` accepts only menu entries and cannot contain Qt's inline search field.
  The popup focuses the same search field immediately, swaps the command list for Rust-filtered
  object/effect/audio-plugin results while typing, closes on Escape or an outside click, and keeps
  the Qt command order. Layer and effect-stack context menus continue to use `ContextMenuArea`.
- The clip context menu now keeps Qt's selection rule and command order for Delete, Split,
  Duplicate, Cut, and Copy. Visual clips expose the checked upper-object clipping action, the
  effect-catalog browser, and the registry's ordered category paths, including nested paths such as
  `変形/クロップ`. Audio clips omit those visual-only actions and instead expose hostable plugins
  in the same normalized category order as Qt, including the `Other` fallback. Direct menu
  insertion reuses the object-settings commands, selection projection, status updates, and one-step
  Undo. The three object-settings entry points schedule a redraw after showing the previously hidden
  window to cover the macOS first-surface paint gap. Native macOS CUA now covers audio and visual
  clip right-click, focused inline search, live effect result projection, Escape dismissal, and the
  first object-settings paint.
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
  custom Bezier control pair. Its Slint `Path` preview uses production keyframe evaluation and the
  framework-neutral multi-segment curve model covers Qt's 25%-400% zoom, right-drag pan,
  double-click insertion, handle/anchor dragging, fixed `(1,1)` endpoint, and right-click removal
  rules. The Qt category catalog is also present with the same five groups, technical-name search,
  current-item selection, and live mini previews generated by that same production evaluator.
  Path controls now invoke the existing cross-platform native file dialog with the metadata filter
  and current file preselected. Color controls expose the Qt start/end swatches and value fields plus
  an RGBA picker that preserves `#RRGGBB`/`#AARRGGBB`; font controls open a searchable system-family
  picker with rendered samples and the default-family choice. Native dialog, hit-area, focus, and
  pointer-feel verification remains deferred and therefore keeps this area at `foundation`.
- Slint list models remain stable during pointer callbacks so delegates are not destroyed while a
  drag or context menu is active.
- Custom project/scene tabs, layers, clips, keyframe tracks, effect rows, catalogs, the timeline
  ruler, and system-settings tabs now expose explicit accessibility roles, names, selection/value
  state, descriptions, and default/value actions. Standard widgets retain their native accessible
  implementations, while form fields that previously relied on adjacent text now have explicit
  labels. VoiceOver traversal and announcement order remain in the deferred native GUI suite.
- Audio objects now switch the left sidebar to the Qt audio-plugin stack while keeping the built-in
  audio-object controls on the right. The framework-neutral catalog reuses the Rust CLAP/VST3 and
  Carla LADSPA/DSSI/LV2/VST2 discovery and inspection paths, preserves Qt category order and
  multi-field search, and restores missing host metadata in older projects without changing their
  dirty state. Restoration is deferred instead of invalidating an existing undo/redo history.
  Plugin insertion, single-item drag reorder, Ctrl/Shift selection, grouped enable/delete,
  right-click presets, current-playhead parameter values, the Qt two-endpoint `K` action,
  double-click insertion, and protected non-draggable endpoints are wired through the existing Rust
  timeline commands. A bypassed plugin remains parameter-editable like Qt. Native plugin scanning,
  menu behavior, pointer feel, and hosted audio output remain in the deferred GUI suite.

## Deferred native GUI suite

Run this only in the later user-requested task on an unlocked desktop. Cover project/scene close
ordering, F3/F4 focus and raise behavior, all native menu items, file chooser cancellation, export
and its close interception, recovery-window interaction, settings persistence, drag-and-drop, zoom/wheel behavior,
IME and shortcut scope, application close confirmation, object/effect/audio-plugin workflows,
keyframes and easing, Package Manager, About links, and VoiceOver output.
