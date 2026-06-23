import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real size: Math.max(0, root.evalNumber("size", 5))
    property real aspect: Math.max(-100, Math.min(100, root.evalNumber("aspect", 0)))
    property bool blurAlpha: root.evalParam("blur_alpha", true)
    readonly property real hRatio: aspect > 0 ? (100 - aspect) / 100 : 1
    readonly property real vRatio: aspect < 0 ? (100 + aspect) / 100 : 1
    readonly property real effectSize: size * Math.max(hRatio, vRatio)

    computeShader: "borderblend.comp.qsb"
    dispatchCount: 3
    extraTextures: [origSourceProxy]

    ShaderEffectSource {
        id: origSourceProxy
        sourceItem: root.source
        hideSource: false
        visible: false
    }
}
