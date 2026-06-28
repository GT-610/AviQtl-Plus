# Contributing to AviQtl-Plus

Thank you for your interest in contributing!

## Getting Started

1. Fork the repository
2. Clone your fork and create a branch from `main`
3. Build the project using `python3 BUILD.py` (see README.md for platform-specific instructions)
4. Make your changes
5. Run tests: `cd build && ctest --output-on-failure`
6. Submit a pull request

## Development Environment

- **C++ Standard**: C++23
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
engine/        - Audio mixer, audio plugins, ECS system, keyframe evaluation
scripting/     - LuaJIT host, mod engine
ui/            - Qt Quick controllers, QML views, undo/redo commands
effect-packages/ - External effect packages
tests/         - Unit tests (CTest)
```

## Adding Effects, Objects, or Transitions

Each effect/object/transition consists of:
- A `.json` metadata file (id, name, params, UI controls)
- A `.qml` component file
- Optionally a `.frag` or `.comp` shader file

See `docs/effects/EFFECT_SCHEMA.md` for the complete reference.

## Adding Tests

Tests are in `tests/` and use Qt Test. Add your test file and register it in `tests/CMakeLists.txt`:

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
