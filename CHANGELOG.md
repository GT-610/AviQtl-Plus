# Changelog

All notable changes to AviQtl-Plus are documented in this file.

## [Unreleased]

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
