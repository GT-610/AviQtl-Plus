import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 100) / 100
    property real centerX: root.evalNumber("centerX", 50) / 100
    property real centerY: root.evalNumber("centerY", 50) / 100
    property real angle: root.evalNumber("angle", 0)
    property real width: root.evalNumber("width", 100) / 100
    property int shape: root.evalNumber("shape", 0)
    property string startColor: root.evalColor("startColor", "#00000000")
    property string endColor: root.evalColor("endColor", "#ffffffff")

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property vector2d center: Qt.vector2d(root.centerX, root.centerY)
        property real angle: root.angle
        property real width: root.width
        property int shape: root.shape
        property vector4d colStart: {
            var c = Qt.colorConvert(root.startColor, "rgba");
            return Qt.vector4d(c.r, c.g, c.b, c.a);
        }
        property vector4d colEnd: {
            var c = Qt.colorConvert(root.endColor, "rgba");
            return Qt.vector4d(c.r, c.g, c.b, c.a);
        }

        anchors.fill: parent
        fragmentShader: "gradient.frag.qsb"
    }
}
