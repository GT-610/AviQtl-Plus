import QtQuick
import QtQuick.Controls
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    // Compute Shader のファイルパスを指定するだけ
    computeShader: "pixelsorter.comp.qsb"
    uniformMapping: {
        "mix": "mixAmount"
    }

    // デバッグ用: シェーダーエラーの表示
    Label {
        anchors.centerIn: parent
        text: "Compute Error:\n" + root.computeError
        color: "red"
        font.bold: true
        visible: root.computeError !== ""
        horizontalAlignment: Text.AlignHCenter

        background: Rectangle {
            color: "black"
            opacity: 0.7
            radius: 4
        }

    }

}
