import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real intensity: Math.max(0, Math.min(100, root.evalNumber("intensity", 10)))
    property int mapType: Math.max(0, Math.min(3, Math.round(root.evalNumber("mapType", 0))))
    property real scaleX: Math.max(10, Math.min(200, root.evalNumber("scaleX", 100)))
    property real scaleY: Math.max(10, Math.min(200, root.evalNumber("scaleY", 100)))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real intensity: root.intensity / 100.0
        property int mapType: root.mapType
        property real scaleX: root.scaleX / 100.0
        property real scaleY: root.scaleY / 100.0
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "displacement_map.frag.qsb"
    }

}