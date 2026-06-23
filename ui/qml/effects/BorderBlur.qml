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

    uniformMapping: ({
        "blur_alpha": "blurAlpha"
    })

    function buildUniforms() {
        var result = {};
        var _ = root._rev;
        if (!root.params) return result;
        var keys = Object.keys(root.params);
        for (var i = 0; i < keys.length; i++) {
            var key = keys[i];
            var val = root.evalParam(key, root.params[key]);
            if (typeof val === "boolean") val = val ? 1 : 0;
            var uniformName = root.uniformMapping[key] || key;
            result[uniformName] = val;
        }
        result["size"] = root.effectSize;
        result["aspect"] = root.aspect;
        result["time"] = root.frame;
        return result;
    }

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
