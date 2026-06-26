import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real embossWidth: root.evalNumber("width", 200) / 100
    property real embossHeight: root.evalNumber("height", 200) / 100
    property real angle: root.evalNumber("angle", 135)
    property real strength: root.evalNumber("strength", 100) / 100

    ShaderEffect {
        property variant source: root.sourceProxy
        property real embossW: root.embossWidth
        property real embossH: root.embossHeight
        property real angle: root.angle
        property real strength: root.strength
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "emboss.frag.qsb"
    }
}
