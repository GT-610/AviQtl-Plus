import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real radius: Math.max(0, root.evalNumber("radius", 10))
    property color colorVal: root.evalColor("color", "#80000000")
    property real xOffset: root.evalNumber("x", 5)
    property real yOffset: root.evalNumber("y", 5)
    property real opacityVal: Math.max(0, Math.min(100, root.evalNumber("opacity", 100))) / 100

    computeShader: "dropshadow_compute.comp.qsb"
    dispatchCount: 2
    extraTextures: [root.sourceProxy]
}
