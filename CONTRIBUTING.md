# Contributing to AviQtl-Plus

Thank you for your interest in contributing!

## Getting Started

1. Fork the repository
2. Clone your fork and create a branch from `main`
3. Build the project using `python3 BUILD.py` (see the [build documentation](https://aviqtl.gt610.dpdns.org/developer/building) for platform-specific instructions)
4. Make your changes
5. Run the checks below for the frontends affected by your changes.
6. Submit a pull request

## Development Environment

- **C++ Standard**: C++23
- **Rust**: the version pinned in `rust-toolchain.toml`
- **Default frontend**: Rust, Slint, wgpu
- **Qt**: 6.x (Qt Quick + QRhi)
- **Build**: CMake 3.21+, Ninja
- **Dependencies**: FFmpeg, LuaJIT, Carla, Vulkan

## Code Style

- **C++**: Follow `.clang-format` and `.clang-tidy` configurations in the repo root
- **QML**: Match existing patterns in `ui/qml/`
- **Commits**: Use concise, descriptive messages

## Project Structure

```
core/          - FFmpeg decoders, document model, effect registry, settings
rust/          - Rust workspace for native, Qt-independent core logic
engine/        - Audio mixer, audio plugins, ECS system, keyframe evaluation
scripting/     - LuaJIT host, mod engine
ui/            - Qt Quick controllers, QML views, undo/redo commands
effect-packages/ - External effect packages
tests/         - Unit tests (CTest)
```

## Adding Effects or Objects

Each effect or object consists of:
- A `.json` metadata file (id, name, params, UI controls)
- A `.qml` component file
- Optionally a `.frag` or `.comp` shader file

See the [effects and objects documentation](https://aviqtl.gt610.dpdns.org/developer/effects) for the extension model.

## Adding Tests

Rust tests live next to their implementations and under `rust/aviqtl-core/tests`.
Run them from the repository root with the platform's native dependency environment loaded:

```sh
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --locked --workspace --all-targets --all-features -- -D warnings
cargo test --manifest-path rust/Cargo.toml --locked --workspace --all-features
python -m unittest tests.test_build_version
python tests/test_rust_abi_contract.py
python tests/check_rust_abi.py --rust-src rust/aviqtl-core/src --header core/include/rust_core_abi.hpp
```

On Windows, use the MSVC developer environment and the same `FFMPEG_DIR`,
`LIBCLANG_PATH`, and runtime DLL paths configured by `BUILD.py --msvc`.
An existing packaged executable alone does not provide the headers needed by Cargo.

The following checks require additional runtime capabilities and are ignored by
the default Rust test command. Run each separately so a missing device or plugin
is reported explicitly:

```sh
# FFmpeg CLI on PATH; linked FFmpeg must provide libx264 and aac.
cargo test --manifest-path rust/Cargo.toml --locked -p aviqtl-media -p aviqtl-audio -- --ignored
# A graphics adapter; the preview tests enable DX12, Vulkan, or Metal by platform.
cargo test --manifest-path rust/Cargo.toml --locked -p aviqtl-preview scaled_msaa_preview -- --ignored
# Installed Carla libraries; the plugin-processing test also needs AVIQTL_CARLA_TEST_LADSPA.
cargo test --manifest-path rust/Cargo.toml --locked -p aviqtl-carla -- --ignored
```

Qt tests remain in `tests/` and use Qt Test. Build the application **and the test
targets**, then run CTest in the CMake build directory, not in the packaged `build/`
directory. For example, after configuring the Qt dependencies on Linux:

```sh
cmake -S . -B .build_tmp/qt-check -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build .build_tmp/qt-check --parallel 2
ctest --test-dir .build_tmp/qt-check --output-on-failure
```

When using `BUILD.py --frontend qt`, the CMake directory is
`.build_tmp/<target>/<Config>/qt-build`. Run `cmake --build` there without
`--target AviQtl` to build the tests as well. PR and release checks exercise both
frontends; the Linux Rust job explicitly runs the FFmpeg integration tests.
Carla/GPU tests without their dependencies remain unverified, not passed.

Register new Qt tests in `tests/CMakeLists.txt`:

```cmake
aviqtl_add_test(
    NAME my_test
    SOURCES test_my_test.cpp
    LINKS AviQtl_Core  # or AviQtl_UI, AviQtl_Engine
)
```

## Reporting Issues

Please report bugs via [GitHub Issues](https://github.com/GT-610/AviQtl-Plus/issues).

## License

By contributing, you agree that your contributions will be licensed under the AGPLv3.
