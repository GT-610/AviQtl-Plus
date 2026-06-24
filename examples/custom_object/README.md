# Example Object: Color Chip

A minimal custom object demonstrating the object extension pattern
for AviQtl-Plus. Renders a colored rectangle with configurable size,
color, corner radius, and opacity.

## Files

| File | Description |
|------|-------------|
| `ColorChipObject.json` | Object metadata: id, params, UI controls |
| `ColorChipObject.qml` | QML component extending `BaseObject` |

## How It Works

### JSON (`ColorChipObject.json`)

Objects use `"kind": "object"` (not `"effect"`). The JSON structure is
identical to effects — params, UI controls, categories — but the
component renders a scene entity rather than a post-processing filter.

```json
{
    "id": "color_chip",
    "kind": "object",
    "categories": ["Custom"],
    "params": { "sizeW": 200, "sizeH": 200, ... }
}
```

### QML (`ColorChipObject.qml`)

Objects extend `Common.BaseObject` which provides:
- `evalNumber()`, `evalColor()`, `evalString()` — keyframe-aware params
- Integration with the 3D scene graph (QtQuick3D `Node`)
- Effect chain support via `fbCaptureItem`
- `sourceItem` — the 2D content to render in the scene

Key requirements:
1. Set `sourceItem` to the root `Item` containing your 2D content
2. Set `sourceItem.visible = false` (the scene graph handles visibility)
3. Add `Common.DisplayModel { baseObject: root }` for 3D rendering
4. Set `outputModelOpacity` to control scene-level opacity

```qml
Common.BaseObject {
    id: root
    sourceItem: sourceItem
    outputModelOpacity: root.opacity

    Item {
        id: sourceItem
        visible: false
        width: root.sizeW
        height: root.sizeH
        // Your 2D content here
    }

    Common.DisplayModel {
        baseObject: root
    }
}
```

### Parameter Naming

The first argument to `evalNumber()` is the **object id** from the JSON,
not the effect id. This is because objects are registered with their own
id namespace:

```qml
property real sizeW: evalNumber("color_chip", "sizeW", 200)
//                        ^^^^^^^^^^  ^^^^^  ^^^
//                        object id   param  default
```

## Installation

1. Copy all files to your AviQtl objects directory:
   ```
   <AviQtl data dir>/objects/color_chip/
   ```

2. Restart AviQtl. The object appears in the object palette.

## Creating Your Own Object

1. Copy this directory and rename files
2. Update `id`, `name`, and `categories` in the JSON
3. Add parameters and UI controls
4. Implement your QML content inside the `sourceItem` `Item`

### Content Types

Objects can contain any Qt Quick content:
- **`Rectangle`** — colored shapes (like this example)
- **`Canvas`** — custom 2D drawing (see `RectObject.qml`)
- **`Text`** — text overlays (see `TextObject.qml`)
- **`ShaderEffect`** — GPU-rendered visuals
- **`Image`** — bitmap content
- **`Loader`** — dynamic content

### Scene Graph Integration

Objects live in a QtQuick3D scene. The `sourceItem` is rendered to a
texture and displayed on a 3D plane. Effects applied to the object
run on this texture, not on the 3D geometry.

See [BaseObject.qml](../../ui/qml/common/BaseObject.qml) for the
full integration API.
