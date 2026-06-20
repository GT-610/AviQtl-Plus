import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real intensity: Math.max(0, Math.min(1, root.evalNumber("intensity", 1.0)))
    property real temperature: Math.max(-1, Math.min(1, root.evalNumber("temperature", 0.0)))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real intensity: root.intensity
        property real temperature: root.temperature
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "sepia.frag.qsb"
    }
}
