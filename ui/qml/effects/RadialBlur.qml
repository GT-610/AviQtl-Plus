import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real range: Math.max(0, root.evalNumber("strength", 0.1) * 2000)
    property real centerX: root.evalNumber("x", 0)
    property real centerY: root.evalNumber("y", 0)
    property bool fixedSize: root.evalParam("fixedSize", false)
    property real samples: root.evalNumber("samples", 10)
    readonly property real strength: range

    computeShader: "radialblur_compute.comp.qsb"
}
