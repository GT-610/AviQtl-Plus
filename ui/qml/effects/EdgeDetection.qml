import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 100) / 100
    property real threshold: root.evalNumber("threshold", 20) / 255
    property real luminanceEdge: root.evalParam("luminanceEdge", true) ? 1 : 0
    property real alphaEdge: root.evalParam("alphaEdge", false) ? 1 : 0
    property string color: root.evalColor("color", "#ffffff")

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property real threshold: root.threshold
        property real luminanceEdge: root.luminanceEdge
        property real alphaEdge: root.alphaEdge
        property vector3d edgeColor: {
            var c = Qt.colorConvert(root.color || "#ffffff", "rgba");
            return Qt.vector3d(c.r, c.g, c.b);
        }
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "edgedetection.frag.qsb"
    }
}
