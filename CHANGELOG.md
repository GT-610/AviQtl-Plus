# Changelog

All notable changes to AviQtl-Plus are documented in this file.

## [Unreleased]

### Added
- The batch 4 editing model now has its interface. The timeline carries a docked
  **Media Library** with source in/out points, flat groups, search, insert, and
  overwrite; clips show draggable object midpoints; ripple, roll, slide, and slip
  are one-shot tools in the scene strip with a live outline and a slip source-in
  readout; and a multiple selection shows each object property as uniform,
  mixed, or absent. A mixed parameter edit applies to every selected object that
  has it, skips objects that do not, treats locked layers as skipped, and remains
  one undo step. The library's overwrite replaces exactly the covered range,
  splitting a clip that merely overlaps it so the part outside the range
  survives, refuses a locked layer before changing anything, and every bin
  insert honours the stored source range.
- The application takes a project path on the command line, so a file manager, a
  shell, or a `open`/`xdg-open` hand-off lands on the project instead of on an
  empty launcher. `--help` prints the options and `--version` prints the version
  instead of both being rejected as unknown arguments

### Fixed
- An object whose `transform` effect omits a parameter - `scale`, `opacity`, a
  rotation - keeps AviUtl's value for it instead of falling to zero. A transform
  that carried only its offsets used to draw the object at scale 0, so a project
  made only of shapes and generated objects previewed and exported a flat frame,
  and an export looked broken until some video or image happened to fill the frame
- The menu bar belongs to the focused window, so the timeline window now declares
  the same File/Edit/Settings/View/Tools bar as the preview window. Previously the
  menus disappeared while the timeline was key, which is exactly when the editor is
  being used. Every item runs the command the preview window already owns, the
  accelerators step aside while a popup holds a text field, and the project items
  grey out with the same conditions on both windows
- The status line speaks the interface language. A status is one sentence in
  English, Simplified Chinese, and Japanese, composed next to the command that
  produces it instead of in the window that shows it, so `Ready`, `Edit applied`,
  the split refusal, and 71 further sentences no longer appear in English inside
  a translated interface. Text that comes from elsewhere, such as an error from the
  core, is still reported unchanged
- A newly inserted text object starts with contents in the interface language
  instead of AviUtl's Japanese `テキスト`; the object metadata keeps AviUtl's
  default, which a script insert and a project opened from AviUtl still use
- A recovery snapshot that stops being written is reported in the status bar once
  per distinct failure instead of only in a window that is not on screen while a
  project is open. The warning is also withdrawn once the snapshot is written
  again, and it is compared by the whole failure list, so a changed second failure
  is not mistaken for a repeat
- The menu bar's accelerators stay out of the way on the timeline while the zoom
  box or one of the layer dialogs holds the keyboard, so Ctrl/Cmd+Z edits that
  field instead of undoing the project
- A layout command from the timeline menu acts on the timeline's screen, so
  "Bring editor windows onto this screen" no longer pulls the editor to the
  preview window's monitor when the two are on different displays
- Window layouts are applied in physical pixels computed from one monitor's
  scale, so a window that lands on a display with a different scaling factor
  keeps the size and place the layout gave it instead of being doubled or halved.
  A saved layout is captured in that same scale, so restoring it on another
  display keeps its proportions too
- The timeline refuses to split a selection it cannot split with a translated
  sentence rather than an English one

## [0.7.0-rc.1] - 2026-10-22

### Added
- The default application is the Rust + Slint + wgpu frontend. Nine workspace
  crates own application commands, project and timeline state, persistence and
  recovery, preview, rendering, export, audio, Carla hosting, media, and desktop
  presentation, and `BUILD.py` builds and packages it for Linux, macOS, and
  Windows MSVC
- Native WGSL effect and object packages are the extension contract for this
  frontend: shader resources are validated before use, one pipeline is kept
  across frames, and the bundled weather objects and color tint effect are
  written against it and installed through the package manager
- Object midpoints, a media bin, multi-object property editing that reports each
  property as uniform, mixed, or absent, and the ripple, roll, slide, and slip
  trim tools of the batch 4 editing model
- A block of clips can be saved as a fragment (`.aviqtlfrag`) and inserted into
  another scene or project; the format keeps every clip, effect, and parameter
  exactly as this build stores them, so nothing is translated or dropped, and an
  insert that would land on existing objects moves forward instead of overlapping
