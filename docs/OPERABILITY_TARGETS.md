# AviUtl Operability Targets

This document defines how AviQtl-Plus should feel to users who already know
AviUtl 1.10 with ExEdit 0.92, while still behaving like a modern video editor.
It is a product and contribution guide, not a promise of binary compatibility.

AviQtl-Plus does not load AviUtl or ExEdit plugins directly. The compatibility
target is the editing model: timeline habits, object settings, parameter
editing, keyframe work, preview control, and project flow should be familiar
enough that an experienced AviUtl user can become productive quickly.

## Product Principle

AviQtl-Plus should keep the parts of AviUtl that made precise, object-based
editing fast, and replace the parts that make modern production difficult.

Familiarity wins when it protects muscle memory:

- Layer-based object placement, with time moving horizontally and layers stacked
  vertically.
- Object-centric editing, where each timeline item owns duration, layer,
  parameters, filters, and motion.
- Direct numeric control for parameters, alongside sliders and visual controls.
- Midpoint/keyframe-style animation editing with explicit interpolation.
- Compact editing windows that prioritize repeated production work over
  marketing-style presentation.

Modernization wins when it removes structural limits:

- Cross-platform operation on Linux, Windows, and macOS.
- 64-bit memory, GPU acceleration, hardware-aware decoding/encoding, and
  responsive preview playback.
- Non-destructive editing, reliable undo/redo, autosave-friendly project data,
  and crash-resistant settings.
- Unicode-safe paths, high-DPI UI, localization, accessibility, and predictable
  keyboard behavior.
- Safe extension points through packages, LuaJIT plugins, QML, GLSL, and
  permission controls.

## Non-Goals

These are intentionally outside the first compatibility target:

- Binary compatibility with AviUtl, ExEdit, or their native plugin ABI.
- Perfect reproduction of every historical window size, color, menu label, or
  undocumented behavior.
- Preserving limitations caused by 32-bit memory, single-threaded rendering, or
  Windows-only APIs.
- Treating unfamiliar modern features as regressions when they improve safety,
  performance, or project portability.

## Required Familiarity Areas

### Project Launch and Windows

Users should be able to start with a project resolution, frame rate, and duration
without learning a new mental model. The preview, timeline, object settings,
system settings, project settings, and export surfaces should remain separate
enough to match AviUtl-style work, but they should be recoverable from menus and
shortcuts if hidden or moved.

Acceptance targets:

- New project setup exposes width, height, frame rate, and scene defaults before
  editing begins.
- The main preview and timeline can be shown, hidden, and refocused from the
  keyboard.
- Closing or quitting protects unsaved work across open project tabs.
- Window state should be persistent, but a broken layout must be resettable.

### Timeline Editing

The timeline is the center of the compatibility target. Users should recognize
layers, objects, drag placement, trimming, splitting, snapping, selection, and
right-click creation as AviUtl-like operations.

Acceptance targets:

- Time is represented in frames first; timecode can supplement it but should not
  obscure frame-accurate work.
- Layers are stable editing lanes. Locking, visibility, and layer-specific
  behavior must not unexpectedly rewrite object timing.
- Dragging an object should preserve duration and layer intent unless the user
  explicitly trims, snaps, groups, or performs a batch operation.
- Multi-selection should behave predictably for move, delete, copy, cut, paste,
  split, and property inspection.
- Snapping should be visible, overridable, and configurable per scene or project.
- The playhead and skimmer/edit target should have clear rules so menu commands
  create or paste at the expected frame and layer.
- Large timelines must remain scrollable and zoomable without forcing users to
  change editing style.

### Objects, Effects, and Filters

AviQtl-Plus should preserve the AviUtl habit of building a scene from objects
and stacking effects or filters on them. The modern system may use JSON, QML,
GLSL, and packages internally, but users should experience it as a searchable
object/effect catalog with consistent parameter editing.

Acceptance targets:

- Built-in objects cover common AviUtl-style editing blocks: media, text, shape,
  scene reference, audio, camera/control objects, and utility objects.
- Effects and transitions are searchable by localized name and category.
- Each object or effect exposes stable defaults, a compact parameter UI, and
  resettable values.
- Effect order is visible and editable.
- Disabled effects stay in the stack without changing rendered output.
- Third-party packages should look and behave like built-in entries after
  installation, while still showing source, version, and permissions.

### Parameter and Keyframe Editing

AviUtl users expect numerical precision and fast animation control. AviQtl-Plus
should keep that precision while adding clearer interpolation, validation, and
preview feedback.

Acceptance targets:

- Numeric parameters support direct typing, step controls, sliders when useful,
  and units where relevant.
- Color, file, enum, boolean, text, and grouped controls use consistent widgets
  across built-in and package-provided effects.
- A parameter with keyframes should make its animated state obvious in the
  object settings UI.
- Keyframes can be added at the current edit target, edited numerically, removed,
  and previewed at neighboring frames.
