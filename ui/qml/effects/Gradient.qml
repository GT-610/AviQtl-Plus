import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 100) / 100
    property real centerX: root.evalNumber("centerX", 50) / 100
    property real centerY: root.evalNumber("centerY", 50) / 100
    property real angle: root.evalNumber("angle", 0)
    property real gradientWidth: root.evalNumber("width", 100) / 100
    property int shape: root.evalNumber("shape", 0)
    property string startColor: root.evalColor("startColor", "#00000000")
    property string endColor: root.evalColor("endColor", "#ffffffff")
    property color resolvedStartColor: root.startColor
    property color resolvedEndColor: root.endColor

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property vector2d center: Qt.vector2d(root.centerX, root.centerY)
        property real angle: root.angle
        property real gradientWidth: root.gradientWidth
        property int shape: root.shape
        property vector4d colStart: Qt.vector4d(root.resolvedStartColor.r, root.resolvedStartColor.g, root.resolvedStartColor.b, root.resolvedStartColor.a)
        property vector4d colEnd: Qt.vector4d(root.resolvedEndColor.r, root.resolvedEndColor.g, root.resolvedEndColor.b, root.resolvedEndColor.a)

        anchors.fill: parent
        fragmentShader: "gradient.frag.qsb"
    }
}