- AviUtl ExEdit object files (`.exo`) can be imported into the current scene,
  dropped onto the timeline at the pointer, or used to start a new project whose
  settings come from the file. They are read only: writing one would have to drop
  whatever AviUtl cannot express, so fragments are the way work is saved
- The import covers text, image, video, audio, the drawing/transform filter, the
  built-in figure types, and the built-in effects whose parameter meanings are
  confirmed
- Object files are decoded in the code page they were written in, so a file from
  a Chinese pack and one from stock Japanese ExEdit both read correctly
- An import reports what it translated and what it preserved, and a preserved
  script-backed effect names the script it needs
- Importing an object file that would land on occupied frames moves the whole
  block forward and reports it, instead of leaving two objects sharing a layer
  and a frame
- Unmapped ExEdit objects and filters are imported as disabled placeholders that
  keep their names, script parameters, track values, and binary data
- A scene holds up to 512 timeline layers
- Timeline and clip context menus carry a searchable object, effect, and
  audio-plugin catalog, effects have favorites and recent selections, shortcuts
  can be recorded with conflicting bindings rejected, and workspace layouts are
  saved and restored to a position that is on screen
- Text previews while it is edited, a numeric parameter can be scrubbed
  reversibly with one undo step per drag, snapping and fit navigation are exposed
  in the timeline, the launcher lists recent projects, scene boundaries are drawn
  as grid lines, and audio monitoring shows waveform feedback
- Startup lists the graphics adapters it found and refuses a CPU-only adapter with
  an explanation instead of crashing; the standalone `no-gpu-probes` crates keep
  the measurements that decision rests on
- `--validate-gpu` renders a fixed number of frames, reports which backend ran,
  and fails on a recorded wgpu error, so a packaged build can be smoke tested
  without a person at the keyboard

### Changed
- Qt/C++ and QML become an optional behavior reference. `python3 BUILD.py
  --frontend qt` still builds it and it still consumes the shared Rust C ABI, but
  it is no longer compiled into, or shipped with, the application
- Clipboard, duplicate, split, trim, and layer operations preview and commit
  through the same Rust planner, so a drag cannot show geometry the commit will
  not apply
- A continuous parameter edit is one undo step, Ctrl/Command-click selects a
  layer's contents instead of a single clip, and menus, effect selection, and
  window layouts gained the controls recorded in the parity gate
- Lua plugins run on a sandboxed Lua 5.1 VM instead of LuaJIT, and `load`,
  `loadstring`, `loadfile`, `dofile`, `print`, `require`, `module`, `io`, `os`,
  `debug`, `package`, and `ffi` are removed from the plugin globals, so a plugin
  that used LuaJIT extensions has to be adapted
- The Windows MSVC jobs key the vcpkg binary cache on a fingerprint of the tools
  that feed vcpkg's ABI hash, and key the Rust caches on `Cargo.lock` and
  `rust-toolchain.toml`, so a runner image update repopulates them instead of
  rebuilding FFmpeg on every run

### Fixed
- Project saves publish through a unique synchronized temporary file, so an
  interrupted write cannot leave a half-written project where a complete one was
- Effect metadata, shader sources, and native package metadata are bounded before
  they are read, and extracted archive entries are rejected when their names or
  paths would escape the destination
- Preview rendering keeps frame results and decode resources bounded and releases
  inactive decoders instead of accumulating them
- The live parameter preview refreshes the parameter under the pointer instead of
  the first numeric parameter in the stack, and repainting a control no longer
  cancels the binding that a later undo or preset load has to reach
- Overlays take focus and answer Escape, and the save confirmation answers Return
  as Qt's dialog does; a right press on a trim handle still opens the clip menu
- Refused commands, completed edits, and the result of an audio-plugin scan are
  visible in the status bar instead of being projected into no element
- Ruler ticks follow the zoom level, a layer header toggles visibility on a left
  click, and context menus keep the Qt category order and localized names
- Export clamps its range to the timeline duration instead of planning past it
- Bundled MOD examples pass the permission checks they declare, and a background
  audio, discovery, or package worker that disconnects no longer stalls the UI
- The macOS bundle writes a numeric `CFBundleShortVersionString` and
  `CFBundleVersion`, so a candidate tag cannot put an invalid version into
  `Info.plist`

