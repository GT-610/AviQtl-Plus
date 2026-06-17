import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real width: Math.max(1, Math.min(100, root.evalNumber("width", 10)))
    property real height: Math.max(1, Math.min(100, root.evalNumber("height", 10)))
    property real speed: Math.max(-100, Math.min(100, root.evalNumber("speed", 0)))
    property real angle: Math.max(0, Math.min(360, root.evalNumber("angle", 0)))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real width: root.width
        property real height: root.height
        property real speed: root.speed
        property real angle: root.angle
        property real time: root.frame
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "raster.frag.qsb"
    }

}