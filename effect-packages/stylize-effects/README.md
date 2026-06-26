# Stylize Effects Pack

A collection of creative stylize effects for AviQtl-Plus.

## Effects Included

| Effect | Type | Description |
|--------|------|-------------|
| `glitch` | Compute | Digital glitch effect with scanlines and color shift |
| `pixelsorter` | Compute | Pixel sorting algorithm for abstract visual effects |
| `chromatic_aberration` | Fragment | RGB channel separation simulating lens distortion |
| `mosaic` | Fragment | Pixelation/mosaic effect |
| `noise` | Fragment | Random noise overlay |
| `emboss` | Fragment | 3D emboss/relief effect |
| `raster` | Fragment | CRT-style raster lines |

## Installation

### Method 1: Package Manager (Recommended)

1. Open AviQtl-Plus
2. Go to Settings → Package Manager
3. Click "Install from Local" and select this folder
4. Restart AviQtl

### Method 2: Manual Installation

1. Copy the entire `stylize-effects/` folder to `<AviQtl Data>/effects/`
2. Restart AviQtl

## Directory Structure

Each effect is contained in its own subdirectory named after the effect ID:

```
stylize-effects/
├── manifest.json              # Package metadata
├── README.md                  # This file
├── glitch/                    # Effect ID: "glitch"
│   ├── glitch.json            # Effect definition
│   ├── Glitch.qml             # QML component
│   └── glitch.comp            # Compute shader
├── pixelsorter/               # Effect ID: "pixelsorter"
│   ├── pixelsorter.json
│   ├── PixelSorter.qml
│   └── pixelsorter.comp
├── chromatic_aberration/      # Effect ID: "chromatic_aberration"
│   ├── chromatic_aberration.json
│   ├── ChromaticAberration.qml
│   └── chromatic_aberration.frag
├── mosaic/                    # Effect ID: "mosaic"
│   ├── mosaic.json
│   ├── Mosaic.qml
│   └── mosaic.frag
├── noise/                     # Effect ID: "noise"
│   ├── noise.json
│   ├── Noise.qml
│   └── noise.frag
├── emboss/                    # Effect ID: "emboss"
│   ├── emboss.json
│   ├── Emboss.qml
│   └── emboss.frag
└── raster/                    # Effect ID: "raster"
    ├── raster.json
    ├── Raster.qml
    └── raster.frag
```

## Effect JSON Format

Each effect is defined by a JSON file with the following structure:

```json
{
  "id": "unique_effect_id",
  "name": "Display Name",
  "qml": "EffectName.qml",
  "version": "1.0.0",
  "kind": "effect",
  "categories": ["Category1", "Category2"],
  "params": {
    "param1": 0.5,
    "param2": 1.0
  },
  "ui": {
    "controls": [
      {
        "type": "slider",
        "param": "param1",
        "label": "Parameter 1",
        "min": 0.0,
        "max": 1.0,
        "step": 0.01
      }
    ]
  }
}
```

## Shader Types

### Fragment Shader (`.frag`)

- Used for per-pixel effects
- Input: `source` texture at binding=1
- Output: `fragColor`
- Parameters via uniform block at binding=0

### Compute Shader (`.comp`)

- Used for parallel GPU computation
- Input: `inTex` texture at binding=1
- Output: `outImage` image at binding=0
- Parameters via uniform block at binding=2
- Work group size: 8x8

## Creating New Effects

1. **Create directory**: Create a new folder named after your effect ID
2. **Define JSON**: Create `<effect_id>.json` with metadata and parameters
3. **Create QML**: Create `<EffectName>.qml` extending `BaseEffect` or `BaseComputeEffect`
4. **Write Shader**: Create `.frag` or `.comp` shader
5. **Compile**: Use `qsb` to compile shader to `.qsb` format
6. **Test**: Place in effects directory and restart AviQtl

See [EFFECT_SCHEMA.md](../../docs/effects/EFFECT_SCHEMA.md) for complete reference.

## License

AGPL-3.0 - Same as AviQtl-Plus
