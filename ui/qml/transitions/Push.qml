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

    function getOffset(p) {
        switch (direction) {
        case "left":
            return Qt.point(-p * width, 0);
        case "right":
            return Qt.point(p * width, 0);
        case "up":
            return Qt.point(0, -p * height);
        case "down":
            return Qt.point(0, p * height);
        default:
            return Qt.point(0, 0);
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
        x: getOffset(reverse ? (1.0 - progress) : progress).x
        y: getOffset(reverse ? (1.0 - progress) : progress).y

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.previousScene
        }
    }

    Item {
        id: nextContainer
        anchors.fill: parent
        x: getOffset(reverse ? progress : (progress - 1.0)).x
        y: getOffset(reverse ? progress : (progress - 1.0)).y

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.nextScene
        }
    }
}
