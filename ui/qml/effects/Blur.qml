import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real size: Math.max(0, root.evalNumber("size", 5))
    property real quality: Math.max(1, Math.min(3, Math.round(root.evalNumber("quality", 1))))

    computeShader: "blur_compute.comp.qsb"
    dispatchCount: 2
}
