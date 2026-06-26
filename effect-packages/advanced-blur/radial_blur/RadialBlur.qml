import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    property real range: Math.max(0, root.evalNumber("strength", 0.1) * 2000)

    computeShader: "radialblur_compute.comp.qsb"

    property var uniformMapping: ({
        "x": "centerX",
        "y": "centerY"
    })

    function buildUniforms() {
        var result = Common.BaseComputeEffect.prototype.buildUniforms.call(root);
        result["strength"] = root.range;
        result["targetWidth"] = root.source ? root.source.width : 0;
        result["targetHeight"] = root.source ? root.source.height : 0;
        return result;
    }
}
