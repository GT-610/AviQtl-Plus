import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 100) / 100
    property string color: root.evalColor("color", "#ffffff")
    property real preserveLuma: root.evalParam("preserveLuma", true) ? 1 : 0
    property color resolvedColor: root.color || "#ffffff"

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property vector3d monoColor: Qt.vector3d(root.resolvedColor.r, root.resolvedColor.g, root.resolvedColor.b)
        property real preserveLuma: root.preserveLuma

        anchors.fill: parent
        fragmentShader: "monochrome.frag.qsb"
    }
}
