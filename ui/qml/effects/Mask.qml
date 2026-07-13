import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property int maskType: Math.max(0, Math.min(3, Math.round(root.evalNumber("maskType", 0))))
    property bool invertMask: root.evalParam("invertMask", false)
    property real maskStrength: Math.max(0, Math.min(100, root.evalNumber("maskStrength", 100)))

    ShaderEffect {
        property var source: root.sourceProxy
        property int maskType: root.maskType
        property real invertMask: root.invertMask ? 1.0 : 0.0
        property real maskStrength: root.maskStrength / 100.0
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "mask_effect.frag.qsb"
    }

}