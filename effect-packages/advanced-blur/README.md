# Advanced Blur Effects Pack

Professional blur effects for AviQtl-Plus.

## Effects Included

| Effect | Type | Description |
|--------|------|-------------|
| `lens_blur` | Compute | Simulates camera lens bokeh with customizable aperture |
| `radial_blur` | Compute | Zoom/radial blur from center point |
| `directional_blur` | Fragment | Blur in a specific direction |
| `motion_blur` | Fragment | Simulates motion blur along movement path |

## Installation

### Method 1: Package Manager (Recommended)

1. Open AviQtl-Plus
2. Go to Settings → Package Manager
3. Click "Install from Local" and select this folder
4. Restart AviQtl

### Method 2: Manual Installation

1. Copy the entire `advanced-blur/` folder to `<AviQtl Data>/effects/`
2. Restart AviQtl

## Directory Structure

Each effect is contained in its own subdirectory named after the effect ID:

```text
advanced-blur/
├── manifest.json              # Package metadata
├── README.md                  # This file
├── lens_blur/                 # Effect ID: "lens_blur"
│   ├── lens_blur.json         # Effect definition
│   ├── LensBlur.qml           # QML component
│   ├── lensblur.frag          # Fragment shader (fallback)
│   └── lensblur_compute.comp  # Compute shader (primary)
├── radial_blur/               # Effect ID: "radial_blur"
│   ├── radial_blur.json
│   ├── RadialBlur.qml
│   ├── radialblur.frag
│   └── radialblur_compute.comp
├── directional_blur/          # Effect ID: "directional_blur"
│   ├── directional_blur.json
│   ├── DirectionalBlur.qml
│   └── directionalblur.frag
└── motion_blur/               # Effect ID: "motion_blur"
    ├── motion_blur.json
    ├── MotionBlur.qml
    └── motionblur.frag
```

## Effect Parameters

### Lens Blur
- `radius` - Blur radius (0-50)
- `aperture` - Aperture shape (0-1)
- `iterations` - Quality (1-10)

### Radial Blur
- `strength` - Blur intensity (0-100)
- `center_x` - Center X position (0-1)
- `center_y` - Center Y position (0-1)

### Directional Blur
- `length` - Blur length (0-100)
- `angle` - Direction angle (0-360)

### Motion Blur
- `length` - Blur trail length (0-100)
- `angle` - Motion direction (0-360)

## Shader Implementation Notes

### Compute Shaders (Lens Blur, Radial Blur)

These effects use compute shaders for better performance:

- Work group size: 8x8
- Multi-pass: Supported via `dispatchCount`
- HDR: Can use RGBA16F for better quality

### Fragment Shaders (Directional Blur, Motion Blur)

These effects use fragment shaders for simplicity:

- Single-pass rendering
- Standard RGBA8 output

## Creating Custom Blur Effects

1. **Create directory**: Create a new folder named after your effect ID
2. **Analyze**: Determine if compute shader is needed (complex kernels)
3. **Prototype**: Start with fragment shader for simplicity
4. **Optimize**: Migrate to compute shader if performance is critical
5. **Test**: Verify with various input sizes and parameter ranges

See [EFFECT_SCHEMA.md](../../docs/effects/EFFECT_SCHEMA.md) for complete reference.

## License

AGPL-3.0 - Same as AviQtl-Plus
