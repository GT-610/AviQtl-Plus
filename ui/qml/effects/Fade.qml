import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    property real fadeIn: Math.max(0, Math.min(100, root.evalNumber("fadeIn", 0)))
    property real fadeOut: Math.max(0, Math.min(100, root.evalNumber("fadeOut", 0)))
    property real fadeInDuration: Math.max(1, Math.min(300, root.evalNumber("fadeInDuration", 30)))
    property real fadeOutDuration: Math.max(1, Math.min(300, root.evalNumber("fadeOutDuration", 30)))

    // Calculate opacity based on frame position
    readonly property real currentFrame: root.frame
    readonly property real opacityValue: {
        if (fadeIn > 0 && currentFrame < fadeInDuration) {
            // Fade in phase
            return currentFrame / fadeInDuration
        } else if (fadeOut > 0 && currentFrame > (root.effectModel.clipDurationFrames - fadeOutDuration)) {
            // Fade out phase
            return (root.effectModel.clipDurationFrames - currentFrame) / fadeOutDuration
        } else {
            // Normal phase
            return 1.0
        }
    }

    ShaderEffect {
        property var source: root.sourceProxy
        property real opacityMultiplier: root.opacityValue

        anchors.fill: parent
        fragmentShader: "fade.frag.qsb"
    }

}