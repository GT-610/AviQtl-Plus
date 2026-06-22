import QtQuick
import AviQtl.UI 1.0
import "common" as Common

Item {
    id: root

    property int sceneId: -1
    property int currentFrame: 0
    property var timelineBridge: null
    property var sceneStack: sceneId >= 0 ? [sceneId] : []
    property int sceneWidth: DefaultWidth
    property int sceneHeight: DefaultHeight
    property var ecsRenderData: ({})

    property var sceneInfo: {
        if (!timelineBridge || sceneId < 0)
            return null;

        var scenes = timelineBridge.scenes;
        for (var i = 0; i < scenes.length; i++) {
            if (scenes[i].id === sceneId)
                return scenes[i];

        }
        return null;
    }

    onSceneInfoChanged: {
        if (sceneInfo) {
            sceneWidth = sceneInfo.width || DefaultWidth;
            sceneHeight = sceneInfo.height || DefaultHeight;
        }
    }
    width: sceneWidth
    height: sceneHeight

    function updateEcsData() {
        var states = ECSRenderBridge.renderStates;
        var map = {};
        for (var i = 0; i < states.length; i++) {
            var s = states[i];
            map[s.clipId] = s;
        }
        root.ecsRenderData = map;
    }

    Connections {
        target: ECSRenderBridge
        function onRenderStatesChanged() {
            root.updateEcsData();
        }
    }

    Component.onCompleted: root.updateEcsData()

    CompositeView {
        id: compositeView

        anchors.fill: parent
        clipModel: {
            if (root.timelineBridge && root.sceneId >= 0)
                return root.timelineBridge.getSceneClips(root.sceneId);

            return [];
        }
        sceneId: root.sceneId
        sceneStack: root.sceneStack
        projectWidth: root.sceneWidth
        projectHeight: root.sceneHeight
        currentFrame: root.currentFrame
        ecsRenderData: root.ecsRenderData
        layerStates: {
            var tlWin = WindowManager.getWindow("timeline");
            return tlWin ? tlWin.globalLayerStates : ({
            });
        }

        Connections {
            function onGlobalLayerStatesChanged() {
                compositeView.layerStates = WindowManager.getWindow("timeline").globalLayerStates;
            }

            target: WindowManager.getWindow("timeline")
        }

    }

}
