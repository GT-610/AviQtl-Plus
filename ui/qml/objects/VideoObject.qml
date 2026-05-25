import QtMultimedia
import QtQuick
import QtQuick3D
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseObject {
    id: base

    property string path: String(evalParam("video", "path", ""))
    property string playMode: String(evalParam("video", "playMode", "開始フレーム＋再生速度"))
    property int startFrame: Number(evalParam("video", "startFrame", 0))
    property real speed: Number(evalParam("video", "speed", 100))
    property int directFrame: Math.ceil(Number(evalParam("video", "directFrame", 0)))
    property real opacity: Number(evalParam("video", "opacity", 1))
    property var source: undefined
    property var params: ({
    })
    property var effectModel: null
    property int frame: 0
    property int width: containerItem.width
    property int height: containerItem.height
    property int updateCounter: 0
    property string instanceKey: String(base.clipId)

    onRelFrameChanged: {
        if (Workspace.currentTimeline && typeof Workspace.currentTimeline.requestVideoFrame === "function" && base.clipId > 0)
            Workspace.currentTimeline.requestVideoFrame(base.clipId, base.relFrame);

    }
    Component.onCompleted: {
        if (Workspace.currentTimeline && typeof Workspace.currentTimeline.requestVideoFrame === "function" && base.clipId > 0)
            Workspace.currentTimeline.requestVideoFrame(base.clipId, base.relFrame);

    }

    Connections {
        function onFrameUpdated(key) {
            if (key === base.instanceKey)
                base.updateCounter++;

        }

        target: videoFrameStore
    }

    Model {
        source: "#Rectangle"
        visible: base.outputModelVisible
        scale: Qt.vector3d((base.displayOutput && base.displayOutput.sourceItem ? base.displayOutput.sourceItem.width : base.sourceItem.width) / 100, (base.displayOutput && base.displayOutput.sourceItem ? base.displayOutput.sourceItem.height : base.sourceItem.height) / 100, 1)
        opacity: base.opacity

        materials: DefaultMaterial {
            lighting: DefaultMaterial.NoLighting
            blendMode: DefaultMaterial.SourceOver

            diffuseMap: Texture {
                sourceItem: base.displayOutput
            }

        }

    }

    sourceItem: Item {
        id: containerItem

        // 動画素材の本来のサイズに合わせる
        width: videoOut.width
        height: videoOut.height
        implicitWidth: width
        implicitHeight: height
        visible: false

        VideoOutput {
            id: videoOut

            // sourceRect (デコードされた動画の解像度) を使用する
            width: sourceRect.width
            height: sourceRect.height
            fillMode: VideoOutput.Stretch
            opacity: base.opacity
            // FBOキャプチャの黒画面制約を突破するGPUレイヤー化
            layer.enabled: true
            layer.format: ShaderEffectSource.RGBA
            Component.onCompleted: {
                if (base.clipId > 0 && typeof videoFrameStore !== "undefined")
                    videoFrameStore.registerSink(base.instanceKey, videoOut.videoSink);

            }

            Connections {
                function onClipIdChanged() {
                    if (base.clipId > 0 && typeof videoFrameStore !== "undefined")
                        videoFrameStore.registerSink(base.instanceKey, videoOut.videoSink);

                }

                function onInstanceKeyChanged() {
                    if (base.clipId > 0 && typeof videoFrameStore !== "undefined")
                        videoFrameStore.registerSink(base.instanceKey, videoOut.videoSink);

                }

                target: base
            }

        }

    }

}
