import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real threshold: Math.max(0, Math.min(100, root.evalNumber("threshold", 50)))
    property real blend: Math.max(0, Math.min(100, root.evalNumber("blend", 10)))
    property bool invert: root.evalParam("invert", false)

    ShaderEffect {
        property variant source: root.sourceProxy
        property real threshold: root.threshold / 100.0
        property real blend: root.blend / 100.0
        property real invert: root.invert ? 1.0 : 0.0

        anchors.fill: parent
        fragmentShader: "luminancekey.frag.qsb"
    }

}