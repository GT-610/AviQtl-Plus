# Basic Transitions Pack

Basic transition effects for AviQtl-Plus.

## Transitions Included

| Transition | Description |
|------------|-------------|
| `cross_fade` | Smooth opacity blend between scenes |
| `slide` | Slide one scene over another |
| `wipe_circle` | Circular wipe reveal effect |

## Installation

### Method 1: Package Manager (Recommended)

1. Open AviQtl-Plus
2. Go to Settings → Package Manager
3. Click "Install from Local" and select this folder
4. Restart AviQtl

### Method 2: Manual Installation

1. Copy the entire `transitions/` folder to `<AviQtl Data>/transitions/`
2. Restart AviQtl

## Directory Structure

Each transition is contained in its own subdirectory named after the transition ID:

```
transitions/
├── manifest.json          # Package metadata
├── README.md              # This file
├── cross_fade/            # Transition ID: "cross_fade"
│   ├── cross_fade.json    # Transition definition
│   └── CrossFade.qml      # QML component
├── slide/                 # Transition ID: "slide"
│   ├── slide.json
│   └── Slide.qml
└── wipe_circle/           # Transition ID: "wipe_circle"
    ├── wipe_circle.json
    └── WipeCircle.qml
```

## Transition Parameters

### Cross Fade
- `duration` - Transition duration in frames (1-300)
- `easing` - Easing function (linear, ease_in, ease_out, ease_in_out)
- `reverse` - Reverse the transition direction

### Slide
- `duration` - Transition duration in frames (1-300)
- `direction` - Slide direction (left, right, up, down)
- `easing` - Easing function

### Wipe Circle
- `duration` - Transition duration in frames (1-300)
- `centerX` - Center X position (0-1)
- `centerY` - Center Y position (0-1)
- `easing` - Easing function

## Transition JSON Format

Transitions are defined with `kind: "transition"`:

```json
{
  "id": "my_transition",
  "name": "My Transition",
  "qml": "MyTransition.qml",
  "version": "1.0.0",
  "kind": "transition",
  "color": "#9C27B0",
  "categories": ["Category1"],
  "params": {
    "duration": 30,
    "easing": "linear",
    "reverse": false
  },
  "ui": {
    "group": "transition",
    "controls": [
      {
        "type": "spinner",
        "param": "duration",
        "label": "Duration",
        "min": 1,
        "max": 300,
        "step": 1
      },
      {
        "type": "enum",
        "param": "easing",
        "label": "Easing",
        "options": ["linear", "ease_in", "ease_out", "ease_in_out"]
      },
      {
        "type": "bool",
        "param": "reverse",
        "label": "Reverse"
      }
    ]
  }
}
```

## Creating Custom Transitions

1. **Create directory**: Create a new folder named after your transition ID
2. **Define JSON**: Create `<transition_id>.json` with `kind: "transition"`
3. **Create QML**: Access `previousScene` and `nextScene` textures
4. **Animate**: Use `progress` property (0.0 → 1.0) for animation
5. **Add Controls**: Define duration, easing, and direction parameters
6. **Test**: Place in transitions directory and restart AviQtl

See [EFFECT_SCHEMA.md](../../docs/effects/EFFECT_SCHEMA.md) for complete reference.

## License

AGPL-3.0 - Same as AviQtl-Plus