- Interpolation choices should be explicit, inspectable, and serializable in the
  project file.
- Copying or duplicating an object must preserve its parameter and keyframe data.

### Preview and Playback

Preview must feel immediate enough for everyday editing. If full-quality preview
is too expensive, the editor should degrade intentionally rather than becoming
unresponsive.

Acceptance targets:

- Space toggles playback when text input is not focused.
- Seeking, skimming, and object parameter changes update the preview with low
  latency.
- Preview rendering should not block basic timeline interaction.
- Audio playback, meters, and waveforms should remain synchronized with frame
  position.
- Quality/performance settings should be understandable without requiring users
  to know renderer internals.

### Import, Save, and Export

AviQtl-Plus should make modern media workflows more reliable than classic
AviUtl, especially around Unicode paths, long projects, audio, and codecs.

Acceptance targets:

- Drag-and-drop import validates file URLs and rejects unsupported schemes
  safely.
- Project files preserve scenes, clips, layer state, effects, keyframes, plugin
  state, and package references.
- Missing media should be reported with enough context to relink or replace it.
- Export exposes practical presets first, with advanced codec controls available
  when needed.
- Hardware encoders may be offered, but a software fallback must remain clear.
- Export failures should name the failed stage and leave partial output in a
  predictable state.

### Shortcuts and Menus

Keyboard habits are part of the AviUtl experience. AviQtl-Plus should keep common
editing commands fast while allowing modern shortcut customization.

Acceptance targets:

- Core commands have defaults for new, open, save, save as, export, undo, redo,
  playback, timeline/object-settings visibility, split, copy, cut, paste, delete,
  and project settings.
- Shortcut editing validates conflicts and supports restoring defaults.
- Context menus should expose the same command vocabulary as keyboard actions,
  so users can discover commands before memorizing shortcuts.
- Text input fields must not accidentally trigger global timeline commands.

## Modern Editor Requirements

The following requirements are not optional polish; they are what let the
AviUtl-like model survive on current systems.

### Stability and Data Safety

- Settings and project saves should be atomic or recoverable.
- Undo/redo must describe user actions, not implementation events.
- Background decoding, plugin scanning, export, and preview work must not create
  data races with UI editing.
- Long media and large timelines should fail gracefully when memory pressure is
  unavoidable.

### Performance

- Timeline scrolling, zooming, and selection should stay interactive on large
  projects.
- Audio decoding should be streaming or chunked for long files.
- Frame caches should have explicit limits and eviction behavior.
- GPU effects should have predictable fallbacks or clear unsupported-state
  messaging.
- Performance regressions should be measured with repeatable scenarios, not only
  subjective preview checks.

### Extensibility

- Built-in and package-provided effects should share one metadata model.
- Plugin permissions should default to least privilege.
- Package installation should validate IDs, paths, versions, and archives before
  deployment.
- Extension APIs should prefer stable, documented operations over direct access
  to internal UI objects.

### Localization and Accessibility

- User-facing strings should be localizable.
- Numeric input should remain unambiguous across locales.
- High-DPI displays should not change editing precision.
- Important state should not be communicated by color alone.
- Keyboard navigation should cover repeated editing workflows, not only dialogs.

## Contribution Checklist

Use this checklist for changes that affect editing behavior:

- Does the change preserve frame-accurate layer/object editing?
- Does it make the result of copy, paste, split, delete, undo, or redo more
  predictable?
- Does it keep direct numeric editing available when adding a visual control?
- Does it clarify how the playhead, skimmer, or selected layer affects the
  command?
- Does it avoid blocking timeline interaction during decoding, rendering,
  scanning, or exporting?
- Does it preserve project serialization and reopen behavior?
- Does it include or update tests for the behavior being changed?
- Does it update user or developer documentation when the workflow changes?

## Suggested Roadmap Order

1. Document and verify the end-to-end daily editing path: create a project,
   import media, place objects, adjust parameters, add keyframes, preview, export,
   save, and reopen.
2. Harden timeline ergonomics: selection, skimming/edit targets, snapping,
   multi-object operations, and large-project interaction.
3. Productize the object/effect catalog: categories, search, presets, defaults,
   localization, and package provenance.
4. Add workflow-level tests around project serialization, import, keyframes,
   export configuration, and reopen behavior.
5. Treat performance work as measured scenarios with named project sizes and
   media durations.

## Verification Guidance

A change should not claim to improve AviUtl familiarity unless it can be checked
against at least one user workflow. Good evidence includes:

- A unit or integration test for the edited model behavior.
- A reproducible manual scenario with exact project settings, commands, and
  expected result.
- Before/after measurements for preview, timeline, audio, or export performance.
- Documentation updates that explain a changed workflow or intentional departure
  from AviUtl behavior.

When compatibility and modernization conflict, record the decision in the pull
request or a follow-up document. The preferred answer is usually to preserve the
editing habit, modernize the implementation, and expose the new behavior only
where it gives users a clear advantage.
