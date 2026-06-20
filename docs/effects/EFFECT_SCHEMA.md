# Effect JSON Schema Reference

This document describes the JSON format for defining effects and objects in AviQtl-Plus.

## Overview

Effects and objects are defined as **JSON + QML + GLSL** triples:

| File | Role |
|------|------|
| `*.json` | Metadata, default parameters, UI controls |
| `*.qml` | QML component connecting parameters to the shader |
| `*.frag` / `*.comp` | GLSL shader (fragment or compute) |

Place files in:
- `ui/qml/effects/` for post-processing effects (blur, color correction, etc.)
- `ui/qml/objects/` for visual objects (text, shapes, media, etc.)

Third-party effects can be placed in any directory and loaded via the package manager.

---

## Root Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | Yes | Unique identifier (e.g. `"blur"`, `"my_effect"`) |
| `name` | string | Yes | Display name (localized at load time) |
| `qml` | string | Yes | QML file name or `qrc:` URL |
| `version` | string | Yes | Semantic version `x.y.z` |
| `kind` | string | Yes | `"effect"` or `"object"` |
| `categories` | string[] | Yes | At least one category (localized) |
| `params` | object | Yes | Default parameter key/value pairs |
| `ui` | object | Yes | UI definition with `controls` array |
| `color` | string | No | Hex color for timeline badge (e.g. `"#3b82f6"`) |

### Example

```json
{
  "id": "blur",
  "name": "ぼかし",
  "qml": "Blur.qml",
  "version": "1.0.0",
  "kind": "effect",
  "categories": ["ぼかし"],
  "params": {
    "size": 5,
    "quality": 1
  },
  "ui": {
    "controls": [
      { "type": "float", "param": "size", "label": "範囲", "min": 0, "max": 100 },
      { "type": "int", "param": "quality", "label": "品質", "min": 1, "max": 3 }
    ]
  }
}
```

---

## `kind` Field

| Value | Location | Purpose |
|-------|----------|---------|
| `"effect"` | `ui/qml/effects/` | Post-processing filters applied to layers |
| `"object"` | `ui/qml/objects/` | Self-contained visual entities (text, shapes, media) |

Both kinds share the same JSON schema. The `kind` field is used for UI filtering/grouping.

---

## `params` Object

A flat key-value map. Values must be JSON primitives (no nested objects or arrays).

| JSON Type | Example | Usage |
|-----------|---------|-------|
| number | `5`, `0.3`, `1.0` | Float/integer parameters |
| string | `"text"`, `"#ffffff"` | Text, colors, file paths |
| boolean | `true`, `false` | Toggles |

Multi-dimensional values are decomposed into separate scalars (e.g. `x`, `y` instead of vec2).

---

## `ui` Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `group` | string | No | `"effect"` or `"object"` (panel grouping) |
| `controls` | array | Yes | Array of control definitions |

---

## Control Types

### `float` / `number` / `slider` / `spinner`

Numeric floating-point input.

