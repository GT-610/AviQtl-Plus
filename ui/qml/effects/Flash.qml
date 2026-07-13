import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real intensity: Math.max(0, Math.min(200, root.evalNumber("intensity", 100)))
    property real speed: Math.max(0, Math.min(100, root.evalNumber("speed", 50)))
    property color flashColor: root.evalColor("color", "#ffffff")
    property int type: Math.max(0, Math.min(2, Math.round(root.evalNumber("type", 0))))

    ShaderEffect {
        property var source: root.sourceProxy
        property real intensity: root.intensity / 100.0
        property real speed: root.speed / 100.0
        property color flashColor: root.flashColor
        property int type: root.type
        property real time: root.frame

        anchors.fill: parent
        fragmentShader: "flash.frag.qsb"
    }

}