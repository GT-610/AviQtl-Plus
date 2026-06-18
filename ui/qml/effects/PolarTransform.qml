import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real centerX: Math.max(0, Math.min(100, root.evalNumber("centerX", 50)))
    property real centerY: Math.max(0, Math.min(100, root.evalNumber("centerY", 50)))
    property real scale: Math.max(10, Math.min(200, root.evalNumber("scale", 100)))
    property real angleOffset: Math.max(0, Math.min(360, root.evalNumber("angleOffset", 0)))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real centerX: root.centerX / 100.0
        property real centerY: root.centerY / 100.0
        property real scale: root.scale / 100.0
        property real angleOffset: root.angleOffset
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "polar_transform.frag.qsb"
    }

}