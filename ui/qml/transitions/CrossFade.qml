import QtQuick

Item {
    id: transitionRoot

    property int duration: 30
    property string easing: "linear"
    property bool reverse: false
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

    // 進行状況を更新
    onProgressChanged: {
        if (previousScene) {
            previousScene.opacity = reverse ? progress : (1.0 - progress);
        }
        if (nextScene) {
            nextScene.opacity = reverse ? (1.0 - progress) : progress;
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

    // 前のシーン表示用Rectangle
    Rectangle {
        id: previousOverlay
        anchors.fill: parent
        color: "black"
        opacity: reverse ? 1.0 : 0.0

        Behavior on opacity {
            NumberAnimation {
                duration: transitionRoot.duration * (1000 / 60)
                easing.type: transitionRoot.getEasingType()
            }
        }
    }

    // 次のシーン表示用Rectangle
    Rectangle {
        id: nextOverlay
        anchors.fill: parent
        color: "white"
        opacity: reverse ? 0.0 : 1.0

        Behavior on opacity {
            NumberAnimation {
                duration: transitionRoot.duration * (1000 / 60)
                easing.type: transitionRoot.getEasingType()
            }
        }
    }

    // デバッグ用テキスト
    Text {
        anchors.centerIn: parent
        text: "Cross Fade"
        color: "white"
        font.pixelSize: 24
        visible: false
    }
}