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

    onProgressChanged: {
        canvas.requestPaint();
    }

    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }

    Canvas {
        id: canvas
        anchors.fill: parent

        onPaint: {
            var ctx = getContext("2d");
            ctx.clearRect(0, 0, width, height);

            var p = reverse ? (1.0 - progress) : progress;

            ctx.fillStyle = "black";
            ctx.fillRect(0, 0, width, height);

            ctx.globalCompositeOperation = "destination-out";
            ctx.beginPath();

            switch (direction) {
            case "left":
                ctx.rect(0, 0, width * p, height);
                break;
            case "right":
                ctx.rect(width * (1.0 - p), 0, width * p, height);
                break;
            case "up":
                ctx.rect(0, 0, width, height * p);
                break;
            case "down":
                ctx.rect(0, height * (1.0 - p), width, height * p);
                break;
            }
            ctx.fill();

            ctx.globalCompositeOperation = "destination-over";
            ctx.fillStyle = "white";
            ctx.fillRect(0, 0, width, height);
        }
    }
}
