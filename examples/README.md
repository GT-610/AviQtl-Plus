# AviQtl-Plus Examples

Complete, self-contained examples demonstrating each extension type.

## Index

| Example | Type | Demonstrates |
|---------|------|-------------|
| [sepia_effect](sepia_effect/) | Fragment + Compute effect | Both shader paths, keyframe params |
| [stylize_effect](stylize_effect/) | Fragment effect | Minimum viable effect template |
| [compute_blur](compute_blur/) | Compute effect | Multi-pass, `passIndex` injection |
| [custom_object](custom_object/) | Object | `BaseObject`, scene graph integration |
| [custom_transition](custom_transition/) | Transition | `previousScene`/`nextScene` animation |
| [hello_mod](hello_mod/) | Lua plugin | Lifecycle hooks, APIs, permissions |

## Quick Start

### Effect (Fragment Shader)

1. Copy `stylize_effect/` to `<data>/effects/posterize/`
2. Compile shader: `qsb --glsl 100es,120,150 --hlsl 50 --msl 12 posterize.frag -o posterize.frag.qsb`
3. Restart AviQtl

### Effect (Compute Shader)

1. Copy `compute_blur/` to `<data>/effects/compute_blur/`
2. Compile shader: `qsb --glsl 310es,430 --hlsl 50 --msl 12 compute_blur.comp -o compute_blur.comp.qsb`
3. Restart AviQtl

### Object

1. Copy `custom_object/` to `<data>/objects/color_chip/`
2. Restart AviQtl

### Transition

1. Copy `custom_transition/` to `<data>/transitions/zoom_transition/`
2. Restart AviQtl

### Lua Plugin

1. Copy `hello_mod/` to `<data>/plugins/hello_mod/`
2. Restart AviQtl
3. Open Lua console, type `hello()`

## Extension Types

| Type | Files | Base Class | Runtime |
|------|-------|-----------|---------|
| Effect | JSON + QML + GLSL | `BaseEffect` / `BaseComputeEffect` | QML + QRhi |
| Object | JSON + QML | `BaseObject` | QML + QtQuick3D |
| Transition | JSON + QML | `Item` | QML |
| Mod | `manifest.lua` + `main.lua` | — | LuaJIT |

## Documentation

- [EFFECT_SCHEMA.md](../docs/effects/EFFECT_SCHEMA.md) — JSON schema reference
- [PLUGIN_SYSTEM_DESIGN.md](../docs/plugins/PLUGIN_SYSTEM_DESIGN.md) — Architecture
- [DEVELOPER_GUIDE.md](../docs/plugins/DEVELOPER_GUIDE.md) — Plugin development
- [API_REFERENCE.md](../docs/plugins/API_REFERENCE.md) — Lua API reference
