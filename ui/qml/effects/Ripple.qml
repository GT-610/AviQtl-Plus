import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real amplitude: Math.max(0, Math.min(100, root.evalNumber("amplitude", 10)))
    property real frequency: Math.max(1, Math.min(50, root.evalNumber("frequency", 5)))
    property real speed: Math.max(-10, Math.min(10, root.evalNumber("speed", 1)))
    property real centerX: Math.max(0, Math.min(100, root.evalNumber("centerX", 50)))
    property real centerY: Math.max(0, Math.min(100, root.evalNumber("centerY", 50)))

    ShaderEffect {
        property var source: root.sourceProxy
        property real amplitude: root.amplitude / 100.0
        property real frequency: root.frequency
        property real speed: root.speed
        property real centerX: root.centerX / 100.0
        property real centerY: root.centerY / 100.0
        property real time: root.frame
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "ripple.frag.qsb"
    }

}