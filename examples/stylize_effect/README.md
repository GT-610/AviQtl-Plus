# Example Effect: Posterize

A minimal fragment shader effect that demonstrates the basics of creating
third-party effects for AviQtl-Plus.

Posterize reduces the number of distinct colors in the image by quantizing
each channel to a fixed number of levels, creating a poster-like look.
An optional dithering parameter adds noise to reduce banding artifacts.

## Files

| File | Description |
|------|-------------|
| `posterize.json` | Effect metadata: id, name, params, UI controls |
| `Posterize.qml` | QML component connecting params to the shader |
| `posterize.frag` | GLSL fragment shader source |
| `posterize.frag.qsb` | Pre-compiled multi-platform shader bundle |

## How It Works

### JSON (`posterize.json`)

Defines two parameters with slider controls:

- **levels** (2–32, default 8): Number of color levels per channel.
  Higher values = more colors = less posterized.
- **dither** (0–1, default 0): Strength of ordered dithering applied
  before quantization. Reduces banding at low level counts.

```json
{
  "id": "posterize",
  "name": "Posterize",
  "qml": "Posterize.qml",
  "version": "1.0.0",
  "kind": "effect",
  "categories": ["Stylize"],
  "params": { "levels": 8.0, "dither": 0.0 },
  "ui": { "controls": [ ... ] }
}
```

### QML (`Posterize.qml`)

Extends `BaseEffect` which provides:
- `evalNumber()` — keyframe-aware parameter evaluation
- `sourceProxy` — texture proxy of the input source
- Automatic parameter binding to the effect chain

The `ShaderEffect` item passes parameters as uniforms to the fragment shader.
All parameters are clamped to their valid range in QML to prevent
out-of-bounds values reaching the shader.

### GLSL (`posterize.frag`)

The shader follows the standard fragment effect binding convention:

```
binding=0  uniform buf { qt_Matrix, qt_Opacity, params... }
binding=1  sampler2D source
```

Algorithm:
1. Sample the input texture
2. If dithering is enabled, add per-pixel noise based on screen-space position
3. Quantize each RGB channel: `floor(color * levels + 0.5) / levels`
4. Output with opacity

## Installation

### Manual

1. Copy all files to your AviQtl effects directory:
   ```
   <AviQtl data dir>/effects/posterize/
   ```

2. If the `.qsb` file is missing or outdated, compile it:
   ```bash
   qsb --glsl 100es,120,150 --hlsl 50 --msl 12 posterize.frag -o posterize.frag.qsb
   ```

3. Restart AviQtl.

### Package Manager

To distribute via the package manager, host the files in a Git repository
and create a release with the effect files. Add an entry to your repository JSON:

```json
{
  "id": "com.example.posterize",
  "display_name": "Posterize Effect",
  "type": "effect",
  "release_feed": "https://github.com/yourname/posterize/releases.atom",
  "repository_url": "https://github.com/yourname/posterize"
}
```

Users can then install it from the Package Manager UI.

## Creating Your Own Effect

1. Copy this directory and rename all files
2. Update `id`, `name`, and `categories` in the JSON
3. Add or modify parameters and UI controls
4. Implement your shader logic in the `.frag` file
5. Compile with `qsb` or let CMake handle it during a full build

### Checklist

- [ ] `id` is unique and lowercase
- [ ] `qml` filename matches the QML component filename exactly
- [ ] `version` follows semver (`x.y.z`)
- [ ] `kind` is `"effect"`, `"object"`, or `"transition"`
- [ ] `categories` is a non-empty array
- [ ] `ui.controls` has at least one control
- [ ] GLSL `uniform` names match JSON `param` keys
- [ ] GLSL `binding` order follows the convention (see below)

### Binding Conventions

**Fragment shaders** (`#version 440`):
```
binding=0  uniform buf { qt_Matrix, qt_Opacity, your_params... }
binding=1  sampler2D source
```

**Compute shaders** (`#version 430`, `local_size_x=8, local_size_y=8`):
```
binding=0  rgba8 image2D outImage      (write-only output)
binding=1  sampler2D inTex              (read-only input)
binding=2  uniform Params { your_params... }
binding=3+ extra textures (optional)
```

See [EFFECT_SCHEMA.md](../../docs/effects/EFFECT_SCHEMA.md) for the complete reference.
