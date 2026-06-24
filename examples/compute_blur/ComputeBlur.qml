import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real radius: Math.max(0, Math.min(32, Math.round(root.evalNumber("radius", 5))))

    computeShader: "compute_blur.comp.qsb"
    dispatchCount: 2
}
