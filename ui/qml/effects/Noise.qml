import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real strength: root.evalNumber("strength", 30) / 100
    property real speed: root.evalNumber("speed", 0)
    property real seed: root.evalNumber("seed", 0)
    property real time: root.frame

    ShaderEffect {
        property var source: root.sourceProxy
        property real strength: root.strength
        property real seed: root.seed
        property real time: root.time * root.speed * 0.01

        anchors.fill: parent
        fragmentShader: "noise.frag.qsb"
    }
}
