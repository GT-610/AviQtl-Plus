import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    computeShader: "lensblur_compute.comp.qsb"

    function buildUniforms() {
        var result = Common.BaseComputeEffect.prototype.buildUniforms.call(root);
        result["targetWidth"] = root.source ? root.source.width : 0;
        result["targetHeight"] = root.source ? root.source.height : 0;
        return result;
    }
}
