import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real size: root.evalNumber("size", 3)
    property real blur: root.evalNumber("blur", 0)
    property string color: root.evalColor("color", "#ff0000")
    property color resolvedColor: root.color || "#ff0000"

    ShaderEffect {
        property var source: root.sourceProxy
        property real size: root.size
        property real blur: root.blur
        property vector3d borderColor: Qt.vector3d(root.resolvedColor.r, root.resolvedColor.g, root.resolvedColor.b)
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "border.frag.qsb"
    }
}
