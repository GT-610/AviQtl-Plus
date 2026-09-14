# AviQtl-Plus native render packages

This directory contains installable Effect and Object packages for the Slint/wgpu application.

## Available packages

| Package | Type | Contents | Description |
| --- | --- | --- | --- |
| [weather-objects](weather-objects/) | Object | 2 objects | Native WGSL rain and snow animations |

## Package model

The package manager continues to own repository synchronization, downloads, SHA-256 verification, safe ZIP extraction, installation, upgrades, rollback, and removal. Effect and Object packages use the `aviqtl-wgsl-v1` runtime; QML files are not loaded.

Each render definition contains JSON metadata and one WGSL source file:

```text
my-package/
├── manifest.json
└── my-effect/
    ├── MyEffect.json
    └── MyEffect.wgsl
```

```json
{
  "id": "my_effect",
  "name": "My Effect",
  "version": "1.0.0",
  "kind": "effect",
  "categories": ["Custom"],
  "params": {
    "amount": 0.5,
    "tint": "#ffffffff"
  },
  "ui": {
    "controls": [
      {
        "type": "slider",
        "param": "amount",
        "label": "Amount",
        "min": 0.0,
        "max": 1.0,
        "step": 0.01
      },
      {
        "type": "color",
        "param": "tint",
        "label": "Tint"
      }
    ]
  },
  "runtime": {
    "engine": "aviqtl-wgsl-v1",
    "shader": "MyEffect.wgsl",
    "uniforms": ["amount", "tint"]
  }
}
```

The `uniforms` array may contain at most 16 unique parameter names. Each entry is available as one `vec4<f32>` through `aviqtl_parameter(index)`:

- numbers and booleans use `.x`;
- colors use RGBA in `.rgba`, normalized to 0–1;
- numeric arrays fill up to four components.

## WGSL contract

Package shaders provide one function and may define private helper functions:

```wgsl
fn aviqtl_effect(
    input_color: vec4<f32>,
    uv: vec2<f32>,
    canvas_size: vec2<f32>,
    time_seconds: f32,
) -> vec4<f32> {
    let amount = aviqtl_parameter(0u).x;
    let tint = aviqtl_parameter(1u);
    return mix(input_color, vec4<f32>(tint.rgb, input_color.a), amount);
}
```

The host also provides `aviqtl_sample(uv)` for neighboring source samples. Package shaders cannot declare bindings, shader stages, or entry points; those resources remain host-owned and are validated before installation.

- An Effect processes the existing layer texture.
- An Object receives a transparent canvas matching the scene size.
- Preview and export execute the same wgpu pipeline.

## Testing a package

1. Put the package under `<AviQtl Data>/effects/<package-id>/` or `<AviQtl Data>/objects/<package-id>/`.
2. Restart AviQtl-Plus so the catalog is reloaded.
3. Add the Effect or Object and test its controls, keyframes, resolutions, and timeline positions.
4. Package the directory as a ZIP and publish its SHA-256 in the repository metadata.

## License

Packages in this directory use AGPL-3.0 unless their manifest states otherwise.
