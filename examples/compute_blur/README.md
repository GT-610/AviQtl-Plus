# Example Effect: Compute Blur

A compute shader effect demonstrating multi-pass rendering with
`passIndex` injection. This is the compute shader equivalent of
a separable blur — the same algorithm runs twice (horizontal then
vertical) with a uniform telling the shader which direction to blur.

## Files

| File | Description |
|------|-------------|
| `compute_blur.json` | Effect metadata with one `radius` parameter |
| `ComputeBlur.qml` | QML component using `BaseComputeEffect` with `dispatchCount: 2` |
| `compute_blur.comp` | GLSL compute shader with per-pass direction switching |

## How It Works

### Multi-Pass Dispatch

The effect runs the same compute shader twice using `dispatchCount: 2`.
Between passes, `ComputeRenderNode` automatically injects the current
pass index into the `passIndex` uniform in the parameter UBO.

```
Pass 0: read from source → horizontal blur → write to texture A
Pass 1: read from texture A → vertical blur → write to texture B
```

The ping-pong mechanism alternates between two output textures so
each pass reads the result of the previous pass.

### `passIndex` Injection

The shader declares `int passIndex` in its `Params` uniform block.
`ComputeRenderNode` finds this member via shader reflection and
updates it before each dispatch:

```glsl
layout(std140, binding = 2) uniform Params {
    float radius;
    float targetWidth;
    float targetHeight;
    int   passIndex;   // ← auto-injected, don't set from QML
};
```

The `passIndex` member must be named exactly `passIndex` and typed as
`int`. It is automatically set to 0, 1, 2, ... for each dispatch.

### QML

```qml
Common.BaseComputeEffect {
    id: root
    property real radius: ...
    computeShader: "compute_blur.comp.qsb"
    dispatchCount: 2   // ← triggers multi-pass
}
```

`BaseComputeEffect` handles:
- Source texture discovery
- Uniform building from JSON params
- `time` injection (current frame number)
- Multi-pass ping-pong via `dispatchCount`

### GLSL

The compute shader follows the standard binding convention:

```
binding=0  rgba8 image2D outImage     (write-only output)
binding=1  sampler2D inTex            (read-only input)
binding=2  uniform Params { ... }     (parameter UBO)
```

Key rules:
- Always check bounds: `if (gid.x >= sz.x || gid.y >= sz.y) return;`
- Use `#version 430` and `local_size_x = 8, local_size_y = 8`
- `passIndex` is auto-injected — do not pass it from QML params

## Installation

1. Copy all files to your AviQtl effects directory:
   ```
   <AviQtl data dir>/effects/compute_blur/
   ```

2. Compile the shader if needed:
   ```bash
   qsb --glsl 310es,430 --hlsl 50 --msl 12 compute_blur.comp -o compute_blur.comp.qsb
   ```

3. Restart AviQtl.

## When to Use Compute vs Fragment

| Feature | Fragment | Compute |
|---------|----------|---------|
| Simple per-pixel effect | ✅ Best choice | Works but overkill |
| Multi-pass (blur, convolution) | Needs ShaderEffectSource chain | ✅ Native ping-pong |
| Image load/store (read-modify-write) | ❌ Not available | ✅ |
| HDR output (RGBA16F) | ❌ Always RGBA8 | ✅ `hdrOutput: true` |
| Extra texture inputs | Limited | ✅ `extraTextures` |
| Requires Vulkan-capable GPU | No | Yes |

See also: [sepia_effect](../sepia_effect/) for a fragment shader example.
