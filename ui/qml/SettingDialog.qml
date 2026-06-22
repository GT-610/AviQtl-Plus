import Qt.labs.qmlmodels
import QtQml
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import "common" as Common
import "common/Logger.js" as Logger

Common.AviQtlWindow {
    id: root

    property int targetClipId: (Workspace.currentTimeline && Workspace.currentTimeline.selection) ? Workspace.currentTimeline.selection.selectedClipId : -1
    property var effectsModel: []
    property var audioEffectsModel: []
    property bool inputting: false // 入力中フラグ（reloadループ防止用）
    property bool reloading: false
    property bool isDeleting: false // 複数エフェクト削除中フラグ（途中reload抑制用）
    property bool enableSnap: SettingsManager && SettingsManager.settings ? SettingsManager.settings.enableSnap : true
    property bool sidebarOnRight: (SettingsManager && SettingsManager.settings && SettingsManager.settings.settingDialogSidebarRight !== undefined) ? SettingsManager.settings.settingDialogSidebarRight : false
    readonly property real _projectFps: (Workspace.currentTimeline && Workspace.currentTimeline.project) ? Workspace.currentTimeline.project.fps : 60
    readonly property bool isAudioWorkspaceClip: Workspace.currentTimeline && Workspace.currentTimeline.activeObjectType === "audio"
    property real audioPeakLeft: 0
    property real audioPeakRight: 0
    property real audioRmsLeft: 0
    property real audioRmsRight: 0
    property int _audioKfRev: 0
    readonly property int _audioClipDur: Workspace.currentTimeline ? Workspace.currentTimeline.clipDurationFrames : 100
    readonly property int _audioRelFrame: (Workspace.currentTimeline && Workspace.currentTimeline.transport) ? Math.max(0, Workspace.currentTimeline.transport.currentFrame - Workspace.currentTimeline.clipStartFrame) : 0
    readonly property var _cachedAudioModel: {
        var _ = _audioKfRev;
        for (var i = 0; i < effectsModel.length; i++) {
            if (effectsModel[i] && effectsModel[i].id === "audio")
                return effectsModel[i];
        }
        return null;
    }
    readonly property int _cachedAudioIdx: {
        var _ = _audioKfRev;
        for (var i = 0; i < effectsModel.length; i++) {
            if (effectsModel[i] && effectsModel[i].id === "audio")
                return i;
        }
        return -1;
    }

    function audioEffectModel() { return _cachedAudioModel; }

    function audioEffectIndex() { return _cachedAudioIdx; }

    function audioParamValue(paramName, fallback) {
        var model = audioEffectModel();
        if (!model || !model.params || model.params[paramName] === undefined)
            return fallback;
        return model.params[paramName];
    }

    function audioEvaluatedParam(paramName, fallback) {
        var _ = root._audioKfRev;
        var model = audioEffectModel();
        if (!model)
            return fallback;
        var curFrame = (Workspace.currentTimeline && Workspace.currentTimeline.transport) ? Math.max(0, Workspace.currentTimeline.transport.currentFrame - Workspace.currentTimeline.clipStartFrame) : 0;
        var v = model.evaluatedParam(paramName, curFrame, root._projectFps);
        return (v !== undefined && v !== null) ? v : fallback;
    }

    function audioKeyframeList(paramName) {
        var _ = root._audioKfRev;
        var model = audioEffectModel();
        return model ? model.keyframeListForUi(paramName) : [];
    }

    function audioKeyframesWithEnd(rawKfs, totalDur, paramName) {
        var out = [];
        if (rawKfs) {
            for (var i = 0; i < rawKfs.length; i++) out.push(rawKfs[i]);
        }
        if (totalDur > 0) {
            var hasEnd = false;
            for (var j = 0; j < out.length; j++) {
                if (out[j].frame === totalDur) { hasEnd = true; break; }
            }
            if (!hasEnd) {
                var model = audioEffectModel();
                var endVal = model ? model.evaluatedParam(paramName || "", totalDur, root._projectFps) : 0;
                out.push({"frame": totalDur, "value": endVal, "interp": "none", "virtualEnd": true});
            }
        }
        out.sort(function(a, b) { return a.frame - b.frame; });
        return out;
    }

    function _audioInterpAt(paramName, frame) {
        var kfs = audioKeyframeList(paramName);
        for (var i = 0; i < kfs.length; i++) {
            if (kfs[i].frame === frame)
                return kfs[i].interp || "linear";
        }
        return "linear";
    }

    function _audioNextKfFrame(rawKfs) {
        for (var i = 0; i < rawKfs.length; i++) {
            if (rawKfs[i].frame > root._audioRelFrame)
                return rawKfs[i].frame;
        }
        return root._audioClipDur;
    }

    function setAudioParam(paramName, value) {
        var idx = audioEffectIndex();
        if (idx >= 0 && Workspace.currentTimeline)
            Workspace.currentTimeline.updateClipEffectParam(targetClipId, idx, paramName, value);
    }

    function currentSceneData() {
        if (!Workspace.currentTimeline || !Workspace.currentTimeline.scenes)
            return null;

        for (let i = 0; i < Workspace.currentTimeline.scenes.length; i++) {
            if (Workspace.currentTimeline.scenes[i].id === Workspace.currentTimeline.currentSceneId)
                return Workspace.currentTimeline.scenes[i];

        }
        return null;
    }

    function gridSettings() {
        const s = currentSceneData();
        if (s)
            return {
            "mode": s.gridMode || "Auto",
            "bpm": s.gridBpm || 120,
            "offset": s.gridOffset || 0,
            "interval": s.gridInterval || 10,
            "subdivision": s.gridSubdivision || 4
        };

        return {
            "mode": "Auto",
            "bpm": 120,
            "offset": 0,
            "interval": 10,
            "subdivision": 4
        };
    }

    function getGridInterval() {
        if (!Workspace.currentTimeline)
            return 1;

        const gs = gridSettings();
        const scale = Workspace.currentTimeline.timelineScale;
        const fps = (Workspace.currentTimeline.project && Workspace.currentTimeline.project.fps) ? Workspace.currentTimeline.project.fps : 60;
        if (gs.mode === "BPM") {
            const beatFrames = fps / (gs.bpm / 60);
            const bpmDiv = scale > 3 ? 4 : scale > 1.5 ? 2 : 1;
            return beatFrames / bpmDiv;
        }
        if (gs.mode === "Frame")
            return gs.interval;

        if (scale < 0.5)
            return Math.ceil(fps);

        if (scale < 1.5)
            return 10;

        return 1;
    }

    function snapRelativeFrame(relFrame) {
        if (!Workspace.currentTimeline || !enableSnap)
            return Math.max(0, Math.round(relFrame));

        const gs = gridSettings();
        const absFrame = Workspace.currentTimeline.clipStartFrame + relFrame;
        const step = getGridInterval();
        const offset = (gs.mode === "BPM" && Workspace.currentTimeline.project) ? gs.offset * Workspace.currentTimeline.project.fps : 0;
        const snappedAbs = Math.max(0, Math.round((Math.round((absFrame - offset) / step) * step) + offset));
        const newRelFrame = snappedAbs - Workspace.currentTimeline.clipStartFrame;
        // Don't snap outside the clip bounds if it goes negative
        return Math.max(0, newRelFrame);
    }

    function reload() {
        if (!Workspace.currentTimeline || !Workspace.currentTimeline.selection || reloading)
            return ;

        reloading = true;
        var id = Workspace.currentTimeline.selection.selectedClipId;
        var oldModel = sidebarList.model;
        var oldSelectedObjects = [];
        var oldCurrentObject = null;
        if (oldModel && sidebarList.selectedIndices) {
            for (var i = 0; i < sidebarList.selectedIndices.length; i++) {
                var idx = sidebarList.selectedIndices[i];
                if (idx >= 0 && idx < oldModel.length)
                    oldSelectedObjects.push(oldModel[idx]);

            }
            if (sidebarList.currentIndex >= 0 && sidebarList.currentIndex < oldModel.length)
                oldCurrentObject = oldModel[sidebarList.currentIndex];

        }
        if (id >= 0) {
            effectsModel = Workspace.currentTimeline.getClipEffectsModel(id);
            audioEffectsModel = Workspace.currentTimeline.getClipEffectStack(id);
        } else {
            effectsModel = [];
            audioEffectsModel = [];
        }
        // 保存したオブジェクト参照から新インデックスを復元
        var newModel = (Workspace.currentTimeline && Workspace.currentTimeline.isAudioClip(id)) ? audioEffectsModel : effectsModel;
        if (newModel && oldSelectedObjects.length > 0) {
            var newSel = [];
            for (var j = 0; j < newModel.length; j++) {
                if (oldSelectedObjects.indexOf(newModel[j]) !== -1)
                    newSel.push(j);

            }
            sidebarList.selectedIndices = newSel;
            var newCurrentIdx = newModel.indexOf(oldCurrentObject);
            if (newCurrentIdx !== -1)
                sidebarList.currentIndex = newCurrentIdx;
            else if (newSel.length > 0)
                sidebarList.currentIndex = newSel[newSel.length - 1];
        }
        reloading = false;
    }

    function executeEffectDelete(indices) {
        if (!Workspace.currentTimeline)
            return ;

        var isAudio = Workspace.currentTimeline.isAudioClip(targetClipId);
        var m = sidebarList.model;
        var toDelete = [];
        for (var i = 0; i < indices.length; i++) {
            var idx = indices[i];
            if (idx >= 0 && m && idx < m.length) {
                if (isAudio || (m[idx] && m[idx].kind === "effect"))
                    toDelete.push(idx);

            }
        }
        if (toDelete.length === 0) {
            sidebarList.clearSelection();
            return ;
        }
        toDelete.sort(function(a, b) {
            return b - a;
        });
        if (isAudio) {
            // AudioPlugin は clipsMutable ループ外で処理されるため従来方式
            root.isDeleting = true;
            for (var j = 0; j < toDelete.length; j++) {
                Workspace.currentTimeline.removeAudioPlugin(targetClipId, toDelete[j]);
            }
            root.isDeleting = false;
        } else {
            Workspace.currentTimeline.removeMultipleEffects(targetClipId, toDelete);
        }
        reload();
        sidebarList.clearSelection();
    }

    function scrollToEffect(index) {
        var isAudio = Workspace.currentTimeline && Workspace.currentTimeline.isAudioClip(targetClipId);
        var repeater = isAudio ? audioEffectsRepeater : videoEffectsRepeater;
        if (!repeater)
            return ;

        var item = repeater.itemAt(index);
        if (item) {
            // ターゲットのY座標を取得
            var targetY = item.y;
            // スクロール可能な最大値を計算 (コンテンツ全体の高さ - ビューポートの高さ)
            var maxScroll = Math.max(0, mainScrollView.contentHeight - mainScrollView.height);
            // ターゲット位置へワープ (0 ～ maxScroll の範囲に収める)
            mainScrollView.contentItem.contentY = Math.min(Math.max(0, targetY), maxScroll);
        }
    }

    function isEditableFocusItem(item) {
        if (!item)
            return false;

        return item.hasOwnProperty("echoMode") || (item.hasOwnProperty("selectionStart") && item.readOnly === false);
    }

    function clearInputFocusOutside(item, container, position) {
        if (!isEditableFocusItem(item) || !container || !position)
            return ;

        var localPos = item.mapFromItem(container, position.x, position.y);
        if (localPos.x < 0 || localPos.y < 0 || localPos.x > item.width || localPos.y > item.height)
            item.focus = false;

    }

    // UI定義を正規化してリストとして取得するヘルパー
    function getUiModel(effectModel) {
        if (!effectModel)
            return [];

        var ui = effectModel.uiDefinition;
        if (ui && ui.controls && typeof ui.controls.length === 'number')
            return ui.controls;

        console.error("Invalid effect uiDefinition: ui.controls is missing for", effectModel ? effectModel.name : "unknown");
        Logger.log("Invalid effect uiDefinition: ui.controls is missing for " + (effectModel ? effectModel.name : "unknown"));
        return [];
    }

    width: isAudioWorkspaceClip ? 820 : 350
    height: isAudioWorkspaceClip ? 620 : 500
    title: qsTr("設定ダイアログ")
    color: palette.window
    visible: true
    x: 500
    y: 200
    onVisibleChanged: {
        if (visible)
            Qt.callLater(reload);

    }

    Connections {
        function onSelectedClipIdChanged() {
            reload();
        }

        function onSelectedClipDataChanged() {
            if (!inputting && !root.isDeleting)
                reload();

        }

        target: Workspace.currentTimeline ? Workspace.currentTimeline.selection : null
    }

    Connections {
        function onClipEffectsChanged(clipId) {
            if (clipId === targetClipId && !root.isDeleting)
                reload();

        }

        function onAudioMeterChanged(clipId, peakLeft, peakRight, rmsLeft, rmsRight) {
            if (clipId !== targetClipId)
                return;

            audioPeakLeft = peakLeft;
            audioPeakRight = peakRight;
            audioRmsLeft = rmsLeft;
            audioRmsRight = rmsRight;
        }

        target: Workspace.currentTimeline ? Workspace.currentTimeline : null
    }

    Connections {
        function onKeyframeTracksChanged() {
            root._audioKfRev++;
        }

        function onParamsChanged() {
            root._audioKfRev++;
        }

        function onParamChanged(key, value) {
            root._audioKfRev++;
        }

        target: root.audioEffectModel()
        ignoreUnknownSignals: true
    }

    // タブ切り替えでプロジェクトが変わった際にモデルをリセットして再ロード
    Connections {
        function onCurrentTimelineChanged() {
            // 旧プロジェクトのサイドバー選択状態をクリア
            sidebarList.selectedIndices = [];
            sidebarList.currentIndex = -1;
            filterMenu._lastBuiltClipId = -2; // メニューキャッシュをリセット
            // エフェクトモデルを即座に空にしてから新プロジェクト向けに再ロード
            effectsModel = [];
            audioEffectsModel = [];
            Qt.callLater(reload);
        }

        target: Workspace
    }

    SplitView {
        id: settingsSplitView

        anchors.fill: parent
        orientation: Qt.Horizontal
        LayoutMirroring.enabled: root.sidebarOnRight
        LayoutMirroring.childrenInherit: true

        TapHandler {
            acceptedButtons: Qt.LeftButton
            gesturePolicy: TapHandler.WithinBounds
            onTapped: function(eventPoint) {
                root.clearInputFocusOutside(root.activeFocusItem, settingsSplitView, eventPoint.position);
            }
        }

        // エフェクト一覧サイドバー
        Rectangle {
            SplitView.preferredWidth: 200
            SplitView.minimumWidth: 150
            color: palette.midlight
            border.width: 1
            border.color: palette.mid

            ListView {
                id: sidebarList

                property int dragTargetIndex: -1
                property int dragSourceIndex: -1
                property var selectedIndices: []

                function toggleSelection(idx) {
                    var s = selectedIndices.slice();
                    var pos = s.indexOf(idx);
                    if (pos >= 0)
                        s.splice(pos, 1);
                    else
                        s.push(idx);
                    selectedIndices = s;
                    currentIndex = idx;
                }

                function rangeSelect(from, to) {
                    var s = [];
                    var lo = Math.min(from, to), hi = Math.max(from, to);
                    for (var i = lo; i <= hi; i++) s.push(i)
                    selectedIndices = s;
                    currentIndex = to;
                }

                function isSelected(idx) {
                    return selectedIndices.indexOf(idx) >= 0;
                }

                function clearSelection() {
                    selectedIndices = [];
                    currentIndex = -1;
                }

                anchors.fill: parent
                LayoutMirroring.enabled: false
                LayoutMirroring.childrenInherit: true
                clip: true
                // 選択中のクリップタイプに応じてモデルを切り替え
                model: (Workspace.currentTimeline && Workspace.currentTimeline.isAudioClip(targetClipId)) ? audioEffectsModel : effectsModel
                boundsBehavior: Flickable.StopAtBounds

                delegate: Item {
                    id: delegateRoot

                    width: sidebarList.width
                    height: 32
                    z: dragArea.drag.active ? 100 : 1

                    // ドロップ先を示すインジケーター（線）
                    Rectangle {
                        width: parent.width
                        height: 3
                        color: palette.highlight
                        visible: sidebarList.dragTargetIndex === index
                        z: 50
                        // ドラッグ元が自分より上なら下側に、下なら上側に線を引く
                        y: (sidebarList.dragSourceIndex < index) ? parent.height - height : 0
                    }

                    Item {
                        id: dragContainer

                        width: parent.width
                        height: parent.height

                        Rectangle {
                            anchors.fill: parent
                            color: (sidebarList.isSelected(index) || sidebarList.currentIndex === index) ? palette.highlight : (dragArea.drag.active ? palette.mid : "transparent")
                            opacity: (sidebarList.isSelected(index) || sidebarList.currentIndex === index) ? 0.2 : (dragArea.drag.active ? 0.8 : 1)
                        }

                        // 複数選択ドラッグ時のカウントバッジ
                        Rectangle {
                            visible: dragArea.drag.active && sidebarList.selectedIndices.length > 1 && sidebarList.isSelected(index)
                            width: 18
                            height: 18
                            radius: 9
                            color: palette.highlight
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: 4
                            z: 10

                            Text {
                                anchors.centerIn: parent
                                text: sidebarList.selectedIndices.length
                                color: palette.highlightedText || "#ffffff"
                                font.pixelSize: 11
                                font.bold: true
                            }

                        }

                        // 背景用クリック領域（他のコントロールの下敷きになるよう先に宣言）
                        MouseArea {
                            anchors.fill: parent
                            acceptedButtons: Qt.LeftButton | Qt.RightButton
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: (mouse) => {
                                if (mouse.button === Qt.RightButton) {
                                    if (!sidebarList.isSelected(index)) {
                                        sidebarList.selectedIndices = [index];
                                        sidebarList.currentIndex = index;
                                    }
                                    effectContextMenu.effectIndex = index;
                                    effectContextMenu.popup();
                                    return ;
                                }
                                if (mouse.modifiers & Qt.ControlModifier) {
                                    sidebarList.toggleSelection(index);
                                } else if (mouse.modifiers & Qt.ShiftModifier) {
                                    sidebarList.rangeSelect(sidebarList.currentIndex >= 0 ? sidebarList.currentIndex : 0, index);
                                } else {
                                    sidebarList.selectedIndices = [index];
                                    sidebarList.currentIndex = index;
                                }
                                root.scrollToEffect(index);
                            }
                        }

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            spacing: 8

                            // ドラッグ用ハンドル
                            Common.AviQtlIcon {
                                iconName: "drag_move_line" // 適切なアイコン名に変更してください
                                size: 16
                                color: palette.text
                                opacity: 0.5

                                MouseArea {
                                    // 選択状態の追従は reload() 内のオブジェクト参照復元で行われます。

                                    id: dragArea

                                    anchors.fill: parent
                                    preventStealing: true
                                    cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                                    drag.target: dragContainer
                                    drag.axis: Drag.YAxis
                                    onPressed: {
                                        sidebarList.interactive = false;
                                        sidebarList.dragSourceIndex = index;
                                    }
                                    onPositionChanged: (mouse) => {
                                        if (drag.active) {
                                            // アイテムの中心Y座標を親のリスト基準で計算
                                            var absoluteY = delegateRoot.y + dragContainer.y + (dragContainer.height / 2);
                                            var hoverIndex = sidebarList.indexAt(10, absoluteY);
                                            if (hoverIndex !== -1 && hoverIndex !== index)
                                                sidebarList.dragTargetIndex = hoverIndex;
                                            else
                                                sidebarList.dragTargetIndex = -1;
                                        }
                                    }
                                    onReleased: (mouse) => {
                                        sidebarList.interactive = true;
                                        var targetIndex = sidebarList.dragTargetIndex;
                                        sidebarList.dragTargetIndex = -1;
                                        dragContainer.x = 0;
                                        dragContainer.y = 0;
                                        if (targetIndex === -1 || targetIndex === index)
                                            return ;

                                        var isAudio = Workspace.currentTimeline.isAudioClip(targetClipId);
                                        var sel = sidebarList.selectedIndices;
                                        // 複数選択中 かつ ドラッグ元が選択範囲内 → 一括移動
                                        if (!isAudio && sel.length > 1 && sel.indexOf(index) >= 0)
                                            Workspace.currentTimeline.reorderMultipleEffects(targetClipId, sel, targetIndex);
                                        else if (isAudio)
                                            Workspace.currentTimeline.reorderAudioPlugins(targetClipId, index, targetIndex);
                                        else
                                            Workspace.currentTimeline.reorderEffects(targetClipId, index, targetIndex);
                                    }
                                }

                            }

                            // 有効・無効切り替えチェックボックス
                            CheckBox {
                                checked: modelData.enabled !== undefined ? modelData.enabled : true
                                Layout.preferredHeight: 20
                                Layout.preferredWidth: 20
                                onToggled: (checked) => {
                                    if (!Workspace.currentTimeline)
                                        return ;

                                    var targets = (sidebarList.isSelected(index) && sidebarList.selectedIndices.length > 1) ? sidebarList.selectedIndices : [index];
                                    var isAudio = Workspace.currentTimeline.isAudioClip(targetClipId);
                                    for (var i = 0; i < targets.length; i++) {
                                        if (isAudio)
                                            Workspace.currentTimeline.setAudioPluginEnabled(targetClipId, targets[i], checked);
                                        else
                                            Workspace.currentTimeline.setEffectEnabled(targetClipId, targets[i], checked);
                                    }
                                }
                            }

                            // エフェクト名 (クリックでワープ)
                            Label {
                                text: modelData.name
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                                color: palette.text
                            }

                        }

                    }

                }

            }

            Menu {
                id: effectContextMenu

                property int effectIndex: -1

                MenuItem {
                    text: {
                        var n = sidebarList.selectedIndices.length;
                        return n > 1 ? qsTr("選択した %1 件を削除").arg(n) : qsTr("削除");
                    }
                    enabled: {
                        var indices = sidebarList.selectedIndices.length > 0 ? sidebarList.selectedIndices : (effectContextMenu.effectIndex >= 0 ? [effectContextMenu.effectIndex] : []);
                        if (indices.length === 0)
                            return false;

                        var m = sidebarList.model;
                        if (!m)
                            return false;

                        var isAudio = Workspace.currentTimeline && Workspace.currentTimeline.isAudioClip(targetClipId);
                        for (var i = 0; i < indices.length; i++) {
                            var idx = indices[i];
                            if (idx >= 0 && idx < m.length)
                                if (isAudio || (m[idx] && m[idx].kind === "effect"))
                                return true;

                        }
                        return false;
                    }
                    onTriggered: {
                        if (!Workspace.currentTimeline)
                            return ;

                        var indices = sidebarList.selectedIndices.length > 0 ? sidebarList.selectedIndices.slice() : (effectContextMenu.effectIndex >= 0 ? [effectContextMenu.effectIndex] : []);
                        if (indices.length === 0)
                            return ;

                        root.executeEffectDelete(indices);
                    }
                }

                MenuSeparator {}

                MenuItem {
                    text: qsTr("プリセットを保存...")
                    enabled: effectContextMenu.effectIndex >= 0 && sidebarList.model && effectContextMenu.effectIndex < sidebarList.model.length
                    onTriggered: {
                        var em = sidebarList.model[effectContextMenu.effectIndex];
                        if (!em)
                            return;

                        presetSaveDialog.effectId = em.id;
                        presetSaveDialog.effectIndex = effectContextMenu.effectIndex;
                        presetSaveDialog.open();
                    }
                }

                Menu {
                    id: presetLoadMenu
                    title: qsTr("プリセットを読み込み")
                    enabled: effectContextMenu.effectIndex >= 0 && sidebarList.model && effectContextMenu.effectIndex < sidebarList.model.length

                    Instantiator {
                        model: {
                            var em = sidebarList.model[effectContextMenu.effectIndex];
                            return em ? PresetManager.presetNames(em.id) : [];
                        }
                        MenuItem {
                            text: modelData
                            onTriggered: {
                                var em = sidebarList.model[effectContextMenu.effectIndex];
                                if (!em)
                                    return;

                                var preset = PresetManager.loadPreset(em.id, modelData);
                                if (!preset || !preset.params)
                                    return;

                                var params = preset.params;
                                var keys = Object.keys(params);
                                for (var i = 0; i < keys.length; i++) {
                                    var key = keys[i];
                                    if (key === "version" || key === "effectId" || key === "name" || key === "enabled")
                                        continue;

                                    Workspace.currentTimeline.updateClipEffectParam(targetClipId, effectContextMenu.effectIndex, key, params[key]);
                                }
                                if (preset.keyframes) {
                                    var tracks = preset.keyframes;
                                    var trackKeys = Object.keys(tracks);
                                    for (var ti = 0; ti < trackKeys.length; ti++) {
                                        var paramName = trackKeys[ti];
                                        var track = tracks[paramName];
                                        if (!track)
                                            continue;

                                        if (track.start) {
                                            var opts = {};
                                            if (track.start.interp !== undefined)
                                                opts.interp = track.start.interp;
                                            Workspace.currentTimeline.setKeyframe(targetClipId, effectContextMenu.effectIndex, paramName, track.start.frame || 0, track.start.value, opts);
                                        }
                                        var points = track.points || [];
                                        for (var pi = 0; pi < points.length; pi++) {
                                            var pt = points[pi];
                                            var ptOpts = {};
                                            if (pt.interp !== undefined)
                                                ptOpts.interp = pt.interp;
                                            if (pt.points !== undefined)
                                                ptOpts.points = pt.points;
                                            if (pt.modeParams !== undefined)
                                                ptOpts.modeParams = pt.modeParams;
                                            Workspace.currentTimeline.setKeyframe(targetClipId, effectContextMenu.effectIndex, paramName, pt.frame, pt.value, ptOpts);
                                        }
                                    }
                                }
                                if (preset.enabled !== undefined)
                                    Workspace.currentTimeline.setEffectEnabled(targetClipId, effectContextMenu.effectIndex, preset.enabled);
                            }
                        }
                        onObjectAdded: presetLoadMenu.insertItem(index, object)
                        onObjectRemoved: presetLoadMenu.removeItem(object)
                    }

                    MenuItem {
                        text: qsTr("プリセットがありません")
                        enabled: false
                        visible: presetLoadMenu.count === 0
                    }
                }

                Menu {
                    id: presetDeleteMenu
                    title: qsTr("プリセットを削除")
                    enabled: effectContextMenu.effectIndex >= 0 && sidebarList.model && effectContextMenu.effectIndex < sidebarList.model.length

                    Instantiator {
                        model: {
                            var em = sidebarList.model[effectContextMenu.effectIndex];
                            return em ? PresetManager.presetNames(em.id) : [];
                        }
                        MenuItem {
                            text: modelData
                            onTriggered: {
                                var em = sidebarList.model[effectContextMenu.effectIndex];
                                if (!em)
                                    return;

                                PresetManager.deletePreset(em.id, modelData);
                            }
                        }
                        onObjectAdded: presetDeleteMenu.insertItem(index, object)
                        onObjectRemoved: presetDeleteMenu.removeItem(object)
                    }

                    MenuItem {
                        text: qsTr("プリセットがありません")
                        enabled: false
                        visible: presetDeleteMenu.count === 0
                    }
                }

            }

        }

        Dialog {
            id: presetSaveDialog

            property string effectId: ""
            property int effectIndex: -1

            title: qsTr("プリセットを保存")
            modal: true
            anchors.centerIn: parent
            standardButtons: Dialog.Ok | Dialog.Cancel

            ColumnLayout {
                spacing: 8

                Label {
                    text: qsTr("プリセット名:")
                }

                TextField {
                    id: presetNameField
                    Layout.preferredWidth: 250
                    placeholderText: qsTr("プリセット名を入力...")
                    selectByMouse: true
                }
            }

            onOpened: presetNameField.text = ""
            onAccepted: {
                if (presetNameField.text.length === 0)
                    return;

                var em = sidebarList.model[effectIndex];
                if (!em)
                    return;

                PresetManager.savePreset(effectId, presetNameField.text, em.params, em.keyframeTracks, em.enabled);
            }
        }

        // 詳細設定スクロールビュー
        ScrollView {
            id: mainScrollView

            SplitView.fillWidth: true
            contentWidth: availableWidth
            clip: true

            ColumnLayout {
                width: parent.width
                spacing: 1
                layoutDirection: Qt.LeftToRight

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    visible: root.isAudioWorkspaceClip

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.margins: 10
                        Layout.preferredHeight: 82
                        radius: 8
                        color: palette.midlight
                        border.width: 1
                        border.color: palette.mid

                        RowLayout {
                            anchors.fill: parent
                            anchors.margins: 12
                            spacing: 14

                            Rectangle {
                                Layout.preferredWidth: 44
                                Layout.preferredHeight: 44
                                radius: 10
                                color: palette.highlight

                                Common.AviQtlIcon {
                                    anchors.centerIn: parent
                                    iconName: "sound_module_line"
                                    size: 26
                                    color: palette.highlightedText
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2

                                Label {
                                    text: qsTr("音声ワークスペース")
                                    color: palette.text
                                    font.pixelSize: 18
                                    font.bold: true
                                }

                                Label {
                                    text: qsTr("独立した音声ソース、Carlaプラグインチェーン、メーターをこのオブジェクト内で管理します。")
                                    color: palette.text
                                    opacity: 0.72
                                    wrapMode: Text.WordWrap
                                    Layout.fillWidth: true
                                }
                            }

                            Button {
                                text: qsTr("プラグイン追加")
                                onClicked: filterMenu.popup()
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.margins: 10
                        Layout.preferredHeight: 130
                        radius: 8
                        color: palette.base
                        border.width: 1
                        border.color: palette.mid

                        Canvas {
                            id: audioWaveformCanvas

                            anchors.fill: parent
                            anchors.margins: 10
                            opacity: 0.9
                            onWidthChanged: requestPaint()
                            onPaint: {
                                var ctx = getContext("2d");
                                ctx.clearRect(0, 0, width, height);
                                ctx.fillStyle = palette.base;
                                ctx.fillRect(0, 0, width, height);

                                if (!Workspace.currentTimeline || targetClipId < 0 || width <= 0 || height <= 0)
                                    return;

                                var pw = Math.max(1, Math.floor(width));
                                var durationFrames = Math.max(1, Workspace.currentTimeline.clipDurationFrames || 1);
                                var peaks = Workspace.currentTimeline.getWaveformPeaks(targetClipId, pw, durationFrames);
                                var cy = height / 2;
                                ctx.strokeStyle = palette.highlight;
                                ctx.lineWidth = 1;
                                if (!peaks || peaks.length === 0) {
                                    ctx.globalAlpha = 0.35;
                                    ctx.beginPath();
                                    ctx.moveTo(0, cy);
                                    ctx.lineTo(width, cy);
                                    ctx.stroke();
                                    ctx.globalAlpha = 1;
                                    return;
                                }

                                var maxH = Math.max(1, cy - 4);
                                for (var i = 0; i < peaks.length / 2; i++) {
                                    var pMin = peaks[i * 2];
                                    var pMax = peaks[i * 2 + 1];
                                    var yTop = cy - (pMax * maxH);
                                    var yBottom = cy - (pMin * maxH);
                                    if (yBottom - yTop < 1)
                                        yBottom = yTop + 1;
                                    ctx.beginPath();
                                    ctx.moveTo(i + 0.5, yTop);
                                    ctx.lineTo(i + 0.5, yBottom);
                                    ctx.stroke();
                                }
                            }

                            Connections {
                                function onClipEffectsChanged(clipId) {
                                    if (clipId === targetClipId)
                                        audioWaveformCanvas.requestPaint();
                                }

                                function onClipDurationFramesChanged() {
                                    audioWaveformCanvas.requestPaint();
                                }

                                target: Workspace.currentTimeline
                            }
                        }
                    }

                    FileDialog {
                        id: audioSourceDialog

                        title: qsTr("音声ソースを選択")
                        nameFilters: ["Audio Files (*.wav *.mp3 *.aac *.m4a *.flac *.ogg)", "All Files (*)"]
                        onAccepted: {
                            var path = selectedFile.toString();
                            if (path.indexOf("file://") === 0) {
                                // file:///Users/foo → /Users/foo (Unix)
                                // file:///C:/foo → C:/foo (Windows)
                                var local = path.replace(/^file:\/\//, "");
                                if (Qt.platform.os === "windows")
                                    local = local.replace(/^\//, "");
                                path = decodeURIComponent(local);
                            }
                            root.setAudioParam("source", path);
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.margins: 10
                        spacing: 10

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 260
                            radius: 8
                            color: palette.midlight
                            border.width: 1
                            border.color: palette.mid

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 10

                                Label {
                                    text: qsTr("Source Deck")
                                    color: palette.text
                                    font.pixelSize: 15
                                    font.bold: true
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    TextField {
                                        Layout.fillWidth: true
                                        text: root.audioParamValue("source", "")
                                        placeholderText: qsTr("音声ファイルを選択...")
                                        selectByMouse: true
                                        onEditingFinished: root.setAudioParam("source", text)
                                    }

                                    Button {
                                        text: qsTr("参照")
                                        onClicked: audioSourceDialog.open()
                                    }
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Label {
                                        text: qsTr("Mode")
                                        color: palette.text
                                        Layout.preferredWidth: 72
                                    }

                                    ComboBox {
                                        Layout.fillWidth: true
                                        model: ["開始時間＋再生速度", "時間直接指定"]
                                        currentIndex: model.indexOf(root.audioParamValue("playMode", "開始時間＋再生速度"))
                                        onActivated: root.setAudioParam("playMode", model[currentIndex])
                                    }
                                }

                                GridLayout {
                                    Layout.fillWidth: true
                                    columns: 2
                                    columnSpacing: 10
                                    rowSpacing: 8

                                    Label {
                                        text: qsTr("Start")
                                        color: palette.text
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true
                                        Slider {
                                            Layout.fillWidth: true
                                            from: 0
                                            to: 60
                                            value: Number(root.audioParamValue("startTime", 0))
                                            onMoved: root.setAudioParam("startTime", value)
                                        }
                                        TextField {
                                            Layout.preferredWidth: 64
                                            text: Number(root.audioParamValue("startTime", 0)).toFixed(2)
                                            horizontalAlignment: Text.AlignRight
                                            onEditingFinished: root.setAudioParam("startTime", Number(text))
                                        }
                                    }

                                    Common.AudioKfParamTrack {
                                        paramName: "speed"; defaultValue: 100; sliderFrom: 1; sliderTo: 400; decimals: 0; buttonLabel: "Speed"
                                    }

                                    Common.AudioKfParamTrack {
                                        paramName: "directTime"; defaultValue: 0; sliderFrom: 0; sliderTo: 60; decimals: 2; buttonLabel: "Direct"
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.preferredWidth: 260
                            Layout.preferredHeight: 260
                            radius: 8
                            color: palette.midlight
                            border.width: 1
                            border.color: palette.mid

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 8

                                RowLayout {
                                    Layout.fillWidth: true

                                    Label {
                                        text: qsTr("Channel Strip")
                                        color: palette.text
                                        font.pixelSize: 15
                                        font.bold: true
                                        Layout.fillWidth: true
                                    }

                                    CheckBox {
                                        text: qsTr("Limit")
                                        checked: Boolean(root.audioParamValue("limiter", true))
                                        onToggled: root.setAudioParam("limiter", checked)
                                    }
                                }

                                GridLayout {
                                    Layout.fillWidth: true
                                    columns: 2
                                    rowSpacing: 6
                                    columnSpacing: 8

                                    Common.AudioKfParamTrack {
                                        paramName: "volume"
                                        defaultValue: 1
                                        sliderFrom: 0
                                        sliderTo: 2
                                        decimals: 2
                                        buttonLabel: "VOL"
                                    }

                                    Common.AudioKfParamTrack {
                                        paramName: "masterVolume"
                                        defaultValue: 1
                                        sliderFrom: 0
                                        sliderTo: 2
                                        decimals: 2
                                        buttonLabel: "MASTER"
                                    }

                                    Common.AudioKfParamTrack {
                                        paramName: "pan"
                                        defaultValue: 0
                                        sliderFrom: -1
                                        sliderTo: 1
                                        decimals: 2
                                        buttonLabel: "PAN"
                                    }

                                    Common.AudioKfParamTrack {
                                        paramName: "fadeIn"
                                        defaultValue: 0
                                        sliderFrom: 0
                                        sliderTo: 10
                                        decimals: 2
                                        buttonLabel: "FADE IN"
                                    }

                                    Common.AudioKfParamTrack {
                                        paramName: "fadeOut"
                                        defaultValue: 0
                                        sliderFrom: 0
                                        sliderTo: 10
                                        decimals: 2
                                        buttonLabel: "FADE OUT"
                                    }
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Button {
                                        Layout.fillWidth: true
                                        text: qsTr("MUTE")
                                        checkable: true
                                        checked: Boolean(root.audioParamValue("mute", false))
                                        onToggled: root.setAudioParam("mute", checked)
                                    }

                                    Button {
                                        Layout.fillWidth: true
                                        text: qsTr("SOLO")
                                        checkable: true
                                        checked: Boolean(root.audioParamValue("solo", false))
                                        onToggled: root.setAudioParam("solo", checked)
                                    }
                                }
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.margins: 10
                        Layout.preferredHeight: 94
                        radius: 8
                        color: palette.midlight
                        border.width: 1
                        border.color: palette.mid

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 12
                            spacing: 8

                            Label {
                                text: qsTr("メーター")
                                color: palette.text
                                font.bold: true
                            }

                            Repeater {
                                model: [
                                    { "name": "L", "peak": root.audioPeakLeft, "rms": root.audioRmsLeft },
                                    { "name": "R", "peak": root.audioPeakRight, "rms": root.audioRmsRight }
                                ]

                                delegate: RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    property real peakDb: modelData.peak > 0.0001 ? 20 * Math.log10(modelData.peak) : -100

                                    Label {
                                        text: modelData.name
                                        Layout.preferredWidth: 14
                                        color: palette.text
                                    }

                                    Rectangle {
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 12
                                        radius: 6
                                        color: palette.base
                                        border.width: 1
                                        border.color: palette.mid

                                        Rectangle {
                                            anchors.left: parent.left
                                            anchors.verticalCenter: parent.verticalCenter
                                            height: parent.height - 2
                                            width: Math.min(parent.width, parent.width * Math.max(0, Math.min(1, modelData.rms)))
                                            radius: 5
                                            color: palette.highlight
                                            opacity: 0.55
                                        }

                                        Rectangle {
                                            anchors.left: parent.left
                                            anchors.verticalCenter: parent.verticalCenter
                                            height: parent.height - 2
                                            width: Math.min(parent.width, parent.width * Math.max(0, Math.min(1, modelData.peak)))
                                            radius: 5
                                            color: modelData.peak > 0.95 ? "#d94f4f" : "#4fd97b"
                                        }
                                    }

                                    Label {
                                        text: {
                                            if (modelData.peak > 0.95) return "CLIP";
                                            var db = parent.peakDb;
                                            return db > -100 ? db.toFixed(1) + "dB" : "-inf";
                                        }
                                        Layout.preferredWidth: 48
                                        horizontalAlignment: Text.AlignRight
                                        color: modelData.peak > 0.95 ? "#d94f4f" : palette.text
                                        font.pixelSize: 11
                                        font.bold: modelData.peak > 0.95
                                        opacity: 0.85
                                    }
                                }
                            }
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.leftMargin: 10
                        Layout.rightMargin: 10
                        text: qsTr("Carlaプラグインチェーン")
                        color: palette.text
                        font.pixelSize: 15
                        font.bold: true
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.leftMargin: 10
                        Layout.rightMargin: 10
                        text: audioEffectsModel.length > 0 ? qsTr("左側のリストでプラグインを選択・並べ替えできます。") : qsTr("上部の「プラグイン追加」またはメニューからCarlaプラグインを追加してください。")
                        color: palette.text
                        opacity: 0.7
                        wrapMode: Text.WordWrap
                    }
                }

                Repeater {
                    id: videoEffectsRepeater

                    model: root.isAudioWorkspaceClip ? [] : effectsModel

                    delegate: ColumnLayout {
                        id: effectRoot

                        property int effectIndex: {
                            if (!Workspace.currentTimeline)
                                return index;

                            var resolvedIndex = Workspace.currentTimeline.getClipEffectIndex(targetClipId, modelData);
                            return resolvedIndex >= 0 ? resolvedIndex : index;
                        }
                        property var effectModel: modelData
                        property int _effectRev: 0

                        width: root.width
                        spacing: 0

                        Connections {
                            function onParamsChanged() {
                                effectRoot._effectRev++;
                            }

                            function onKeyframeTracksChanged() {
                                effectRoot._effectRev++;
                            }

                            target: effectRoot.effectModel
                            ignoreUnknownSignals: true
                        }

                        // エフェクトヘッダー
                        Rectangle {
                            Layout.fillWidth: true
                            height: 24
                            color: palette.midlight

                            Label {
                                text: modelData.name
                                color: palette.text
                                font.bold: true
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.left: parent.left
                                anchors.leftMargin: 10
                            }

                            Button {
                                visible: modelData.kind === "effect" && !(effectRoot.effectIndex === 0 && modelData.id === "transform")
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                flat: true
                                hoverEnabled: true
                                width: 24
                                height: 24
                                onClicked: Workspace.currentTimeline.removeEffect(targetClipId, effectRoot.effectIndex)

                                contentItem: Common.AviQtlIcon {
                                    iconName: "close_line"
                                    size: 16
                                    color: parent.hovered ? "red" : parent.palette.text
                                }

                            }

                        }

                        // 全パラメータ（統一処理）
                        Repeater {
                            // 終了点は EasingConfigWindow が isEnd=true で生成する

                            model: getUiModel(effectModel)

                            delegate: ColumnLayout {
                                id: paramDelegate

                                property int activeDragOriginal: -1
                                property int activeDragCurrent: -1
                                property var def: modelData
                                property string key: (def && (def.param || def.name)) || ""
                                property var effVal: {
                                    var _ = effectRoot._effectRev;
                                    if (!effectModel)
                                        return undefined;

                                    var v = effectModel.evaluatedParam(key, curRelFrame, root._projectFps);
                                    if (v !== undefined && v !== null)
                                        return v;

                                    if (effectModel.params)
                                        return effectModel.params[key];

                                    return undefined;
                                }
                                property bool isNumber: typeof effVal === "number" && (!def.type || ["float", "number", "slider", "spinner", "int", "integer"].indexOf(def.type) !== -1)
                                property bool isColor: !!def && (def.type === "color" || def.type === "colour")
                                property bool supportsRangeUi: isNumber || isColor
                                property var effectModel: effectRoot.effectModel
                                property int effIdx: effectRoot.effectIndex
                                // キーフレーム
                                property int curRelFrame: (Workspace.currentTimeline && Workspace.currentTimeline.transport) ? Math.max(0, Workspace.currentTimeline.transport.currentFrame - Workspace.currentTimeline.clipStartFrame) : 0
                                property int clipDur: Workspace.currentTimeline ? Workspace.currentTimeline.clipDurationFrames : 100
                                property var tracks: effectModel ? effectModel.keyframeTracks : null
                                property var rawKfs: {
                                    var _ = tracks;
                                    return effectModel ? effectModel.keyframeListForUi(key) : [];
                                }
                                property var kfs: keyframesWithVirtualEnd(rawKfs, clipDur)
                                property bool hasKeyframes: kfs.length > 0
                                property var interval: findInterval(kfs, curRelFrame, clipDur)
                                property int startFrame: interval.start
                                property int endFrame: interval.end
                                property var startVal: {
                                    var _t = tracks;
                                    var _r = effectRoot._effectRev;
                                    return effectModel ? effectModel.evaluatedParam(key, startFrame, root._projectFps) : effVal;
                                }
                                property var endVal: {
                                    var _t = tracks;
                                    var _r = effectRoot._effectRev;
                                    return effectModel ? effectModel.evaluatedParam(key, endFrame, root._projectFps) : effVal;
                                }
                                property string interpType: {
                                    var _ = tracks;
                                    return hasKeyframes ? getInterpAt(startFrame) : "constant";
                                }
                                property bool isMoving: supportsRangeUi && (hasKeyframes || interpType !== "constant")

                                function hasKeyframeAt(f) {
                                    if (!kfs)
                                        return false;

                                    for (var i = 0; i < kfs.length; i++) {
                                        if (kfs[i].frame === f)
                                            return true;

                                    }
                                    return false;
                                }

                                function hasRealKeyframeAt(f) {
                                    if (!rawKfs)
                                        return false;

                                    for (var i = 0; i < rawKfs.length; i++) {
                                        if (rawKfs[i].frame === f)
                                            return true;

                                    }
                                    return false;
                                }

                                function keyframesWithVirtualEnd(points, totalDur) {
                                    var out = [];
                                    if (points) {
                                        for (var i = 0; i < points.length; i++) out.push(points[i])
                                    }
                                    if (totalDur > 0) {
                                        var hasEnd = false;
                                        for (var j = 0; j < out.length; j++) {
                                            if (out[j].frame === totalDur) {
                                                hasEnd = true;
                                                break;
                                            }
                                        }
                                        if (!hasEnd) {
                                            var endValue = effectModel ? effectModel.evaluatedParam(key, totalDur, root._projectFps) : effVal;
                                            out.push({
                                                "frame": totalDur,
                                                "value": endValue,
                                                "interp": "none",
                                                "virtualEnd": true
                                            });
                                        }
                                    }
                                    out.sort(function(a, b) {
                                        return a.frame - b.frame;
                                    });
                                    return out;
                                }

                                function seekTrackFrameAt(xPos) {
                                    if (!Workspace.currentTimeline || !Workspace.currentTimeline.transport || clipDur <= 0 || trackItem.width <= 0)
                                        return ;

                                    var rawRelFrame = (xPos / trackItem.width) * clipDur;
                                    var relFrame = Math.max(0, Math.min(clipDur, Math.round(rawRelFrame)));
                                    Workspace.currentTimeline.transport.setCurrentFrame_seek(Workspace.currentTimeline.clipStartFrame + relFrame);
                                }

                                function ensureKeyframeAt(f) {
                                    if (!effectModel || !key)
                                        return ;

                                    if (hasRealKeyframeAt(f))
                                        return ;

                                    var raw = effectModel.evaluatedParam(key, f, root._projectFps);
                                    var v = (raw !== undefined && raw !== null) ? raw : effVal;
                                    Workspace.currentTimeline.setKeyframe(targetClipId, effIdx, paramDelegate.key, f, v, interpolationOptionsAt(f));
                                }

                                function ensureRangeKeyframes() {
                                    ensureKeyframeAt(startFrame);
                                    if (endFrame !== clipDur)
                                        ensureKeyframeAt(endFrame);

                                }

                                function findInterval(kfs, cur, totalDur) {
                                    let s = 0, e = totalDur;
                                    if (!kfs || kfs.length === 0)
                                        return {
                                        "start": s,
                                        "end": e
                                    };

                                    if (cur >= totalDur) {
                                        e = totalDur;
                                        for (let i = kfs.length - 1; i >= 0; i--) {
                                            if (kfs[i].frame < totalDur) {
                                                s = kfs[i].frame;
                                                break;
                                            }
                                        }
                                        return {
                                            "start": s,
                                            "end": e
                                        };
                                    }
                                    let foundStart = false;
                                    for (let i = kfs.length - 1; i >= 0; i--) {
                                        if (kfs[i].frame <= cur) {
                                            s = kfs[i].frame;
                                            foundStart = true;
                                            if (i + 1 < kfs.length)
                                                e = kfs[i + 1].frame;
                                            else
                                                e = totalDur;
                                            break;
                                        }
                                    }
                                    if (!foundStart) {
                                        e = kfs[0].frame;
                                        s = 0;
                                    }
                                    return {
                                        "start": s,
                                        "end": e
                                    };
                                }

                                function getInterpAt(f) {
                                    if (!kfs)
                                        return "linear";

                                    for (var i = 0; i < kfs.length; i++) {
                                        if (kfs[i].frame === f)
                                            return kfs[i].interp || "linear";

                                    }
                                    return "linear";
                                }

                                function interpolationOptionsAt(f) {
                                    var options = {
                                        "interp": "none"
                                    };
                                    if (!kfs)
                                        return options;

                                    var source = null;
                                    for (var i = kfs.length - 1; i >= 0; i--) {
                                        if (kfs[i].frame <= f) {
                                            source = kfs[i];
                                            break;
                                        }
                                    }
                                    if (!source && kfs.length > 0)
                                        source = kfs[0];

                                    if (!source)
                                        return options;

                                    options.interp = source.interp || "none";
                                    if (source.points)
                                        options.points = source.points;

                                    if (source.modeParams)
                                        options.modeParams = source.modeParams;

                                    return options;
                                }

                                function getGridLines() {
                                    if (!Workspace.currentTimeline || !enableSnap)
                                        return [];

                                    let step = getGridInterval();
                                    if (step <= 0)
                                        return [];

                                    let gs = gridSettings();
                                    let fps = (Workspace.currentTimeline.project && Workspace.currentTimeline.project.fps) ? Workspace.currentTimeline.project.fps : 60;
                                    let offsetF = (gs.mode === "BPM") ? gs.offset * fps : 0;
                                    let lines = [];
                                    let startAbs = Workspace.currentTimeline.clipStartFrame;
                                    let endAbs = startAbs + clipDur;
                                    let firstLine = Math.ceil((startAbs - offsetF) / step) * step + offsetF;
                                    if (clipDur / step > 150)
                                        return [];

                                    for (let f = firstLine; f <= endAbs; f += step) {
                                        let rel = f - startAbs;
                                        if (rel >= 0 && rel <= clipDur)
                                            lines.push(rel);

                                    }
                                    return lines;
                                }

                                function updateParam(frame, val) {
                                    if (!effectModel || !key)
                                        return ;

                                    if (!hasKeyframes) {
                                        Workspace.currentTimeline.updateClipEffectParam(targetClipId, effIdx, key, val);
                                        var source = String(effectModel.params["source"] || "").toLowerCase();
                                        var sourceIsVideo = /\.(mp4|mov|avi|mkv|webm|wmv)$/.test(source);
                                        if (key === "linkedVideo" && val === true && sourceIsVideo)
                                            Workspace.currentTimeline.updateClipEffectParam(targetClipId, effIdx, "speed", 100.0);

                                        return ;
                                    }
                                    let type = "linear";
                                    if (frame === startFrame)
                                        type = getInterpAt(startFrame);
                                    else
                                        type = getInterpAt(frame);
                                    if (type === "constant")
                                        type = "linear";

                                    Workspace.currentTimeline.setKeyframe(targetClipId, effIdx, paramDelegate.key, frame, val, {
                                        "interp": type
                                    });
                                }

                                Layout.fillWidth: true
                                spacing: 0

                                readonly property bool isControlDisabled: {
                                    var _ = effectRoot._effectRev;
                                    if (!def || !effectModel)
                                        return false;
                                    if (def.disabledByVideoLink === true) {
                                        var source = String(effectModel.params["source"] || "").toLowerCase();
                                        var sourceIsVideo = /\.(mp4|mov|avi|mkv|webm|wmv)$/.test(source);
                                        return sourceIsVideo && effectModel.params["linkedVideo"] === true;
                                    }
                                    if (def.disabledBy) {
                                        var refParam = effectModel.params[def.disabledBy];
                                        return refParam === true;
                                    }
                                    return false;
                                }

                                // 数値
                                Common.ParamControl {
                                    Layout.fillWidth: true
                                    Layout.margins: 4
                                    visible: isNumber
                                    enabled: isNumber && !paramDelegate.isControlDisabled
                                    opacity: paramDelegate.isControlDisabled ? 0.45 : 1
                                    isRangeMode: isMoving && hasKeyframeAt(endFrame)
                                    interpolationType: interpType
                                    paramName: {
                                        var interpLabel = {
                                            "linear": qsTr(" (直線)"),
                                            "ease_in": qsTr(" (加速)"),
                                            "ease_out": qsTr(" (減速)"),
                                            "ease_in_out": qsTr(" (加減速)"),
                                            "bezier": qsTr(" (ベジェ)")
                                        };
                                        var name = (def.label && def.label !== "") ? def.label : key;
                                        return name + (isMoving ? (interpLabel[interpType] || "") : "");
                                    }
                                    startValue: Number(startVal) || 0
                                    endValue: Number(endVal) || 0
                                    minValue: (def.min !== undefined) ? def.min : ((key === "scale" || key === "opacity") ? 0 : -1000)
                                    maxValue: (def.max !== undefined) ? def.max : ((key === "scale") ? 500 : (key === "opacity" ? 1 : 1000))
                                    decimals: (def.decimals !== undefined) ? def.decimals : 2
                                    onStartValueModified: function(val) {
                                        root.inputting = true;
                                        updateParam(startFrame, val);
                                        var _rightActive = isMoving && hasKeyframeAt(endFrame) && interpType !== "" && interpType !== "constant";
                                        if (!_rightActive && endFrame !== startFrame && endFrame !== clipDur) {
                                            ensureKeyframeAt(endFrame);
                                            updateParam(endFrame, val);
                                        }
                                        root.inputting = false;
                                    }
                                    onEndValueModified: function(val) {
                                        root.inputting = true;
                                        updateParam(endFrame, val);
                                        root.inputting = false;
                                    }
                                    onParamButtonClicked: {
                                        if (!effectModel || !key)
                                            return ;

                                        // 区間キーフレームがない場合は生成
                                        ensureRangeKeyframes();
                                        var win = WindowManager.getWindow("easingConfig");
                                        if (win)
                                            win.openConfig({
                                            "clipId": targetClipId,
                                            "effectIndex": effIdx,
                                            "effectModel": effectModel,
                                            "paramName": key,
                                            "keyframeFrame": startFrame
                                        });

                                    }
                                }

                                // 非数値 (ControlLoader で型別UI)
                                Common.ControlLoader {
                                    property int startFrameState: startFrame
                                    property int endFrameState: endFrame
                                    property bool rightInteractiveState: isMoving && hasKeyframeAt(endFrame) && interpType !== "" && interpType !== "constant"

                                    Layout.fillWidth: true
                                    Layout.margins: 4
                                    visible: !isNumber
                                    enabled: !paramDelegate.isControlDisabled
                                    definition: def
                                    value: effVal
                                    effectRootRef: effectRoot
                                    onStartValueModified: function(val) {
                                        root.inputting = true;
                                        updateParam(startFrame, val);
                                        if (!rightInteractiveState && endFrame !== startFrame && endFrame !== clipDur) {
                                            ensureKeyframeAt(endFrame);
                                            updateParam(endFrame, val);
                                        }
                                        root.inputting = false;
                                    }
                                    onEndValueModified: function(val) {
                                        root.inputting = true;
                                        updateParam(endFrame, val);
                                        root.inputting = false;
                                    }
                                    onValueModified: function(val) {
                                        root.inputting = true;
                                        updateParam(startFrame, val);
                                        root.inputting = false;
                                    }
                                    onParamButtonClicked: {
                                        if (!effectModel || !key)
                                            return ;

                                        ensureRangeKeyframes();
                                        var win = WindowManager.getWindow("easingConfig");
                                        if (win)
                                            win.openConfig({
                                            "clipId": targetClipId,
                                            "effectIndex": effIdx,
                                            "effectModel": effectModel,
                                            "paramName": key,
                                            "keyframeFrame": startFrame
                                        });

                                    }
                                }

                                // ミニタイムラインバー
                                Item {
                                    id: trackItem

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 12
                                    Layout.leftMargin: 4
                                    Layout.rightMargin: 4
                                    visible: supportsRangeUi

                                    Rectangle {
                                        anchors.centerIn: parent
                                        width: parent.width
                                        height: 2
                                        color: palette.mid

                                        Rectangle {
                                            property int vStart: (paramDelegate && paramDelegate.activeDragOriginal === startFrame) ? paramDelegate.activeDragCurrent : startFrame
                                            property int vEnd: (paramDelegate && paramDelegate.activeDragOriginal === endFrame) ? paramDelegate.activeDragCurrent : endFrame

                                            height: 4
                                            anchors.verticalCenter: parent.verticalCenter
                                            color: palette.highlight
                                            opacity: 0.7
                                            x: (Math.min(vStart, vEnd) / clipDur) * parent.width
                                            width: Math.max(0, (Math.abs(vEnd - vStart) / clipDur) * parent.width)
                                            visible: clipDur > 0
                                        }

                                    }

                                    Repeater {
                                        model: enableSnap ? getGridLines() : []

                                        Rectangle {
                                            width: 1
                                            height: 8
                                            color: palette.midlight
                                            opacity: 0.6
                                            anchors.verticalCenter: parent.verticalCenter
                                            x: (modelData / clipDur) * trackItem.width
                                        }

                                    }

                                    Repeater {
                                        model: kfs

                                        Item {
                                            id: kfItem

                                            property int originalFrame: modelData.frame
                                            property int currentFrame: originalFrame
                                            // Capture outer scope variables here, where visual tree resolution works perfectly
                                            property var targetModel: effectModel
                                            property string targetKey: key
                                            property var rootWindow: root
                                            property int minDragFrame: 0
                                            property int maxDragFrame: clipDur
                                            property bool isEndpoint: originalFrame === 0 || !!modelData.virtualEnd

                                            width: 16
                                            height: 16
                                            anchors.verticalCenter: parent.verticalCenter
                                            x: Math.min(trackItem.width - width / 2, (currentFrame / clipDur) * trackItem.width - width / 2)

                                            Rectangle {
                                                width: 8
                                                height: 8
                                                color: kfMa.containsMouse || pointDrag.active ? palette.highlight : palette.text
                                                anchors.centerIn: parent
                                                rotation: 45
                                                antialiasing: true
                                            }

                                            MouseArea {
                                                id: kfMa

                                                anchors.fill: parent
                                                hoverEnabled: true
                                                cursorShape: kfItem.isEndpoint ? Qt.ArrowCursor : (pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor)
                                                acceptedButtons: Qt.LeftButton | Qt.RightButton
                                                onClicked: function(mouse) {
                                                    if (mouse.button === Qt.LeftButton)
                                                        paramDelegate.seekTrackFrameAt(kfItem.currentFrame / clipDur * trackItem.width);
                                                    else if (mouse.button === Qt.RightButton && !kfItem.isEndpoint)
                                                        Workspace.currentTimeline.removeKeyframe(targetClipId, effIdx, kfItem.targetKey, kfItem.originalFrame);
                                                }
                                                onDoubleClicked: function(mouse) {
                                                    mouse.accepted = true;
                                                }
                                            }

                                            DragHandler {
                                                id: pointDrag

                                                property real startX: 0

                                                target: null
                                                enabled: !kfItem.isEndpoint
                                                acceptedButtons: Qt.LeftButton
                                                onActiveChanged: {
                                                    if (active) {
                                                        startX = kfItem.x;
                                                        kfItem.rootWindow.inputting = true;
                                                        let minF = 0;
                                                        let maxF = clipDur;
                                                        for (let i = 0; i < kfs.length; i++) {
                                                            let f = kfs[i].frame;
                                                            if (f < kfItem.originalFrame && f >= minF)
                                                                minF = f + 1;

                                                            if (f > kfItem.originalFrame && f <= maxF)
                                                                maxF = f - 1;

                                                        }
                                                        kfItem.minDragFrame = minF;
                                                        kfItem.maxDragFrame = maxF;
                                                        if (typeof paramDelegate !== "undefined") {
                                                            paramDelegate.activeDragOriginal = kfItem.originalFrame;
                                                            paramDelegate.activeDragCurrent = kfItem.originalFrame;
                                                        }
                                                    } else {
                                                        if (typeof paramDelegate !== "undefined")
                                                            paramDelegate.activeDragOriginal = -1;

                                                        if (kfItem.currentFrame !== kfItem.originalFrame)
                                                            Workspace.currentTimeline.moveKeyframe(targetClipId, effIdx, kfItem.targetKey, kfItem.originalFrame, kfItem.currentFrame);

                                                        kfItem.rootWindow.inputting = false;
                                                    }
                                                }
                                                onTranslationChanged: {
                                                    if (active) {
                                                        let newX = startX + translation.x;
                                                        let rawRelFrame = ((newX + kfItem.width / 2) / trackItem.width) * clipDur;
                                                        let snappedFrame = snapRelativeFrame(rawRelFrame);
                                                        snappedFrame = Math.max(kfItem.minDragFrame, Math.min(kfItem.maxDragFrame, snappedFrame));
                                                        kfItem.currentFrame = snappedFrame;
                                                        if (typeof paramDelegate !== "undefined")
                                                            paramDelegate.activeDragCurrent = snappedFrame;

                                                    }
                                                }
                                            }

                                        }

                                    }

                                    Rectangle {
                                        width: 1
                                        height: parent.height
                                        color: palette.highlight
                                        x: (curRelFrame / clipDur) * parent.width
                                        visible: clipDur > 0
                                    }

                                    MouseArea {
                                        anchors.fill: parent
                                        acceptedButtons: Qt.LeftButton
                                        onClicked: function(mouse) {
                                            seekTrackFrameAt(mouse.x);
                                        }
                                        onDoubleClicked: function(mouse) {
                                            let rawRelFrame = (mouse.x / trackItem.width) * clipDur;
                                            let f = snapRelativeFrame(rawRelFrame);
                                            f = Math.max(0, Math.min(clipDur, f));
                                            if (hasKeyframeAt(f))
                                                return ;

                                            let val = effectModel.evaluatedParam(key, f, root._projectFps);
                                            let options = interpolationOptionsAt(f);
                                            Workspace.currentTimeline.setKeyframe(targetClipId, effIdx, key, f, val, options);
                                        }
                                    }

                                }

                            }

                        }

                    }

                }

                // オーディオプラグインのパラメータ表示
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1
                    visible: Workspace.currentTimeline && Workspace.currentTimeline.isAudioClip(targetClipId)

                    Repeater {
                        id: audioEffectsRepeater

                        model: audioEffectsModel

                        delegate: ColumnLayout {
                            id: audioEffectRoot

                            property int effectIndex: index

                            Layout.fillWidth: true
                            spacing: 0

                            Rectangle {
                                Layout.fillWidth: true
                                height: 24
                                color: palette.midlight

                                Label {
                                    text: modelData.name + " (" + modelData.format + ")"
                                    color: palette.text
                                    font.bold: true
                                    anchors.verticalCenter: parent.verticalCenter
                                    anchors.left: parent.left
                                    anchors.leftMargin: 10
                                }

                                Button {
                                    anchors.right: parent.right
                                    anchors.verticalCenter: parent.verticalCenter
                                    flat: true
                                    hoverEnabled: true
                                    width: 24
                                    height: 24
                                    onClicked: Workspace.currentTimeline.removeAudioPlugin(targetClipId, audioEffectRoot.effectIndex)

                                    contentItem: Common.AviQtlIcon {
                                        iconName: "close_line"
                                        size: 16
                                        color: parent.hovered ? "red" : parent.palette.text
                                    }

                                }

                            }

                            Repeater {
                                id: audioPluginParamsRepeater
                                model: Workspace.currentTimeline.getEffectParameters(targetClipId, index)

                                delegate: ColumnLayout {
                                    id: audioPluginParamDelegate
                                    Layout.fillWidth: true
                                    Layout.margins: 4
                                    spacing: 2

                                    required property var modelData
                                    required property int index

                                    property string paramKey: modelData.pKey || String(modelData.pIdx)
                                    property string paramName: modelData.name
                                    property real paramMin: modelData.min || 0
                                    property real paramMax: modelData.max || 1
                                    property int paramIdx: modelData.pIdx
                                    property string paramType: modelData.type || "slider"

                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: 6

                                        Label {
                                            text: audioPluginParamDelegate.paramName
                                            color: palette.text
                                            font.pixelSize: 11
                                            Layout.preferredWidth: 100
                                            elide: Text.ElideRight
                                        }

                                        Slider {
                                            id: audioPluginSlider
                                            Layout.fillWidth: true
                                            from: audioPluginParamDelegate.paramMin
                                            to: audioPluginParamDelegate.paramMax
                                            value: {
                                                var _ = root._audioKfRev;
                                                var v = Workspace.currentTimeline.audioPluginEvaluatedParam(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, root._audioRelFrame);
                                                return (v !== undefined && v !== null) ? Number(v) : audioPluginParamDelegate.paramMin;
                                            }
                                            onMoved: {
                                                var kfs = Workspace.currentTimeline.audioPluginKeyframeListForUi(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey);
                                                if (kfs.length > 0) {
                                                    Workspace.currentTimeline.setAudioPluginKeyframe(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, root._audioRelFrame, value, {"interp": "linear"});
                                                } else {
                                                    Workspace.currentTimeline.setEffectParameter(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramIdx, value);
                                                }
                                            }
                                        }

                                        TextField {
                                            Layout.preferredWidth: 56
                                            text: {
                                                var _ = root._audioKfRev;
                                                var v = Workspace.currentTimeline.audioPluginEvaluatedParam(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, root._audioRelFrame);
                                                return (v !== undefined && v !== null) ? Number(v).toFixed(3) : "0.000";
                                            }
                                            horizontalAlignment: Text.AlignHCenter
                                            font.pixelSize: 11
                                            onEditingFinished: {
                                                Workspace.currentTimeline.setEffectParameter(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramIdx, Number(text));
                                            }
                                        }

                                        Button {
                                            Layout.preferredWidth: 24
                                            flat: true
                                            text: "K"
                                            font.pixelSize: 10
                                            font.bold: true
                                            onClicked: {
                                                var kfs = Workspace.currentTimeline.audioPluginKeyframeListForUi(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey);
                                                if (kfs.length === 0) {
                                                    var curVal = Workspace.currentTimeline.audioPluginEvaluatedParam(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, 0);
                                                    Workspace.currentTimeline.setAudioPluginKeyframe(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, 0, (curVal !== undefined && curVal !== null) ? curVal : 0, {"interp": "linear"});
                                                    curVal = Workspace.currentTimeline.audioPluginEvaluatedParam(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, root._audioClipDur);
                                                    Workspace.currentTimeline.setAudioPluginKeyframe(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, root._audioClipDur, (curVal !== undefined && curVal !== null) ? curVal : 0, {"interp": "linear"});
                                                }
                                            }
                                        }
                                    }

                                    Item {
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 12
                                        property var rawKfs: Workspace.currentTimeline.audioPluginKeyframeListForUi(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey)
                                        Rectangle { anchors.centerIn: parent; width: parent.width; height: 2; color: palette.mid }
                                        Repeater {
                                            model: parent.rawKfs
                                            Rectangle {
                                                required property var modelData
                                                property int kfFrame: modelData.frame
                                                property bool isEndpoint: kfFrame === 0 || kfFrame === root._audioClipDur
                                                width: 8; height: 8; rotation: 45; antialiasing: true
                                                color: kfMa2.containsMouse ? palette.highlight : palette.text
                                                anchors.verticalCenter: parent.verticalCenter
                                                x: Math.min(parent.width - 4, (kfFrame / Math.max(1, root._audioClipDur)) * parent.width - 4)
                                                MouseArea {
                                                    id: kfMa2
                                                    anchors.fill: parent; anchors.margins: -4; hoverEnabled: true
                                                    cursorShape: parent.isEndpoint ? Qt.ArrowCursor : Qt.PointingHandCursor
                                                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                                                    onClicked: function(mouse) {
                                                        if (mouse.button === Qt.LeftButton && Workspace.currentTimeline && Workspace.currentTimeline.transport)
                                                            Workspace.currentTimeline.transport.setCurrentFrame_seek(Workspace.currentTimeline.clipStartFrame + parent.kfFrame);
                                                        else if (mouse.button === Qt.RightButton && !parent.isEndpoint && Workspace.currentTimeline)
                                                            Workspace.currentTimeline.removeAudioPluginKeyframe(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, parent.kfFrame);
                                                    }
                                                }
                                            }
                                        }
                                        Rectangle { width: 1; height: parent.height; color: palette.highlight; x: (root._audioRelFrame / Math.max(1, root._audioClipDur)) * parent.width; visible: root._audioClipDur > 0 }
                                        MouseArea {
                                            anchors.fill: parent; acceptedButtons: Qt.LeftButton | Qt.RightButton
                                            onDoubleClicked: function(mouse) {
                                                var f = Math.round((mouse.x / parent.width) * root._audioClipDur);
                                                f = Math.max(0, Math.min(root._audioClipDur, f));
                                                var v = Workspace.currentTimeline.audioPluginEvaluatedParam(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, f);
                                                Workspace.currentTimeline.setAudioPluginKeyframe(targetClipId, audioEffectRoot.effectIndex, audioPluginParamDelegate.paramKey, f, (v !== undefined && v !== null) ? v : 0, {"interp": "linear"});
                                            }
                                        }
                                    }
                                }

                            }

                        }

                    }

                }

            }

        }

        handle: Rectangle {
            implicitWidth: 4
            implicitHeight: 4
            color: splitMouseArea.pressed ? palette.highlight : palette.mid
            opacity: (splitMouseArea.pressed || splitMouseArea.containsMouse) ? 1 : 0

            MouseArea {
                id: splitMouseArea

                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.NoButton
                cursorShape: Qt.SplitHCursor
            }

        }

    }

    // エフェクトサイドバー向け Delete ショートカット
    Shortcut {
        sequence: "Delete"
        context: Qt.WindowShortcut
        onActivated: {
            if (!Workspace.currentTimeline)
                return ;

            var indices = sidebarList.selectedIndices.length > 0 ? sidebarList.selectedIndices.slice() : (sidebarList.currentIndex >= 0 ? [sidebarList.currentIndex] : []);
            if (indices.length === 0)
                return ;

            root.executeEffectDelete(indices);
        }
    }

    menuBar: MenuBar {
        Menu {
            // ignore

            id: filterMenu

            property string searchText: ""
            property string _lastBuildSearch: ""
            property int _lastBuiltClipId: -2
            property var _dynamicObjects: []

            function rebuildMenu() {
                // クリップが変わっておらず、かつ検索ワードも変わっていない場合のみスキップ
                if (_lastBuiltClipId === targetClipId && _lastBuildSearch === searchText && filterMenu.count > 0)
                    return ;

                _clearDynamicMenu();
                _lastBuildSearch = searchText;
                _lastBuiltClipId = targetClipId;
                if (!Workspace.currentTimeline)
                    return ;

                _doBuild();
            }

            function _registerDynamic(obj) {
                if (obj)
                    _dynamicObjects.push(obj);

                return obj;
            }

            function _clearDynamicMenu() {
                for (var i = 0; i < _dynamicObjects.length; ++i) {
                    if (_dynamicObjects[i])
                        _dynamicObjects[i].destroy();

                }
                _dynamicObjects = [];
                // 検索ボックス (index 0) を除外してクリア
                while (filterMenu.count > 1)filterMenu.takeItem(1)
            }

            function _doBuild() {
                if (searchText !== "") {
                    if (targetClipId !== -1 && Workspace.currentTimeline.isAudioClip(targetClipId)) {
                        var cats = Workspace.currentTimeline.getPluginCategories();
                        for (var c = 0; c < cats.length; c++) {
                            var plugins = Workspace.currentTimeline.getPluginsByCategory(cats[c]);
                            for (var p = 0; p < plugins.length; p++) {
                                var plug = plugins[p];
                                if (plug.name.toLowerCase().indexOf(searchText.toLowerCase()) !== -1)
                                    (function(id) {
                                    var it = _registerDynamic(menuItemComp.createObject(root.contentItem, {
                                        "text": plug.name,
                                        "iconName": "music_line"
                                    }));
                                    it.triggered.connect(() => {
                                        Workspace.currentTimeline.addAudioPlugin(targetClipId, id);
                                    });
                                    filterMenu.addItem(it);
                                })(plug.id);

                            }
                        }
                    } else {
                        var effects = Workspace.currentTimeline.getAvailableEffects();
                        buildFiltered(filterMenu, effects);
                    }
                    return ;
                }
                if (targetClipId !== -1 && Workspace.currentTimeline.isAudioClip(targetClipId)) {
                    var categories = Workspace.currentTimeline.getPluginCategories();
                    for (var c = 0; c < categories.length; c++) {
                        var catName = categories[c];
                        var subMenu = _registerDynamic(subMenuComp.createObject(root.contentItem, {
                            "title": catName
                        }));
                        var plugins = Workspace.currentTimeline.getPluginsByCategory(catName);
                        for (var p = 0; p < plugins.length; p++) {
                            (function(id) {
                                var plugItem = _registerDynamic(menuItemComp.createObject(root.contentItem, {
                                    "text": plugins[p].name,
                                    "iconName": "music_line"
                                }));
                                plugItem.triggered.connect(() => {
                                    Workspace.currentTimeline.addAudioPlugin(targetClipId, id);
                                });
                                subMenu.addItem(plugItem);
                            })(plugins[p].id);
                        }
                        filterMenu.addMenu(subMenu);
                    }
                } else {
                    var effects = Workspace.currentTimeline.getAvailableEffects();
                    buildMenu(filterMenu, effects);
                }
            }

            function buildFiltered(parentMenu, items) {
                var filter = searchText.toLowerCase();
                for (var i = 0; i < items.length; ++i) {
                    var node = items[i];
                    if (node.isCategory) {
                        buildFiltered(parentMenu, node.children);
                    } else if (node.name.toLowerCase().indexOf(filter) !== -1) {
                        var effItem = _registerDynamic(menuItemComp.createObject(root.contentItem, {
                            "text": node.name,
                            "iconName": "magic_line"
                        }));
                        (function(id) {
                            effItem.triggered.connect(() => {
                                Workspace.currentTimeline.addEffect(targetClipId, id);
                            });
                        })(node.id);
                        parentMenu.addItem(effItem);
                    }
                }
            }

            function buildMenu(parentMenu, items) {
                for (var i = 0; i < items.length; ++i) {
                    var node = items[i];
                    if (node.isCategory) {
                        var subMenu = _registerDynamic(subMenuComp.createObject(root.contentItem, {
                            "title": node.title
                        }));
                        buildMenu(subMenu, node.children);
                        parentMenu.addMenu(subMenu);
                    } else {
                        var effItem = _registerDynamic(menuItemComp.createObject(root.contentItem, {
                            "text": node.name,
                            "iconName": "magic_line"
                        }));
                        (function(id) {
                            effItem.triggered.connect(() => {
                                Workspace.currentTimeline.addEffect(targetClipId, id);
                            });
                        })(node.id);
                        parentMenu.addItem(effItem);
                    }
                }
            }

            title: qsTr("エフェクトを追加")
            onAboutToShow: {
                rebuildMenu();
                Qt.callLater(() => {
                    effectSearchField.forceActiveFocus();
                });
            }

            TextField {
                id: effectSearchField

                placeholderText: qsTr("検索...")
                width: parent.width
                onTextChanged: {
                    filterMenu.searchText = text;
                    filterMenu.rebuildMenu();
                }
            }

            Component {
                id: subMenuComp

                Menu {
                }

            }

            Component {
                id: menuItemComp

                Common.IconMenuItem {
                }

            }

        }

    }

}
