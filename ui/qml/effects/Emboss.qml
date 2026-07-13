import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real width: root.evalNumber("width", 200) / 100
    property real height: root.evalNumber("height", 200) / 100
    property real angle: root.evalNumber("angle", 135)
    property real strength: root.evalNumber("strength", 100) / 100

    ShaderEffect {
        property var source: root.sourceProxy
        property real width: root.width
        property real height: root.height
        property real angle: root.angle
        property real strength: root.strength
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "emboss.frag.qsb"
    }
}
