# Weather Objects Pack

Weather animation objects for AviQtl-Plus.

## Objects Included

| Object | Description |
|--------|-------------|
| `rain` | Animated rain particles with customizable intensity and angle |
| `snow` | Animated snow particles with wind and density controls |

## Installation

### Method 1: Package Manager (Recommended)

1. Open AviQtl-Plus
2. Go to Settings → Package Manager
3. Click "Install from Local" and select this folder
4. Restart AviQtl

### Method 2: Manual Installation

1. Copy the entire `weather-objects/` folder to `<AviQtl Data>/objects/`
2. Restart AviQtl

## Directory Structure

Each object is contained in its own subdirectory named after the object ID:

```
weather-objects/
├── manifest.json          # Package metadata
├── README.md              # This file
├── rain/                  # Object ID: "rain"
│   ├── RainObject.json    # Object definition
│   └── RainObject.qml     # QML component
└── snow/                  # Object ID: "snow"
    ├── SnowObject.json    # Object definition
    └── SnowObject.qml     # QML component
```

## Object Parameters

### Rain Object
- `intensity` - Rain intensity (0-100)
- `angle` - Rain angle (-90 to 90)
- `speed` - Drop speed (0-100)
- `dropLength` - Drop length (1-50)
- `color` - Drop color (#RRGGBB)
- `opacity` - Overall opacity (0-1)

### Snow Object
- `density` - Snow density (0-100)
- `wind` - Wind direction (-100 to 100)
- `speed` - Fall speed (0-100)
- `flakeSize` - Flake size (1-20)
- `color` - Flake color (#RRGGBB)
- `opacity` - Overall opacity (0-1)

## Object JSON Format

Objects are defined similarly to effects but with `kind: "object"`:

```json
{
  "id": "my_object",
  "name": "My Object",
  "qml": "MyObject.qml",
  "version": "1.0.0",
  "kind": "object",
  "categories": ["Category1"],
  "params": {
    "param1": 50
  },
  "ui": {
    "group": "object",
    "controls": [
      {
        "type": "slider",
        "param": "param1",
        "label": "Parameter 1",
        "min": 0,
        "max": 100
      }
    ]
  }
}
```

## Creating Custom Objects

1. **Create directory**: Create a new folder named after your object ID
2. **Define JSON**: Create `<ObjectID>.json` with `kind: "object"`
3. **Create QML**: Extend `BaseObject` for scene graph integration
4. **Add Properties**: Define visual properties in QML
5. **Connect Parameters**: Bind JSON params to QML properties
6. **Test**: Place in objects directory and restart AviQtl

See [EFFECT_SCHEMA.md](../../docs/effects/EFFECT_SCHEMA.md) for complete reference.

## License

AGPL-3.0 - Same as AviQtl-Plus
