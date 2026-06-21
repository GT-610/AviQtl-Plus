import QtQuick

Item {
    id: transitionRoot

    property int duration: 30
    property string easing: "linear"
    property bool reverse: false
    property real centerX: 0.5
    property real centerY: 0.5
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

    // 最大半径を計算
    property real maxRadius: {
        var cx = width * centerX;
        var cy = height * centerY;
        var dx = Math.max(cx, width - cx);
        var dy = Math.max(cy, height - cy);
        return Math.sqrt(dx * dx + dy * dy);
    }

    // 進行状況を更新
    onProgressChanged: {
        canvas.requestPaint();
    }

    // アニメーション
    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }

    // 円形ワイプ用キャンバス
    Canvas {
        id: canvas
        anchors.fill: parent

        onPaint: {
            var ctx = getContext("2d");
            ctx.clearRect(0, 0, width, height);

            var cx = width * centerX;
            var cy = height * centerY;
            var p = reverse ? (1.0 - progress) : progress;
            var radius = maxRadius * p;

            // 前のシーンを描画（黒でマスク）
            ctx.fillStyle = "black";
            ctx.fillRect(0, 0, width, height);

            // 円形の穴を開ける
            ctx.globalCompositeOperation = "destination-out";
            ctx.beginPath();
            ctx.arc(cx, cy, radius, 0, Math.PI * 2);
            ctx.fill();

            // 次のシーンを描画
            ctx.globalCompositeOperation = "destination-over";
            ctx.fillStyle = "white";
            ctx.fillRect(0, 0, width, height);
        }
    }

    // デバッグ用テキスト
    Text {
        anchors.centerIn: parent
        text: "Wipe Circle"
        color: "white"
        font.pixelSize: 24
        visible: false
    }
}