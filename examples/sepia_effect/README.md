# Example Third-Party Effect: Sepia

This directory contains a complete example of a third-party effect for AviQtl-Plus.

## Files

| File | Description |
|------|-------------|
| `sepia.json` | Effect metadata and UI definition (fragment shader version) |
| `Sepia.qml` | QML component connecting parameters to the fragment shader |
| `sepia.frag` | GLSL fragment shader implementing the sepia effect |
| `sepia_compute.json` | Effect metadata (compute shader version) |
| `SepiaCompute.qml` | QML component for the compute shader version |
| `sepia.comp` | GLSL compute shader implementing the same effect |

## How It Works

### Fragment Shader Version (`sepia.*`)

1. **sepia.json** defines:
   - Two parameters: `intensity` (0-1) and `temperature` (-1 to 1)
   - Two slider controls in the UI
   - Category "Color" for menu organization

2. **Sepia.qml** extends `BaseEffect`:
   - Reads parameters via `evalNumber()` (supports keyframe animation)
   - Creates a `ShaderEffect` with the fragment shader

3. **sepia.frag** implements:
   - Standard sepia tone conversion using a color matrix
   - Temperature-based warm/cool color shift
   - Intensity-based blend between original and sepia

### Compute Shader Version (`sepia_compute.*`)

Same effect implemented as a compute shader, demonstrating:
- `BaseComputeEffect` usage (minimal QML)
- Image load/store pattern
- Bounds checking
- Automatic parameter injection

## Installation

### Manual Installation

1. Copy the effect files to your AviQtl effects directory:
   ```
   <AviQtl data dir>/effects/sepia/
   ```

2. Restart AviQtl or use the package manager to reload effects.

### Package Manager Distribution

To distribute via the package manager:

1. Host the files in a GitHub or Codeberg repository
2. Create a release with the effect files
3. Add an entry to your repository JSON:

```json
{
  "id": "com.example.sepia",
  "display_name": "Sepia Effect",
  "type": "effect",
  "release_feed": "https://github.com/yourusername/sepia-effect/releases.atom",
  "repository_url": "https://github.com/yourusername/sepia-effect"
}
```

## Creating Your Own Effect

1. Copy this directory and rename files
2. Update `id`, `name`, and `categories` in the JSON
3. Modify parameters and UI controls as needed
4. Implement your shader logic in the `.frag` or `.comp` file
5. Compile shaders with `qsb` (or let CMake handle it)

See [EFFECT_SCHEMA.md](../../docs/effects/EFFECT_SCHEMA.md) for the complete JSON reference.

## Key Conventions

- **Fragment shaders**: Use `#version 440`, bind uniforms at `binding=0`, texture at `binding=1`
- **Compute shaders**: Use `#version 430`, image at `binding=0`, texture at `binding=1`, params at `binding=2`
- **Always check bounds** in compute shaders
- **Parameter names** in GLSL must match JSON `param` keys (or use `uniformMapping` in QML)
