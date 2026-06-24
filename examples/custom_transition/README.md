# Example Transition: Zoom

A custom transition effect demonstrating the transition extension pattern.
The next scene zooms in from half-size to full-size while the previous
scene fades out.

## Files

| File | Description |
|------|-------------|
| `zoom_transition.json` | Transition metadata: id, params, UI controls |
| `ZoomTransition.qml` | QML component with zoom + fade animation |

## How It Works

### JSON

Transitions use `"kind": "transition"`. They share the same JSON
structure as effects but with a `"group": "transition"` in the UI
and typically include `duration`, `easing`, and `reverse` params.

```json
{
    "id": "zoom_transition",
    "kind": "transition",
    "categories": ["Basic", "Zoom"],
    "params": { "duration": 30, "easing": "ease_in_out", "reverse": false }
}
```

### QML

Transitions extend `Item` (not `BaseEffect` or `BaseObject`) and
implement three key properties:

- **`previousScene`** — the scene being transitioned out
- **`nextScene`** — the scene being transitioned in
- **`progress`** — animated from 0.0 to 1.0 over `duration` frames

The transition modifies the `opacity`, `scale`, `position`, etc. of
these scene references based on `progress`.

```qml
Item {
    property int duration: 30
    property string easing: "ease_in_out"
    property bool reverse: false
    property real progress: 0.0

    property var previousScene: null
    property var nextScene: null

    onProgressChanged: {
        if (previousScene) previousScene.opacity = 1.0 - progress;
        if (nextScene) {
            nextScene.opacity = progress;
            nextScene.scale = 0.5 + progress * 0.5;
        }
    }

    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }
}
```

### Animation

The `NumberAnimation on progress` drives the transition. Duration is
in frames (converted to milliseconds at 60fps). Easing controls the
acceleration curve.

## Installation

1. Copy all files to your AviQtl transitions directory:
   ```
   <AviQtl data dir>/transitions/zoom_transition/
   ```

2. Restart AviQtl. The transition appears in the transition selector.

## Creating Your Own Transition

1. Copy this directory and rename files
2. Update `id`, `name`, and `categories` in the JSON
3. Modify the `onProgressChanged` handler to implement your animation
4. Optionally add more parameters (direction, intensity, etc.)

### Common Patterns

**Fade**: Adjust `previousScene.opacity` and `nextScene.opacity`

**Slide**: Adjust `previousScene.x` and `nextScene.x` (or `y`)

**Zoom**: Adjust `nextScene.scale` with `transformOrigin`

**Rotate**: Adjust `nextScene.rotation` with `transformOrigin`

**Wipe**: Use `ShaderEffectSource` clipping with a mask

You can combine multiple properties for complex transitions.
