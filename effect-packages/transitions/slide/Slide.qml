import QtQuick

Item {
    id: transitionRoot

    property int duration: 30
    property string easing: "linear"
    property bool reverse: false
    property string direction: "left"
    property real progress: 0.0

    // 前のシーンと次のシーンを受け取るプロパティ
    property var previousScene: null
    property var nextScene: null

    // イージング関数
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

    // 方向に基づいてオフセットを計算（前のシーン用：画面外へスライドアウト）
    function getOffset() {
        var offset = progress;
        if (reverse) {
            offset = 1.0 - offset;
        }

        switch (direction) {
        case "left":
            return Qt.point(offset * width, 0);
        case "right":
            return Qt.point(-offset * width, 0);
        case "up":
            return Qt.point(0, offset * height);
        case "down":
            return Qt.point(0, -offset * height);
        default:
            return Qt.point(0, 0);
        }
    }

    // 次のシーン用オフセット（画面外からスライドイン）
    function getIncomingOffset() {
        var offset = 1.0 - progress;
        if (reverse) {
            offset = progress;
        }

        switch (direction) {
        case "left":
            return Qt.point(-offset * width, 0);
        case "right":
            return Qt.point(offset * width, 0);
        case "up":
            return Qt.point(0, -offset * height);
        case "down":
            return Qt.point(0, offset * height);
        default:
            return Qt.point(0, 0);
        }
    }

    // アニメーション
    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }

    // 前のシーン（スライドアウト）
    Item {
        id: previousContainer
        anchors.fill: parent
        x: transitionRoot.getOffset().x
        y: transitionRoot.getOffset().y

        Behavior on x {
            NumberAnimation {
                duration: transitionRoot.duration * (1000 / 60)
                easing.type: transitionRoot.getEasingType()
            }
        }

        Behavior on y {
            NumberAnimation {
                duration: transitionRoot.duration * (1000 / 60)
                easing.type: transitionRoot.getEasingType()
            }
        }

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.previousScene
        }
    }

    // 次のシーン（スライドイン）
    Item {
        id: nextContainer
        anchors.fill: parent
        x: transitionRoot.getIncomingOffset().x
        y: transitionRoot.getIncomingOffset().y

        Behavior on x {
            NumberAnimation {
                duration: transitionRoot.duration * (1000 / 60)
                easing.type: transitionRoot.getEasingType()
            }
        }

        Behavior on y {
            NumberAnimation {
                duration: transitionRoot.duration * (1000 / 60)
                easing.type: transitionRoot.getEasingType()
            }
        }

        Loader {
            anchors.fill: parent
            sourceComponent: transitionRoot.nextScene
        }
    }

    // デバッグ用テキスト
    Text {
        anchors.centerIn: parent
        text: "Slide"
        color: "white"
        font.pixelSize: 24
        visible: false
    }
}