import QtQuick
import QtQuick3D
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common
import "qrc:/qt/qml/AviQtl/ui/qml/common/BlendModeUtils.js" as BlendModeUtils

Common.BaseEffect {
    id: root

    readonly property vector3d outputPosition: {
        const x = evalNumber("x", 0);
        const y = evalNumber("y", 0);
        const z = evalNumber("z", 0);
        return Qt.vector3d(x, y, z);
    }
    readonly property vector3d outputRotation: {
        const rx = evalNumber("rotationX", 0);
        const ry = evalNumber("rotationY", 0);
        const rz = evalNumber("rotationZ", 0);
        return Qt.vector3d(rx, ry, -rz);
    }
    readonly property vector3d outputPivot: {
        const cx = evalNumber("cx", 0);
        const cy = evalNumber("cy", 0);
        const cz = evalNumber("cz", 0);
        return Qt.vector3d(cx, cy, cz);
    }
    readonly property real outputOpacity: evalNumber("opacity", 1)
    readonly property int outputCullMode: {
        return evalParam("backfaceVisible", true) ? DefaultMaterial.NoCulling : DefaultMaterial.BackFaceCulling;
    }
    readonly property real output2dScale: Math.max(0, evalNumber("scale", 100)) / 100
    readonly property real output2dX: evalNumber("x", 0)
    readonly property real output2dY: evalNumber("y", 0)
    readonly property real output2dRotationZ: evalNumber("rotationZ", 0)
    readonly property real outputCx: evalNumber("cx", 0)
    readonly property real outputCy: evalNumber("cy", 0)
    readonly property int blendModeInt: BlendModeUtils.blendModeToInt(evalParam("blendMode", "通常"))

    ShaderEffect {
        property var source: root.sourceProxy
        // アフィン変換 uniform バインド
        property real translationX: 0
        property real translationY: 0
        property real scale: 1
        property real rotationZ: 0
        property real cx: 0
        property real cy: 0
        property real opacityValue: root.outputOpacity
        property real targetWidth: root.width
        property real targetHeight: root.height
        property int blendMode: root.blendModeInt

        anchors.fill: parent
        fragmentShader: "transform.frag.qsb"
    }

}