### Removed
- QML extension packages are rejected by the package validator; the native WGSL
  package contract replaces them
- The Slint-only timeline toolbar strip and the hint line under the scene tabs,
  which had no Qt equivalent, so the ruler now sits directly below the tabs
- Duplicated native policy paths, unused Qt restoration helpers, and test-only
  wrappers over business state

## [0.6.2] - 2026-09-05

### Added
- Rust-owned scene bake plans, export and audio planning, runtime settings,
  plugin permissions, package, effect, and script-plugin catalogs, and recovery
  metadata
- Automated Rust C ABI consistency checks and focused coverage for the new
  authoritative core states

### Changed
- Rust timeline state is now authoritative for scene, clip, effect, keyframe,
  and audio-plugin edits, including undo and redo transaction replay
- Qt timeline objects and the native ECS are maintained as projections of the
  committed Rust state instead of independent business-state owners
- Keyframe evaluation, package catalog queries, audio playback, waveform
  generation, video caching, and composite rendering reuse cached or batched data
- Obsolete migration interfaces and duplicated native policy paths were removed

### Fixed
- Preview and exported video frames render deployed objects correctly after the
  transform-pipeline optimization, including objects added after window creation
- Untrusted project, package, plugin, recovery, and shader inputs are bounded and
  validated before reaching native resources
- Recovery generations, package deployment rollback, timeline projection rollback,
  and cross-thread audio/video state updates preserve atomicity and ownership
- Export now rejects invalid image-sequence and audio-planning states instead of
  silently producing incomplete output
- Localized playback-mode values and new package/export errors are normalized and
  translated consistently
- macOS release archives identify the architecture they actually contain and
  verify the packaged app signature before archiving

## [0.6.1] - 2026-08-19

### Changed
- Timeline scene, clip, effect, keyframe, and audio-plugin edits now use
  targeted, reversible Rust state transactions
- Qt timeline objects are reconciled from the committed Rust state after each
  transaction, keeping the native projection aligned with the core
- Optional weather objects are externalized from the main effect packages

### Fixed
- A failed Qt projection synchronization now rolls back both the Rust
  transaction and native timeline mutation atomically
- Scene removal undo restores the original scene position, and batch clip
  geometry edits reject partial application
- Audio duration changes and effect/keyframe updates consistently use the
  committed Rust timeline state

## [0.6.0] - 2026-08-16

### Added
- Portable Rust core workspace with a stable C ABI for project documents,
  settings, presets, packages, effects, scripts, plugins, and keyframes
- Rust timeline planning and baking for editing, selection, snapping, layer
  operations, duration constraints, ID allocation, render data, and audio data
- Rust audio DSP for resampling, mixing, metering, and batch-mix policy
- Reproducible native MSVC builds with committed vcpkg manifests and triplets,
  complete runtime deployment, and dedicated Windows CI coverage
- Opt-in representative video and audio performance workloads with repeatable
  startup, seek, waveform, cache, and memory diagnostics
- Audio plugin scan coverage for large directories, repeated scans, and
  malformed discovery output
- Observable bounded-cache statistics for streaming audio decoding and waveform
  generation
- Visible snap-frame feedback for timeline moves, trims, and media drops

### Changed
- Platform-neutral domain rules now live in the Rust core, while Qt/C++ remains
  responsible for UI, native integration, and framework object lifetimes
- The Windows release artifact is now built with MSVC instead of MSYS2/UCRT64
- Rust is pinned to version 1.97.1 with edition 2024 across local and CI builds
- Windows QML composition capture tests use the OpenGL RHI backend for
  repeatable headless frame capture and encoded-video verification
- Audio decoder test media is generated in buffered blocks, keeping longer
  workload fixtures inexpensive
- Removed the incomplete transition catalog, package type, examples, and
  placeholder assets until transitions are implemented in the real timeline
  and frame-driven composition pipeline

### Fixed
- MSVC packages now include the required Qt, FFmpeg, LuaJIT, Vulkan, and Carla
  runtime libraries without bundling host-only vcpkg tools
- Windows CTest entries now configure their Qt, vcpkg, and Carla runtime paths
  independently of the invoking shell environment
- Relative audio plugin search paths now scan the resolved application-relative
  directory instead of the process working directory
- Main-window split commands now apply to the complete clip selection
- Video encoding now queries supported pixel and sample formats through the
  API required by FFmpeg 8 and libavcodec 63