```json
{
  "type": "slider",
  "param": "opacity",
  "label": "Opacity",
  "min": 0.0,
  "max": 1.0,
  "step": 0.01,
  "decimals": 2,
  "unit": "%"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `param` | string | — | Key in `params` |
| `label` | string | — | Display label (localized) |
| `min` | number | -100000 | Minimum value |
| `max` | number | 100000 | Maximum value |
| `step` | number | — | Step increment |
| `decimals` | integer | 2 | Decimal places |
| `unit` | string | — | Unit suffix (localized) |

### `int` / `integer`

Integer input.

```json
{
  "type": "int",
  "param": "quality",
  "label": "Quality",
  "min": 1,
  "max": 10,
  "unit": "f"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `param` | string | — | Key in `params` |
| `label` | string | — | Display label (localized) |
| `min` | integer | -2147483648 | Minimum value |
| `max` | integer | 2147483647 | Maximum value |
| `unit` | string | — | Unit suffix (localized) |

### `bool` / `boolean`

Boolean toggle.

```json
{
  "type": "bool",
  "param": "reverse",
  "label": "Reverse"
}
```

### `color`

Color picker with hex field and dialog.

```json
{
  "type": "color",
  "param": "startColor",
  "label": "Start Color"
}
```

Values: `"#rrggbb"` (RGB) or `"#aarrggbb"` (ARGB).

### `string` / `text`

Multi-line text input.

```json
{
  "type": "string",
  "param": "text",
  "label": "Text Content"
}
```

### `path` / `file`

File path picker with dialog.

```json
{
  "type": "path",
  "param": "source",
  "label": "Source File",
  "filter": "Video Files (*.mp4 *.mov *.avi)"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `filter` | string | File dialog filter (localized) |

### `enum`

Dropdown with static options.

```json
{
  "type": "enum",
  "param": "direction",
  "label": "Direction",
  "options": ["Horizontal", "Vertical"]
}
```

Options can also use explicit value/label pairs:

```json
{
  "type": "enum",
  "param": "mode",
  "label": "Mode",
  "options": [
    { "value": "frame", "label": "Frames" },
    { "value": "time", "label": "Seconds" }
  ]
}
```

### `combo`

Dropdown bound to a live data model.

```json
{
  "type": "combo",
  "param": "targetSceneId",
  "label": "Target Scene",
  "source": "Workspace.currentTimeline",
  "sourceProperty": "scenes",
  "textRole": "name",
  "valueRole": "id",
  "excludeCurrentScene": true
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `source` | string | — | QML object path |
| `sourceProperty` | string | — | Property on source |
| `textRole` | string | `"name"` | Model role for display |
| `valueRole` | string | `"id"` | Model role for value |
| `excludeCurrentScene` | bool | false | Filter out current scene |

### `font`

Font family picker.

```json
{
  "type": "font",
  "param": "fontFamily",
  "label": "Font"
}
```

### `header`

Non-interactive section separator.

```json
{
  "type": "header",
  "label": "Advanced Settings"
}
```

---

## QML Path Resolution

1. **`qrc:` prefix**: Used as-is (e.g. `"qrc:/effects/MyEffect.qml"`)
2. **Relative path** (default): Resolved relative to the JSON file's directory
3. **Missing file**: Effect is skipped with a warning

---

## Localization

All user-visible strings are passed through `QCoreApplication::translate()` with context `"AviQtl::Core::EffectRegistry"`. This enables Qt's `.ts`/`.qm` translation system.

**Localized fields:**
- Root: `name`, `categories` entries
- Controls: `label`, `title`, `text`, `name`, `filter`, `placeholder`, `unit`
- Enum options: `label` in `{value, label}` pairs

---

## Fragment Shader Conventions (`*.frag`)

```glsl
#version 440
layout(location=0) in vec2 qt_TexCoord0;    // UV coordinates [0,1]
layout(location=0) out vec4 fragColor;       // Output color
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;      // Required by Qt Quick
    float qt_Opacity;     // Required by Qt Quick
    float myParam;        // Must match QML property name
    float targetWidth;    // Pass from QML: root.width
    float targetHeight;   // Pass from QML: root.height
};
layout(binding=1) uniform sampler2D source;  // Input texture

void main() {
    vec2 texel = vec2(1.0 / targetWidth, 1.0 / targetHeight);
    vec4 color = texture(source, qt_TexCoord0);
    // ... apply effect ...
    fragColor = result * qt_Opacity;
}
```

**Key points:**
- Uniform block members must match QML `ShaderEffect` property names exactly
- `qt_Matrix` and `qt_Opacity` are required by Qt Quick
- `source` at binding=1 is the input texture
- Coordinates: `qt_TexCoord0` is in [0,1], origin at top-left

---

## Compute Shader Conventions (`*.comp`)

```glsl
#version 430
layout(local_size_x = 8, local_size_y = 8) in;

layout(binding = 0, rgba8) uniform coherent image2D outImage;  // Output
layout(binding = 1) uniform sampler2D inTex;                    // Input
layout(std140, binding = 2) uniform Params {
    float myParam;      // Must match JSON param key (or uniformMapping)
    float time;         // Auto-injected by BaseComputeEffect
};
// Optional extra textures (binding 3, 4, 5, ...)
// layout(binding = 3) uniform sampler2D extraTex0;
// layout(binding = 4) uniform sampler2D extraTex1;

void main() {
    ivec2 gid = ivec2(gl_GlobalInvocationID.xy);
    ivec2 sz  = imageSize(outImage);
    if (gid.x >= sz.x || gid.y >= sz.y) return;  // Bounds check

    vec2 uv = (vec2(gid) + 0.5) / vec2(sz);       // Pixel-center UV
    vec4 color = texture(inTex, uv);
    // ... apply effect ...
    imageStore(outImage, gid, result);
}
```

**Key points:**
- Work group size: always `local_size_x = 8, local_size_y = 8`
- Binding 0: output image (RGBA8), binding 1: input texture, binding 2: params UBO
- Binding 3+: extra textures (set via `extraTextures` property in QML)
- Bounds check is mandatory
- `time` is auto-injected (current frame number)
- Uniform block member names must match JSON param keys (after `uniformMapping`)

---

## QML Structure

### Fragment Effect

```qml
import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    // Read parameters with evalNumber/evalParam/evalColor
    property real myParam: root.evalNumber("myParam", 1.0)

    ShaderEffect {
        property variant source: root.sourceProxy
        property real myParam: root.myParam
        property real targetWidth: root.width
        property real targetHeight: root.height
        anchors.fill: parent
        fragmentShader: "my_effect.frag.qsb"
    }
}
```

### Compute Effect

```qml
import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root
    computeShader: Qt.resolvedUrl("my_effect.comp.qsb")
    // Optional: uniformMapping: ({ "jsonKey": "glslMemberName" })
    // Optional: hdrOutput: true  // Use RGBA16F instead of RGBA8
    // Optional: extraTextures: [someItemWithLayer]  // binding 3, 4, ...
}
```

When using `extraTextures`, each entry must be a `QQuickItem` with
`layer.enabled: true`. The first extra texture is at binding 3, the second
at binding 4, and so on.

---

## Shader Compilation

Shaders are compiled at build time by CMake using `Qt6::qsb`:

```cmake
# Fragment shader
qt6_add_shaders(AviQtl "effect_shaders" FILES ui/qml/effects/my_effect.frag)

# Compute shader
qt6_add_shaders(AviQtl "compute_shaders" FILES ui/qml/effects/my_effect.comp)
```

Output: `*.qsb` multi-backend bundles (GLSL, HLSL, MSL, SPIR-V).

---

## Package Distribution

Third-party effects can be distributed as packages via the package manager:

1. Create a repository JSON with `"type": "effect"` or `"type": "object"`
2. Host the release on GitHub or Codeberg
3. Users install via the package manager UI

After installation, effects are hot-loaded from the deploy directory.

See `examples/sepia_effect/` for a complete working example.
