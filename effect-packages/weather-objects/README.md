# Weather Objects Pack

Native WGSL rain and snow objects for the AviQtl-Plus Slint edition.

## Included objects

| Object | Description |
| --- | --- |
| `rain` | Animated rain streaks with configurable density, speed, size, spread, seed, color, and opacity |
| `snow` | Animated snow particles with configurable density, speed, size, spread, seed, color, and opacity |

## Installation

Package repositories distribute this directory as a ZIP archive. The package manager verifies the archive hash, validates every metadata and WGSL file, and installs it under `<AviQtl Data>/objects/com.aviqtl.objects.weather/`.

For development, copy the whole directory to that location and restart AviQtl-Plus.

## Directory structure

```text
weather-objects/
├── manifest.json
├── README.md
├── rain/
│   ├── RainObject.json
│   └── RainObject.wgsl
└── snow/
    ├── SnowObject.json
    └── SnowObject.wgsl
```

Each JSON document declares `runtime.engine` as `aviqtl-wgsl-v1`, names its shader relative to the JSON file, and lists the parameter order exposed through `aviqtl_parameter(index)`.

The host supplies the WGSL entry contract:

```wgsl
fn aviqtl_effect(
    input_color: vec4<f32>,
    uv: vec2<f32>,
    canvas_size: vec2<f32>,
    time_seconds: f32,
) -> vec4<f32>
```

Objects receive a transparent input canvas. Effects receive the previous layer result. Both use the same function and are rendered identically in preview and export.

## Parameters

- `count`: approximate particle density, from 1 to 2000
- `speed`: animation direction and speed, from -20 to 20
- `particleSize`: particle or streak size
- `spread`: wind or streak spread
- `seed`: deterministic layout seed
- `color`: RGBA particle color
- `opacity`: object opacity applied by the compositor

## License

AGPL-3.0, the same license as AviQtl-Plus.
