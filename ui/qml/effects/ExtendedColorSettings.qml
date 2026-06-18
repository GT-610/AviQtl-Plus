import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real red: Math.max(0, Math.min(200, root.evalNumber("red", 100))) / 100
    property real green: Math.max(0, Math.min(200, root.evalNumber("green", 100))) / 100
    property real blue: Math.max(0, Math.min(200, root.evalNumber("blue", 100))) / 100
    property real hue: Math.max(-180, Math.min(180, root.evalNumber("hue", 0)))
    property real saturation: Math.max(0, Math.min(200, root.evalNumber("saturation", 100))) / 100
    property real value: Math.max(0, Math.min(200, root.evalNumber("value", 100))) / 100

    ShaderEffect {
        property variant source: root.sourceProxy
        property real red: root.red
        property real green: root.green
        property real blue: root.blue
        property real hue: root.hue
        property real saturation: root.saturation
        property real value: root.value

        anchors.fill: parent
        fragmentShader: "extended_color_settings.frag.qsb"
    }

}