import AviQtl
import QtQuick
import QtQuick.Controls

BaseEffect {
    id: root

    property alias computeShader: compEffect.shaderPath
    property alias autoWorkGroup: compEffect.autoWorkGroup
    property alias workGroupSizeX: compEffect.workGroupSizeX
    property alias workGroupSizeY: compEffect.workGroupSizeY
    property alias computeError: compEffect.error
    // JSONのパラメータ名とシェーダーのUniform名が異なる場合のマッピング
    // 例: { "mix": "mixAmount" }
    property var uniformMapping: ({
    })

    source: {
        var p = parent;
        while (p) {
            if (p.fbCaptureItem !== undefined && p.fbCaptureItem !== null && p.fbCaptureItem !== root && p.fbCaptureItem.hasOwnProperty("recursive"))
                return p.fbCaptureItem;
            p = p.parent;
        }
        return null;
    }

    // params からキーフレーム評価済みの Uniform オブジェクトを自動構築する
    function buildUniforms() {
        var result = {
        };
        // _rev を参照することで、キーフレームの現在値が変化した際に再評価される
        var _ = root._rev;
        if (!root.params)
            return result;

        var keys = Object.keys(root.params);
        for (var i = 0; i < keys.length; i++) {
            var key = keys[i];
            var val = root.evalParam(key, root.params[key]);
            // bool型はシェーダー用に1/0に変換
            if (typeof val === "boolean")
                val = val ? 1 : 0;

            var uniformName = root.uniformMapping[key] || key;
            result[uniformName] = val;
        }
        // time は現在のフレーム番号を自動注入
        result["time"] = root.frame;
        return result;
    }

    ComputeEffect {
        id: compEffect

        anchors.fill: parent
        source: root.sourceProxy
        params: root.buildUniforms()
        autoWorkGroup: true
    }

    Label {
        anchors.centerIn: parent
        text: "Compute Error:\n" + root.computeError
        color: "red"
        font.bold: true
        visible: root.computeError !== undefined && root.computeError !== ""
        horizontalAlignment: Text.AlignHCenter

        background: Rectangle {
            color: "black"
            opacity: 0.7
            radius: 4
        }
    }
}
