import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real hue: root.evalNumber("hue", 120)
    property real hueRange: root.evalNumber("hueRange", 30) / 360
    property real similarity: root.evalNumber("similarity", 40) / 100
    property real blend: root.evalNumber("blend", 10) / 100
    property real invert: root.evalParam("invert", false) ? 1 : 0

    ShaderEffect {
        property var source: root.sourceProxy
        property real hue: root.hue / 360
        property real hueRange: root.hueRange
        property real similarity: root.similarity
        property real blend: root.blend
        property real invert: root.invert

        anchors.fill: parent
        fragmentShader: "chromakey.frag.qsb"
    }
}
