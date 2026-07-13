import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property color keyColor: root.evalColor("keyColor", "#00ff00")
    property real similarity: Math.max(0, Math.min(100, root.evalNumber("similarity", 40)))
    property real blend: Math.max(0, Math.min(100, root.evalNumber("blend", 10)))
    property bool invert: root.evalParam("invert", false)

    ShaderEffect {
        property var source: root.sourceProxy
        property color keyColor: root.keyColor
        property real similarity: root.similarity / 100.0
        property real blend: root.blend / 100.0
        property real invert: root.invert ? 1.0 : 0.0

        anchors.fill: parent
        fragmentShader: "colorkey.frag.qsb"
    }

}