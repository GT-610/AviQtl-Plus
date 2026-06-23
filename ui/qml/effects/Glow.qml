import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real intensity: Math.max(0, Math.min(200, root.evalNumber("intensity", 100)))
    property real radius: Math.max(0, Math.min(50, root.evalNumber("radius", 10)))
    property real threshold: Math.max(0, Math.min(100, root.evalNumber("threshold", 50)))
    property color glowColor: root.evalColor("color", "#ffffff")

    computeShader: "glow_compute.comp.qsb"
    dispatchCount: 2
}
