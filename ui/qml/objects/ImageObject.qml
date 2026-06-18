import QtMultimedia
import QtQuick
import QtQuick3D
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseObject {
    id: base

    property string imagePath: String(evalParam("image", "path", ""))
    property int fillMode: Number(evalParam("image", "fillMode", VideoOutput.PreserveAspectFit))
    property real imageOpacity: Number(evalParam("image", "opacity", 1))
    outputModelOpacity: base.imageOpacity
    property int updateCounter: 0
    property string instanceKey: String(base.clipId)

    sourceItem: containerItem
    onImagePathChanged: {
        if (Workspace.currentTimeline && typeof Workspace.currentTimeline.requestImageLoad === "function" && base.clipId > 0)
            Workspace.currentTimeline.requestImageLoad(base.clipId, base.imagePath);

    }
    Component.onCompleted: {
        if (Workspace.currentTimeline && typeof Workspace.currentTimeline.requestImageLoad === "function" && base.clipId > 0)
            Workspace.currentTimeline.requestImageLoad(base.clipId, base.imagePath);

    }

    Connections {
        function onFrameUpdated(key) {
            if (key === base.instanceKey)
                base.updateCounter++;

        }

        target: videoFrameStore
    }

    Common.DisplayModel {
        baseObject: base
    }

    Item {
        id: containerItem

        // プロジェクトサイズへの強制リサイズを解除し、ソース本来のサイズに合わせます
        width: videoOut.width
        height: videoOut.height
        implicitWidth: width
        implicitHeight: height
        visible: false

        VideoOutput {
            id: videoOut

            // sourceRect (デコードされたフレームの解像度) を使用することで 1:1 表示を実現します
            width: sourceRect.width
            height: sourceRect.height
            fillMode: base.fillMode
            opacity: base.imageOpacity
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
