import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property int lightType: Math.max(0, Math.min(2, Math.round(root.evalNumber("lightType", 0))))
    property real intensity: Math.max(0, Math.min(200, root.evalNumber("intensity", 100)))
    property real radius: Math.max(0, Math.min(100, root.evalNumber("radius", 50)))
    property real x: Math.max(0, Math.min(100, root.evalNumber("x", 50)))
    property real y: Math.max(0, Math.min(100, root.evalNumber("y", 50)))
    property color lightColor: root.evalColor("color", "#ffffff")

    ShaderEffect {
        property var source: root.sourceProxy
        property int lightType: root.lightType
        property real intensity: root.intensity / 100.0
        property real radius: root.radius / 100.0
        property real lightX: root.x / 100.0
        property real lightY: root.y / 100.0
        property color lightColor: root.lightColor
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "light.frag.qsb"
    }

}