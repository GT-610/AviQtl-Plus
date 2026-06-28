import QtQuick

Item {
    id: transitionRoot

    property int duration: 30
    property string easing: "linear"
    property bool reverse: false
    property string direction: "left"
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

    readonly property real p: reverse ? (1.0 - progress) : progress

    Item {
        id: previousContainer
        anchors.fill: parent

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.previousScene
        }
    }

    Item {
        id: nextContainer
        clip: true
        x: (direction === "right") ? parent.width * (1.0 - p) : 0
        y: (direction === "down") ? parent.height * (1.0 - p) : 0
        width: (direction === "left" || direction === "right") ? parent.width * p : parent.width
        height: (direction === "up" || direction === "down") ? parent.height * p : parent.height

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.nextScene
        }
    }
}
