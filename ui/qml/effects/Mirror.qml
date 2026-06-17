import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real transparency: root.evalNumber("transparency", 50) / 100
    property real decay: root.evalNumber("decay", 0) / 100
    property int direction: root.evalNumber("direction", 0)
    property real centerOffset: root.evalNumber("centerOffset", 0) / 100

    ShaderEffect {
        property variant source: root.sourceProxy
        property real transparency: root.transparency
        property real decay: root.decay
        property int direction: root.direction
        property real centerOffset: root.centerOffset

        anchors.fill: parent
        fragmentShader: "mirror.frag.qsb"
    }
}
