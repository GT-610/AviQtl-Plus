import QtQuick
import QtQuick.Controls
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseComputeEffect {
    id: root

    source: {
        var p = parent;
        while (p) {
            if (p.fbCaptureItem !== undefined && p.fbCaptureItem !== null && p.fbCaptureItem !== root && p.fbCaptureItem.hasOwnProperty("recursive"))
                return p.fbCaptureItem;
            p = p.parent;
        }
        return null;
    }
    computeShader: Qt.resolvedUrl("glitch.comp.qsb")

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
