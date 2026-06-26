# AviQtl-Plus Effect Packages

This directory contains effect packages that demonstrate the extensibility of AviQtl-Plus's effect system.

## Purpose

1. **User Value**: Provide additional effects that can be installed via the package manager
2. **Developer Reference**: Serve as examples for creating custom effects
3. **Modularity**: Show how effects can be organized into independent packages

## Available Packages

| Package | Type | Contents | Description |
|---------|------|----------|-------------|
| [stylize-effects](stylize-effects/) | Effect | 7 effects | Creative stylize effects (glitch, pixel sorting, etc.) |
| [advanced-blur](advanced-blur/) | Effect | 4 effects | Professional blur effects (lens, radial, etc.) |
| [weather-objects](weather-objects/) | Object | 2 objects | Weather animations (rain, snow) |
| [transitions](transitions/) | Transition | 3 transitions | Basic transitions (fade, slide, wipe) |

## Quick Start

### For Users

1. Download a package folder
2. Open AviQtl-Plus → Settings → Package Manager
3. Click "Install from Local" and select the folder
4. Restart AviQtl

### For Developers

1. Study the package structure
2. Read the package's README.md
3. Create your own effect following the same pattern
4. See [EFFECT_SCHEMA.md](../docs/effects/EFFECT_SCHEMA.md) for complete reference

## Package Structure

Each package follows this structure:

```
package-name/
├── manifest.json          # Package metadata
├── README.md              # Package documentation
├── effect_id_1/           # Each effect in its own directory
│   ├── effect_id_1.json   # Effect definition
│   ├── EffectName.qml     # QML component
│   └── effect_id_1.frag   # GLSL shader
├── effect_id_2/
│   ├── effect_id_2.json
│   ├── EffectName.qml
│   └── effect_id_2.comp
└── ...
```

**Important**: Each effect/object/transition must be in a subdirectory named after its ID. The ID in the directory name must match the `id` field in the JSON file.

## Effect Types

| Type | Kind | Location | Base Class | Runtime |
|------|------|----------|-----------|---------|
| Effect | `"effect"` | `effects/` | `BaseEffect` / `BaseComputeEffect` | QML + QRhi |
| Object | `"object"` | `objects/` | `BaseObject` | QML + QtQuick3D |
| Transition | `"transition"` | `transitions/` | `Item` | QML |

## Creating Your Own Package

### 1. Choose a Type

- **Effect**: Visual filter applied to layers (blur, color correction, etc.)
- **Object**: Self-contained visual entity (text, shape, particle, etc.)
- **Transition**: Animation between scenes

### 2. Create the Structure

```
my-package/
├── manifest.json
├── README.md
└── my_effect/
    ├── my_effect.json
    ├── MyEffect.qml
    └── my_effect.frag
```

### 3. Define the Effect

Create a JSON file with:

```json
{
  "id": "my_effect",
  "name": "My Effect",
  "qml": "MyEffect.qml",
  "version": "1.0.0",
  "kind": "effect",
  "categories": ["My Category"],
  "params": {
    "intensity": 0.5
  },
  "ui": {
    "controls": [
      {
        "type": "slider",
        "param": "intensity",
        "label": "Intensity",
        "min": 0.0,
        "max": 1.0,
        "step": 0.01
      }
    ]
  }
}
```

### 4. Create the QML

For effects:

```qml
import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root
    property real intensity: root.evalNumber("intensity", 0.5)

    ShaderEffect {
        property variant source: root.sourceProxy
        property real intensity: root.intensity
        property real targetWidth: root.width
        property real targetHeight: root.height
        anchors.fill: parent
        fragmentShader: "my_effect.frag.qsb"
    }
}
```

### 5. Write the Shader

Fragment shader:

```glsl
#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    float intensity;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 color = texture(source, qt_TexCoord0);
    // Apply your effect here
    fragColor = color * qt_Opacity;
}
```

### 6. Test

1. Place your package in the AviQtl effects directory
2. Restart AviQtl
3. Verify the effect appears in the effects list
4. Test all parameters and edge cases

## Documentation

- [EFFECT_SCHEMA.md](../docs/effects/EFFECT_SCHEMA.md) - JSON schema reference
- [PLUGIN_SYSTEM_DESIGN.md](../docs/plugins/PLUGIN_SYSTEM_DESIGN.md) - Architecture
- [DEVELOPER_GUIDE.md](../docs/plugins/DEVELOPER_GUIDE.md) - Plugin development
- [API_REFERENCE.md](../docs/plugins/API_REFERENCE.md) - Lua API reference

## Contributing

Want to contribute your effect package?

1. Fork the repository
2. Create your package in `effect-packages/`
3. Add documentation in README.md
4. Submit a pull request

## License

All effect packages in this directory are licensed under AGPL-3.0, same as AviQtl-Plus.
