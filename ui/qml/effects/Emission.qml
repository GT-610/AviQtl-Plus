import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 50) / 100
    property real diffusion: root.evalNumber("diffusion", 20) / 100
    property real threshold: root.evalNumber("threshold", 80) / 100
    property string color: root.evalColor("color", "")
    property bool fixedSize: root.evalParam("fixedSize", false)

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property real diffusion: root.diffusion
        property real threshold: root.threshold
        property vector3d glowColor: {
            if (root.color.length > 0) {
                var c = Qt.colorConvert(root.color, "rgba");
                return Qt.vector3d(c.r, c.g, c.b);
            }
            return Qt.vector3d(0, 0, 0);
        }
        property real useCustomColor: root.color.length > 0 ? 1.0 : 0.0
        property real texelW: root.source ? 1.0 / root.source.width : 0
        property real texelH: root.source ? 1.0 / root.source.height : 0

        anchors.fill: parent
        fragmentShader: "emission.frag.qsb"
    }
}
