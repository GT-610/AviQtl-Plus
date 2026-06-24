import QtQuick

Item {
    id: transitionRoot

    property int duration: 30
    property string easing: "ease_in_out"
    property bool reverse: false
    property real progress: 0.0

    property var previousScene: null
    property var nextScene: null

    function getEasingType() {
        switch (easing) {
        case "ease_in":    return Easing.InQuad;
        case "ease_out":   return Easing.OutQuad;
        case "ease_in_out": return Easing.InOutQuad;
        default:           return Easing.Linear;
        }
    }

    onProgressChanged: {
        var p = reverse ? (1.0 - progress) : progress;
        var q = 1.0 - p;

        if (previousScene) {
            previousScene.opacity = q;
        }
        if (nextScene) {
            nextScene.opacity = p;
            nextScene.scale = 0.5 + p * 0.5;
            nextScene.transformOrigin = Item.Center;
        }
    }

    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }
}
