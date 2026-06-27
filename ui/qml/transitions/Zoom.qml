import QtQuick

Item {
    id: transitionRoot

    property int duration: 30
    property string easing: "linear"
    property bool reverse: false
    property real zoomScale: 1.5
    property real progress: 0.0

    property var previousScene: null
    property var nextScene: null

    function getEasingType() {
        switch (easing) {
        case "ease_in":
            return Easing.InQuad;
        case "ease_out":
            return Easing.OutQuad;
        case "ease_in_out":
            return Easing.InOutQuad;
        default:
            return Easing.Linear;
        }
    }

    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }

    Item {
        id: previousContainer
        anchors.fill: parent
        scale: reverse ? (1.0 + (zoomScale - 1.0) * (1.0 - progress)) : (1.0 + (zoomScale - 1.0) * progress)
        opacity: reverse ? progress : (1.0 - progress)

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.previousScene
        }
    }

    Item {
        id: nextContainer
        anchors.fill: parent
        scale: reverse ? (zoomScale - (zoomScale - 1.0) * progress) : (zoomScale - (zoomScale - 1.0) * (1.0 - progress))
        opacity: reverse ? (1.0 - progress) : progress

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.nextScene
        }
    }
}
