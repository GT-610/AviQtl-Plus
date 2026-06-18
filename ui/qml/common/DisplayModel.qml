import QtQuick
import QtQuick3D

// Shared 3D Model block used by all objects that render 2D content into the
// 3D scene. Displays root.displayOutput as a textured rectangle with the
// correct scale, blend mode, and culling settings from BaseObject.
Model {
    id: displayModel

    // BaseObject reference — must be set by the parent object.
    // Typed as `var` because callers pass BaseObject (QtQuick3D Node), not QtQuick Item.
    required property var baseObject

    source: "#Rectangle"
    visible: baseObject.outputModelVisible
    scale: baseObject.displayModelScale
    opacity: baseObject.outputModelOpacity

    materials: DefaultMaterial {
        lighting: DefaultMaterial.NoLighting
        blendMode: baseObject.blendMode
        cullMode: baseObject.cullMode

        diffuseMap: Texture {
            sourceItem: baseObject.displayOutput
        }
    }
}
