import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real shadowOffsetX: root.evalNumber("x", 5)
    property real shadowOffsetY: root.evalNumber("y", 5)
    property real shadowOpacity: root.evalNumber("opacity", 80) / 100
    property real diffusion: root.evalNumber("diffusion", 5) / 100
    property string color: root.evalColor("color", "#000000")
    property color resolvedColor: root.color || "#000000"

    ShaderEffect {
        property var source: root.sourceProxy
        property real offsetX: root.shadowOffsetX
        property real offsetY: root.shadowOffsetY
        property real shadowOpacity: root.shadowOpacity
        property real diffusion: root.diffusion
        property vector3d shadowColor: Qt.vector3d(root.resolvedColor.r, root.resolvedColor.g, root.resolvedColor.b)
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "shadow.frag.qsb"
    }
}
