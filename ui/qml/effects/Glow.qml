import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real intensity: Math.max(0, Math.min(200, root.evalNumber("intensity", 100)))
    property real radius: Math.max(0, Math.min(50, root.evalNumber("radius", 10)))
    property real threshold: Math.max(0, Math.min(100, root.evalNumber("threshold", 50)))
    property color glowColor: root.evalColor("color", "#ffffff")

    ShaderEffect {
        property variant source: root.sourceProxy
        property real intensity: root.intensity / 100.0
        property real radius: root.radius
        property real threshold: root.threshold / 100.0
        property color glowColor: root.glowColor
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "glow.frag.qsb"
    }

}