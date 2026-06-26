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
    readonly property real maxRadius: {
        var cxc = width * centerX;
        var cyc = height * centerY;
        var dx = Math.max(cxc, width - cxc);
        var dy = Math.max(cyc, height - cyc);
        return Math.sqrt(dx * dx + dy * dy);
    }

    // アニメーション
    NumberAnimation on progress {
        from: 0.0
        to: 1.0
        duration: transitionRoot.duration * (1000 / 60)
        easing.type: transitionRoot.getEasingType()
        running: true
    }

    // 前のシーンと次のシーンをレイヤーとしてキャプチャ
    ShaderEffectSource {
        id: prevSource
        sourceItem: prevLoader
        visible: false
        hideSource: false
    }

    ShaderEffectSource {
        id: nextSource
        sourceItem: nextLoader
        visible: false
        hideSource: false
    }

    // 前のシーン
    Item {
        id: prevContainer
        anchors.fill: parent
        Loader { id: prevLoader; anchors.fill: parent; sourceComponent: transitionRoot.previousScene }
    }

    // 次のシーン
    Item {
        id: nextContainer
        anchors.fill: parent
        Loader { id: nextLoader; anchors.fill: parent; sourceComponent: transitionRoot.nextScene }
    }

    // 円形ワイプ合成
    ShaderEffect {
        anchors.fill: parent
        property variant prevTexture: prevSource
        property variant nextTexture: nextSource
        property real cx: transitionRoot.centerX
        property real cy: transitionRoot.centerY
        property real wipeRadius: transitionRoot.maxRadius * (transitionRoot.reverse ? (1.0 - transitionRoot.progress) : transitionRoot.progress)
        property real targetWidth: parent.width
        property real targetHeight: parent.height

        fragmentShader: "wipe_circle.frag.qsb"
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