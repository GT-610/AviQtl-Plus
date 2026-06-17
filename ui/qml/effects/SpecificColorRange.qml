import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real targetHue: Math.max(0, Math.min(360, root.evalNumber("targetHue", 120)))
    property real hueRange: Math.max(0, Math.min(180, root.evalNumber("hueRange", 30)))
    property color targetColor: root.evalColor("targetColor", "#ff0000")
    property real strength: Math.max(0, Math.min(100, root.evalNumber("strength", 100)))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real targetHue: root.targetHue / 360.0
        property real hueRange: root.hueRange / 360.0
        property color targetColor: root.targetColor
        property real strength: root.strength / 100.0

        anchors.fill: parent
        fragmentShader: "specific_color_range.frag.qsb"
    }

}