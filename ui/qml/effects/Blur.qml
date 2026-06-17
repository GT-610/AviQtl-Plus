import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real size: Math.max(0, root.evalNumber("size", 5))
    property int quality: Math.max(1, Math.min(3, Math.round(root.evalNumber("quality", 1))))

    ShaderEffect {
        property variant source: root.sourceProxy
        property real size: root.size
        property real quality: root.quality
        property real targetWidth: root.width
        property real targetHeight: root.height

        anchors.fill: parent
        fragmentShader: "blur.frag.qsb"
    }

}