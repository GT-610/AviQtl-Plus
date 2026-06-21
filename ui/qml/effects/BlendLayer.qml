import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    computeShader: Qt.resolvedUrl("blend_layer.comp.qsb")
}
