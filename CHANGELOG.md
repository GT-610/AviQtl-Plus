# Changelog

All notable changes to AviQtl-Plus are documented in this file.

## [Unreleased]

### Added
- 4 new transitions: Dissolve, Push, Zoom, Wipe (7 total)
- CONTRIBUTING.md and CHANGELOG.md
- CI: test steps for macOS and Windows in build workflow

### Fixed
- Wipe transition now properly composites previous/next scenes
- Push transition reverse direction positioning

### Removed
- Duplicate effect-packages/transitions/ (built-in transitions only)
- Empty plugins/placeholder

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
