import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 50) / 100
    property real range: root.evalNumber("range", 100) / 100

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property real range: root.range
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "sharpen.frag.qsb"
    }
}
