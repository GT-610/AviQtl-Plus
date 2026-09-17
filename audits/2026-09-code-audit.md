# September 2026 code audit

Baseline: `2aa57c0` on `main`. The audit covers the default Rust/Slint frontend,
the retained Qt frontend, their shared Rust core, scripting, media, and tests.
External dependencies and generated build products are excluded from cleanup.

## Removal evidence

References were checked in production source, tests, QML, Slint, Lua, examples,
documentation, and build registration. Public C ABI symbols and Qt meta-object
entry points are not classified as dead merely because a textual caller is absent.

| Removed code | Evidence and replacement |
| --- | --- |
| `ScriptRuntime::has_hook` | Only called by a test that already dispatches the same hook and checks its commands. |
| `frame_sample_range` | Obsolete audio timing implementation under `cfg(test)`; cumulative timing now exercises `plan_export_audio_frame`, and mixing uses explicit production sample counts. |
| `ScriptParamParser::parseHeader` | No callers; production uses `parse`. |
| `ModEngine::loadScriptParams` | Test-only wrapper; parser regression now calls the production parser. |
| `ModEngine::loadedPlugins` | Test-only projection of `pluginInfos`; duplicate assertions removed. |
| `ModEngine::getPluginParams` / `setPluginParam` | Only test callers. Despite `Q_INVOKABLE` tokens, `ModEngine` is not a QObject and has no meta-object exposure. Tests now verify persisted parameters through real plugin loading. |
| `PermissionManager::isPluginAuthorized` and `PermissionState::isAuthorized` | Wrapper chain with one test caller; tests assert actual permission revocation. The exported Rust C ABI remains intact. |
| Qt `restoreEffectInternal`, `restoreMultipleEffectsInternal`, `restoreAudioPluginStateInternal` | No callers; undo/redo uses retained Rust transactions and projection replacement. Their unused effect restoration helpers were also removed. |
| Qt `restoreSceneProjectionsInternal` / `removeSceneProjectionsInternal` | Unused wrappers; production scene commands use `replaceSceneProjectionsInternal`. |
| Qt `addClipsDirectInternal` and unused abort wrapper | Test-only business wrapper; fixtures and rollback tests now use the production projection transaction API. The no-argument transaction-ending wrapper became a default argument on the real implementation. |
| `ECSTimerScope`, `ECS_TIMER_SCOPE`, `ECS_PROF_ADD` | No callers, including profiling builds. Used ECS counters and `ECS_PROF_INC` remain. |

Test fixtures, fault injection, counter snapshots, and necessary observation
helpers remain. Dynamic QML entry points such as `getRenderState`,
`setClipProperty`, `toggleVisible`, and `grantAllPermissions` remain for compatibility.

## Behavior and resource changes

- Effect/package JSON ABI serialization and output-range validation share an
  implementation. Validation no longer allocates a temporary overlap vector;
  invalid ranges still take precedence over aliasing, capacity queries report
  required lengths, and failed writes leave output bytes untouched.
- Box selection uses hash membership while preserving the existing ordered
  result and primary selection. A 20,000-clip regression covers additive hits,
  duplicates, ordering, and an empty subsequent selection. No wall-time speedup
  percentage is claimed.
- Preview retains one completed result instead of an unbounded channel. Replaced
  batches release their frames; sequential export requests still each receive
  their completion. Reset reaches an idle worker and clears resource caches.
- Images use an LRU byte budget from the existing `cacheSize` setting. Oversized
  images remain renderable without being cached. Video decoders are retained
  only for paths used by the current scene, including nested scenes and masks.
- Both package frontends reject normalized duplicate paths and file/directory
  conflicts before extraction. Windows additionally rejects device names,
  alternate data streams, invalid characters, trailing dots/spaces, and case
  collisions. Unix filenames and valid internal `..` normalization are retained.
- Effect metadata and WGSL reads enforce their byte limits on an opened stream.
- Atomic saves use exclusive unique temporary files in the destination directory,
  flush and synchronize contents, and replace only after success. Publication is
  serialized within the process to avoid Windows simultaneous-replacement errors;
  preparation and file writes remain concurrent. Failure preserves previous
  files, document paths, and dirty state.

The project format, plugin protocols, C ABI layout/version, QML APIs, and default
frontend selection are unchanged. `MediaPreview::set_cache_size_mb` is the only
new cross-crate configuration method; it consumes an existing preference.

## Test corrections and coverage

Previously ignored FFmpeg tests exposed Windows fixture cleanup failures: they
deleted media before dropping the decoder/mixer holding the file open. Fixtures
now release those handles first. Preview GPU tests now enable a native backend
on Windows/Linux, as they already did on macOS; this affects test dependencies,
not the production graphics selection.

PR and release workflows reuse Rust checks, explicitly execute real FFmpeg
decoding/mixing/encoding tests, and build the Qt application and all CTest targets.
Python ABI checks run independently of Qt availability. Carla/GPU checks have
separate documented entry points and retain their explicit runtime requirements.

Local Windows validation:

- Baseline workspace: 374 passed, 7 ignored.
- Updated workspace with all features: 385 passed, 7 ignored.
- Explicit FFmpeg integration run: 4 passed (video/audio decoding, timeline mixing,
  and H.264/AAC encoding).
- Build-script tests: 11 passed; ABI checker tests: 6 passed; C ABI contract consistent.
- Rust formatting, Clippy with warnings denied, Git whitespace checks, and
  actionlint passed.
- `BUILD.py --msvc --offline` completed the optimized Slint build and packaged
  `build/AviQtl.exe` and `dist/AviQtl-MSVC-x86_64.zip` with the FFmpeg runtime.
- The Qt configure attempt found the MSVC compiler but no Qt6 SDK. Local Qt
  compilation/CTest could not run; the dedicated Linux CI installs those dependencies.
- The explicit GPU test found no usable DX12 adapter on this host. Hardware
  rendering remains unverified here. Carla libraries/test plugins are also absent.
- Packaged GUI validation reported only Microsoft Basic Render Driver and no
  GPU-backed adapter. Forcing software rendering exited with `0xc0000005`, also
  reproduced using the pre-existing package in `.build_tmp/msvc/archive-check`.
  Neither attempt is counted as successful GUI validation.

This is a call-site and behavioral audit, not an instrumented coverage report.
No line/branch coverage percentage or measured rendering speedup is claimed.
Remaining environment-specific validation includes hardware GPU rendering,
real Carla plugin processing, and macOS execution.
