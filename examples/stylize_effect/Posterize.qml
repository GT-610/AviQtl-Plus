import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real levels: Math.max(2, Math.min(32, Math.round(root.evalNumber("levels", 8))))
    property real dither: Math.max(0, Math.min(1, root.evalNumber("dither", 0)))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real levels: root.levels
        property real dither: root.dither
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "posterize.frag.qsb"
    }
}