## [0.5.8] - 2026-07-23

### Added
- Green release baseline checks for canonical versioning and tag consistency
- Independent autosave snapshots and startup crash recovery for multiple
  projects
- Recovery coverage for formal saves, discarded projects, corrupt snapshots,
  stale data, and orphaned snapshot cleanup

### Changed
- Local and manually dispatched builds now use the canonical CMake project
  version instead of reporting `0.0.0`
- Tagged release builds reject tags that do not match the project version
- Qt Test output is explicitly routed through CTest on all platforms
- Periodic recovery serialization and disk writes now run outside the UI thread
- Project-level resolution, frame-rate, and sample-rate changes now mark the
  project as unsaved

### Fixed
- Preset path containment checks now handle Windows path separators correctly

## [0.5.7] - 2026-07-22

### Added
- Unified, searchable object and effect catalog with package provenance
- Daily editing reliability coverage for project editing workflows

### Fixed
- macOS QML composite test asset deployment

## [0.5.6] - 2026-07-19

### Added
- Large-timeline performance and QML viewport virtualization fixtures
- Encoded QML export, shader rendering, and video decoder round-trip coverage
- Observable GOP and bounded frame-cache eviction coverage

### Changed
- Continuous timeline zoom now reuses the loaded viewport range
- Test suite labels and coverage were streamlined

## [0.5.5] - 2026-07-13

### Added
- Animated text and real `CompositeView` capture tests

### Changed
- Removed dead code and hardened package quality checks

## [0.5.4] - 2026-07-12

### Added
- Atomic project saves and missing-media recovery workflow
- Hardened timeline batch movement and export output integrity

### Fixed
- Linux headless test startup and rendering/audio correctness issues

## [0.5.3] - 2026-07-08

### Added
- AviUtl operability and timeline edit-target specifications
- Daily editing workflow coverage and catalog provenance metadata

### Changed
- Export failures now identify configuration, capture, and encoding stages
- Windows and macOS release packages were reduced in size

## [0.5.2] - 2026-07-05

### Added
- Timeline skimmer edit target
- Chunked audio decoding and waveform peaks

### Changed
- Security, correctness, memory, and thread-safety hardening

## [0.5.1] - 2026-06-30

### Fixed
- macOS CI test failures

## [0.5.0] - 2026-06-30

### Added
- Installable effect packages and expanded plugin examples
- Advanced package management and effect registry/package tests
- Undoable audio plugin changes and configurable undo history
- Dissolve, Push, Zoom, and Wipe transitions
- CONTRIBUTING.md and CHANGELOG.md
- macOS and Windows test steps in the release workflow

### Fixed
- Wipe compositing between previous and next scenes
- Push transition reverse-direction positioning

### Removed
- Duplicate packaged transitions in favor of built-in transitions
- Empty plugin placeholder directory

## [0.4.0] - 2026-06-24

### Added
- GPU compute shader effects (SRB pre-allocation, dirty flags, separable blur, bitonic sort)
- Encoder discovery with preset/profile selection
- BorderBlur O(n²) → O(2n) optimization with edge-detect blend
- Multi-pass ping-pong correctness for 3+ pass dispatches

### Fixed
- macOS packaging: Carla-discovery-native Homebrew RPATH

## [0.3.2] - 2026-06-22

### Fixed
- macOS build: replace std::atomic<shared_ptr> with mutex-protected shared_ptr

## [0.3.0] - 2026-06-19

### Added
- Lua plugin system with permissions, scripting engine, and package manager
- Audio mixer with plugin persistence and workspace UI
- Effects for video objects
- Keyframe evaluation and media utility refactoring
- Unit tests for KeyframeUtils and MediaUtils

### Fixed
- Plugin startup parameter handling

## [0.2.0] - 2026-06-15

### Added
- Compute shader effects and effect presets
- ECS refactoring (removed CoreBridge singleton)
- Centralized media duration calculation
- libx264 fallback codec for video export

### Changed
- Project renamed to AviQtl-Plus
- New icons and splash screen

## [0.1.3] - 2026-06-12

### Added
- Audio-video linking and playback speed handling

## [0.1.0] - 2026-06-08

### Added
- Initial release
- Timeline editing with clips, effects, transitions
- Video/audio export with FFmpeg
- Cross-platform builds (Linux, Windows, macOS)
- CI/CD with automated releases
