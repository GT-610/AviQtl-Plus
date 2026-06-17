import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real x: root.evalNumber("x", 5)
    property real y: root.evalNumber("y", 5)
    property real shadowOpacity: root.evalNumber("opacity", 80) / 100
    property real diffusion: root.evalNumber("diffusion", 5) / 100
    property string color: root.evalColor("color", "#000000")

    ShaderEffect {
        property variant source: root.sourceProxy
        property real offsetX: root.x
        property real offsetY: root.y
        property real shadowOpacity: root.shadowOpacity
        property real diffusion: root.diffusion
        property vector3d shadowColor: {
            var c = Qt.colorConvert(root.color || "#000000", "rgba");
            return Qt.vector3d(c.r, c.g, c.b);
        }
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "shadow.frag.qsb"
    }
}
