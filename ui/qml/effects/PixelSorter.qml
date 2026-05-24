import QtQuick
import QtQuick.Controls
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    // デフォルト値など

    id: root

    // 1. 確実に fbCaptureItem を持つ親を遡って探す
    source: {
        var p = parent;
        while (p) {
            // fbCaptureItem を持ち、かつそれが単なる数値や自分自身ではない有効なオブジェクトであることを確認
            if (p.fbCaptureItem !== undefined && p.fbCaptureItem !== null && typeof p.fbCaptureItem === "object" && p.fbCaptureItem !== root)
                return p.fbCaptureItem;

            p = p.parent;
        }
        console.warn("[PixelSorter] fbCaptureItem could not be found in any parent!");
        return null;
    }
    // デバッグ用: バインディングの状態をコンソールに出力
    onSourceChanged: console.log("[PixelSorter] source updated to:", source)
    // Qt.resolvedUrl を使うことで、この QML ファイルと同じディレクトリにある QSB を絶対パスで指定できます
    computeShader: Qt.resolvedUrl("pixelsorter.comp.qsb")
    // C++ の params プロパティに直接マップする（BaseComputeEffect の実装に依存）
    params: ({
        "mix": 1
    })

    // デバッグ用: シェーダーエラーの表示
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
