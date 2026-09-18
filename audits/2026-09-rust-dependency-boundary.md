# Rust application dependency boundary

Inspected against `22fd7c3` on 2026-09-18, before the frontend module split.
This is a source/build dependency inventory, not a GUI acceptance result.

## Default executable

`BUILD.py` defaults to `frontend="slint"` and invokes Cargo with
`--package aviqtl-slint`. Only `--frontend qt` invokes the root CMake build.
Cargo metadata lists nine workspace packages; their targets are Rust libraries,
the Slint executable, Rust integration tests, and the Slint build script.
No workspace build script compiles the project's C/C++ sources.

The default application uses the Rust APIs of `aviqtl-app`, `aviqtl-preview`,
`aviqtl-export`, `aviqtl-audio`, `aviqtl-media`, `aviqtl-render`, and
`aviqtl-rust-core`. `aviqtl-carla` provides the external Carla boundary.
The core's C ABI/static library remains available for the retained Qt frontend;
the Rust application consumes its Rust library API instead.

## Dependencies that remain intentional

| Dependency | Consumer and purpose | Retirement implication |
| --- | --- | --- |
| FFmpeg | `aviqtl-media`, through `ffmpeg-the-third`, decodes and encodes media | External native dependency; not unfinished migration of application C++ |
| Carla | `aviqtl-carla` dynamically loads external libraries; application/audio code discovers and hosts plugins | Keep the native plugin boundary and discovery tools |
| CLAP/VST3 hosts | `aviqtl-audio` uses `truce-rack` | Keep plugin ABI interoperability |
| Platform graphics/audio/dialog APIs | wgpu, Slint/winit, CPAL, rfd | Keep platform integration; Rust ownership does not imply a native-library-free binary |
| Script VM | `aviqtl-core/src/script_runtime.rs` uses `luna-core`; `aviqtl-app/src/mod_host.rs` owns host integration | The default frontend does not use the old C++ LuaJIT host |

## Shared resources still located beside Qt code

- `ui/qml/effects` and `ui/qml/objects`: `EffectCatalog` loads metadata from
  resource roots supplied by `aviqtl-app/src/settings.rs`. Development resolves
  these source directories, and `BUILD.py` copies them into packaged resources.
  A directory name containing `qml` does not mean Rust executes QML.
- `ui/resources/remixicon.ttf`: embedded by `aviqtl-render/src/text.rs`.
- `i18n/AviQtl_en_US.ts` and `i18n/AviQtl_zh_CN.ts`: read by the Slint build script
  to generate effect metadata translations. Slint UI translations separately
  use its gettext catalogs.
- `plugins`, `effect-packages`, and `repos`: shared packaged runtime data.
- Rust catalog tests read the shipped Qt-era metadata to check built-in effect
  and object inventory. Preserve equivalent inventory checks when moving it.

Do not delete `ui` or `i18n` wholesale when retiring Qt. First relocate shared
resources and update compile-time includes, development lookup, packaging,
translation extraction, and tests in the same change.

## Retained implementation

`core`, `engine`, `scripting`, the C++/QML frontend under `ui`, root
`CMakeLists.txt`, C++ tests, and Qt CI remain the optional Qt application and its
validation path. Their continued presence is not a dependency of the default
executable. The Rust C ABI remains required while this consumer exists.

The application is unreleased and has no external extension migration
requirement. Native WGSL packages are the Rust extension contract; preserving
the old QML package runtime is not a retirement gate.

Actual desktop operation is deferred to a separate session. This inventory
does not authorize marking Qt behavior parity or hardware output as verified.
