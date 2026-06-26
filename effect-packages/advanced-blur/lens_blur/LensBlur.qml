import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real range: Math.max(0, root.evalNumber("radius", 10))
    property real lightPower: Math.max(0, root.evalNumber("brightness", 0))
    property bool fixedSize: root.evalParam("fixedSize", false)

    computeShader: "lensblur_compute.comp.qsb"
}
