import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: track
    Layout.columnSpan: 2
    Layout.fillWidth: true
    spacing: 4

    required property string paramName
    required property real defaultValue
    required property real sliderFrom
    required property real sliderTo
    required property int decimals
    required property string buttonLabel

    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Slider {
            Layout.fillWidth: true; from: track.sliderFrom; to: track.sliderTo
            value: { var _ = root._audioKfRev; return Number(root.audioEvaluatedParam(track.paramName, track.defaultValue)); }
            onMoved: {
                var model = root.audioEffectModel(); var idx = root.audioEffectIndex();
                if (model && idx >= 0 && Workspace.currentTimeline) {
                    var rawKfs = model.keyframeListForUi(track.paramName);
                    if (rawKfs.length > 0) Workspace.currentTimeline.setKeyframe(targetClipId, idx, track.paramName, root._audioRelFrame, value, {"interp": "linear"});
                    else root.setAudioParam(track.paramName, value);
                } else root.setAudioParam(track.paramName, value);
            }
        }
        TextField {
            Layout.preferredWidth: 64
            text: { var _ = root._audioKfRev; return Number(root.audioEvaluatedParam(track.paramName, track.defaultValue)).toFixed(track.decimals); }
            horizontalAlignment: Text.AlignHCenter
            onEditingFinished: root.setAudioParam(track.paramName, Number(text))
        }
        Button {
            Layout.preferredWidth: 100
            text: {
                var il = {"linear": qsTr(" (直線)"), "ease_in": qsTr(" (加速)"), "ease_out": qsTr(" (減速)"), "ease_in_out": qsTr(" (加減速)"), "custom": qsTr(" (ベジェ)")};
                var rawKfs = root.audioKeyframeList(track.paramName);
                return qsTr(track.buttonLabel) + (rawKfs.length > 0 ? (il[root._audioInterpAt(track.paramName, root._audioRelFrame)] || "") : "");
            }
            onClicked: {
                var model = root.audioEffectModel(); var idx = root.audioEffectIndex();
                if (!model || idx < 0 || !Workspace.currentTimeline) return;
                if (model.keyframeListForUi(track.paramName).length === 0) {
                    Workspace.currentTimeline.setKeyframe(targetClipId, idx, track.paramName, 0, root.audioParamValue(track.paramName, track.defaultValue), {"interp": "linear"});
                    Workspace.currentTimeline.setKeyframe(targetClipId, idx, track.paramName, root._audioClipDur, root.audioParamValue(track.paramName, track.defaultValue), {"interp": "linear"});
                }
                var win = WindowManager.getWindow("easingConfig");
                if (win) win.openConfig({"clipId": targetClipId, "effectIndex": idx, "effectModel": model, "paramName": track.paramName, "keyframeFrame": root._audioRelFrame});
            }
        }
        TextField {
            Layout.preferredWidth: 64
            text: {
                var _ = root._audioKfRev;
                var rawKfs = root.audioKeyframeList(track.paramName);
                if (rawKfs.length < 2) return Number(root.audioEvaluatedParam(track.paramName, track.defaultValue)).toFixed(track.decimals);
                var nf = root._audioNextKfFrame(rawKfs);
                var model = root.audioEffectModel();
                var v = model ? model.evaluatedParam(track.paramName, nf, root._projectFps) : track.defaultValue;
                return (v !== undefined && v !== null) ? Number(v).toFixed(track.decimals) : Number(track.defaultValue).toFixed(track.decimals);
            }
            horizontalAlignment: Text.AlignHCenter
            enabled: root.audioKeyframeList(track.paramName).length >= 2
            opacity: enabled ? 1 : 0.45
            onEditingFinished: {
                var model = root.audioEffectModel(); var idx = root.audioEffectIndex();
                if (!model || idx < 0) return;
                var rawKfs = model.keyframeListForUi(track.paramName);
                var nf = root._audioNextKfFrame(rawKfs);
                Workspace.currentTimeline.setKeyframe(targetClipId, idx, track.paramName, nf, Number(text), {"interp": "linear"});
            }
        }
        Slider {
            Layout.fillWidth: true; from: track.sliderFrom; to: track.sliderTo
            enabled: root.audioKeyframeList(track.paramName).length >= 2
            opacity: enabled ? 1 : 0.45
            value: {
                var _ = root._audioKfRev;
                var rawKfs = root.audioKeyframeList(track.paramName);
                if (rawKfs.length < 2) return Number(root.audioEvaluatedParam(track.paramName, track.defaultValue));
                var nf = root._audioNextKfFrame(rawKfs);
                var model = root.audioEffectModel();
                var v = model ? model.evaluatedParam(track.paramName, nf, root._projectFps) : track.defaultValue;
                return (v !== undefined && v !== null) ? Number(v) : track.defaultValue;
            }
            onMoved: {
                var model = root.audioEffectModel(); var idx = root.audioEffectIndex();
                if (!model || idx < 0 || !Workspace.currentTimeline) return;
                var rawKfs = model.keyframeListForUi(track.paramName);
                var nf = root._audioNextKfFrame(rawKfs);
                Workspace.currentTimeline.setKeyframe(targetClipId, idx, track.paramName, nf, value, {"interp": "linear"});
            }
        }
    }

    Item {
        Layout.fillWidth: true
        Layout.preferredHeight: 12
        property var rawKfs: root.audioKeyframeList(track.paramName)
        property var kfs: root.audioKeyframesWithEnd(rawKfs, root._audioClipDur, track.paramName)
        property string kfMaId: "kfMa_" + track.paramName
        Rectangle { anchors.centerIn: parent; width: parent.width; height: 2; color: palette.mid }
        Repeater {
            model: parent.kfs
            Rectangle {
                required property var modelData
                property int kfFrame: modelData.frame
                property bool isEndpoint: kfFrame === 0 || !!modelData.virtualEnd
                width: 8; height: 8; rotation: 45; antialiasing: true
                color: kfMa.containsMouse ? palette.highlight : palette.text
                anchors.verticalCenter: parent.verticalCenter
                x: Math.min(parent.width - 4, (kfFrame / Math.max(1, root._audioClipDur)) * parent.width - 4)
                MouseArea {
                    id: kfMa
                    anchors.fill: parent; anchors.margins: -4; hoverEnabled: true
                    cursorShape: parent.isEndpoint ? Qt.ArrowCursor : Qt.PointingHandCursor
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    onClicked: function(mouse) {
                        if (mouse.button === Qt.LeftButton && Workspace.currentTimeline?.transport)
                            Workspace.currentTimeline.transport.setCurrentFrame_seek(Workspace.currentTimeline.clipStartFrame + parent.kfFrame);
                        else if (mouse.button === Qt.RightButton && !parent.isEndpoint && Workspace.currentTimeline)
                            Workspace.currentTimeline.removeKeyframe(targetClipId, root.audioEffectIndex(), track.paramName, parent.kfFrame);
                    }
                }
            }
        }
        Rectangle { width: 1; height: parent.height; color: palette.highlight; x: (root._audioRelFrame / Math.max(1, root._audioClipDur)) * parent.width; visible: root._audioClipDur > 0 }
        MouseArea {
            anchors.fill: parent; acceptedButtons: Qt.LeftButton | Qt.RightButton
            onDoubleClicked: function(mouse) {
                var model = root.audioEffectModel(); var idx = root.audioEffectIndex();
                if (!model || idx < 0 || !Workspace.currentTimeline) return;
                var f = Math.round((mouse.x / parent.width) * root._audioClipDur);
                f = Math.max(0, Math.min(root._audioClipDur, f));
                var v = model.evaluatedParam(track.paramName, f, root._projectFps);
                Workspace.currentTimeline.setKeyframe(targetClipId, idx, track.paramName, f, (v !== undefined && v !== null) ? v : track.defaultValue, {"interp": "linear"});
            }
        }
    }
}
