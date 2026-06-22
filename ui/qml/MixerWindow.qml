import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "common" as Common

Common.AviQtlWindow {
    id: root

    title: qsTr("ミキサー")
    width: 800
    height: 320
    minimumWidth: 400
    minimumHeight: 200

    property var audioClips: []
    property var meterData: ({})
    property real masterPeakL: 0
    property real masterPeakR: 0

    function refreshClips() {
        if (!Workspace.currentTimeline) return;
        audioClips = Workspace.currentTimeline.getAllAudioClips();
        root.meterData = ({});
        root.masterPeakL = 0;
        root.masterPeakR = 0;
    }

    Component.onCompleted: refreshClips()

    Connections {
        target: Workspace.currentTimeline
        function onClipsChanged() { refreshClips(); }
        function onClipEffectsChanged(clipId) { refreshClips(); }
        function onAudioMeterChanged(clipId, peakLeft, peakRight, rmsLeft, rmsRight) {
            var md = root.meterData;
            md[clipId] = {"peakL": peakLeft, "peakR": peakRight, "rmsL": rmsLeft, "rmsR": rmsRight};
            root.meterData = md;

            // Sum for master
            var sumL = 0, sumR = 0;
            for (var k in md) {
                sumL = Math.max(sumL, md[k].peakL);
                sumR = Math.max(sumR, md[k].peakR);
            }
            root.masterPeakL = sumL;
            root.masterPeakR = sumR;
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 4

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 4

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                RowLayout {
                    spacing: 2
                    height: parent.height

                    Repeater {
                        model: root.audioClips

                        delegate: Rectangle {
                            id: channelStrip
                            required property var modelData
                            required property int index

                            Layout.preferredWidth: 80
                            Layout.fillHeight: true
                            radius: 4
                            color: palette.base
                            border.width: 1
                            border.color: palette.mid

                            property int clipId: modelData.id
                            property real vol: modelData.volume !== undefined ? modelData.volume : 1
                            property real panVal: modelData.pan !== undefined ? modelData.pan : 0
                            property bool isMuted: modelData.mute || false
                            property bool isSoloed: modelData.solo || false
                            property var meters: root.meterData[clipId] || {"peakL": 0, "peakR": 0, "rmsL": 0, "rmsR": 0}

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 4
                                spacing: 2

                                // Clip name
                                Label {
                                    Layout.fillWidth: true
                                    text: {
                                        var src = modelData.source || "";
                                        var parts = src.split("/");
                                        return parts[parts.length - 1] || qsTr("Audio %1").arg(channelStrip.clipId);
                                    }
                                    font.pixelSize: 9
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignHCenter
                                    color: palette.text
                                }

                                // VU Meters
                                Column {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 60
                                    spacing: 1

                                    Repeater {
                                        model: [
                                            {"name": "L", "peak": channelStrip.meters.peakL, "rms": channelStrip.meters.rmsL},
                                            {"name": "R", "peak": channelStrip.meters.peakR, "rms": channelStrip.meters.rmsR}
                                        ]
                                        delegate: Row {
                                            spacing: 2
                                            property real peakDb: modelData.peak > 0.0001 ? 20 * Math.log10(modelData.peak) : -100
                                            Label { text: modelData.name; font.pixelSize: 8; color: palette.text; width: 8 }
                                            Rectangle {
                                                width: 48; height: 8; radius: 2
                                                color: palette.mid
                                                Rectangle {
                                                    anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                                                    width: parent.width * Math.min(1, modelData.rms); height: parent.height; radius: 2
                                                    color: palette.highlight; opacity: 0.5
                                                }
                                                Rectangle {
                                                    anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                                                    width: parent.width * Math.min(1, modelData.peak); height: parent.height; radius: 2
                                                    color: modelData.peak > 0.95 ? "#d94f4f" : "#4fd97b"
                                                }
                                            }
                                            Label { text: modelData.peak > 0.95 ? "CLIP" : (peakDb > -100 ? peakDb.toFixed(1) + "dB" : "-inf"); font.pixelSize: 7; color: modelData.peak > 0.95 ? "#d94f4f" : palette.text; width: 36; horizontalAlignment: Text.AlignRight }
                                        }
                                    }
                                }

                                // Volume slider
                                Column {
                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    spacing: 2

                                    Label { text: qsTr("VOL"); font.pixelSize: 8; color: palette.text; anchors.horizontalCenter: parent.horizontalCenter }
                                    Slider {
                                        id: volSlider
                                        anchors.horizontalCenter: parent.horizontalCenter
                                        orientation: Qt.Vertical
                                        from: 0; to: 2
                                        value: channelStrip.vol
                                        Layout.fillHeight: true
                                        onMoved: {
                                            if (Workspace.currentTimeline)
                                                Workspace.currentTimeline.setAudioClipVolume(channelStrip.clipId, value);
                                        }
                                    }
                                    Label { text: volSlider.value.toFixed(2); font.pixelSize: 8; color: palette.text; anchors.horizontalCenter: parent.horizontalCenter }
                                }

                                // Pan slider
                                Column {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Label { text: qsTr("PAN"); font.pixelSize: 8; color: palette.text; anchors.horizontalCenter: parent.horizontalCenter }
                                    Slider {
                                        id: panSlider
                                        anchors.horizontalCenter: parent.horizontalCenter
                                        from: -1; to: 1
                                        value: channelStrip.panVal
                                        onMoved: {
                                            if (Workspace.currentTimeline)
                                                Workspace.currentTimeline.setAudioClipPan(channelStrip.clipId, value);
                                        }
                                    }
                                }

                                // Mute / Solo buttons
                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Button {
                                        Layout.fillWidth: true
                                        text: "M"
                                        font.pixelSize: 9
                                        font.bold: true
                                        checkable: true
                                        checked: channelStrip.isMuted
                                        palette.buttonText: checked ? "red" : palette.text
                                        onToggled: {
                                            if (Workspace.currentTimeline)
                                                Workspace.currentTimeline.setAudioClipMute(channelStrip.clipId, checked);
                                        }
                                    }
                                    Button {
                                        Layout.fillWidth: true
                                        text: "S"
                                        font.pixelSize: 9
                                        font.bold: true
                                        checkable: true
                                        checked: channelStrip.isSoloed
                                        palette.buttonText: checked ? "#ffd700" : palette.text
                                        onToggled: {
                                            if (Workspace.currentTimeline)
                                                Workspace.currentTimeline.setAudioClipSolo(channelStrip.clipId, checked);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Master section
            Rectangle {
                Layout.preferredWidth: 90
                Layout.fillHeight: true
                radius: 4
                color: palette.midlight
                border.width: 1
                border.color: palette.mid

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 4
                    spacing: 2

                    Label {
                        Layout.fillWidth: true
                        text: qsTr("MASTER")
                        font.pixelSize: 10
                        font.bold: true
                        horizontalAlignment: Text.AlignHCenter
                        color: palette.text
                    }

                    // Master VU Meters
                    Column {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 60
                        spacing: 1

                        Repeater {
                            model: [
                                {"name": "L", "peak": root.masterPeakL},
                                {"name": "R", "peak": root.masterPeakR}
                            ]
                            delegate: Row {
                                spacing: 2
                                property real peakDb: modelData.peak > 0.0001 ? 20 * Math.log10(modelData.peak) : -100
                                Label { text: modelData.name; font.pixelSize: 8; color: palette.text; width: 8 }
                                Rectangle {
                                    width: 56; height: 8; radius: 2
                                    color: palette.mid
                                    Rectangle {
                                        anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                                        width: parent.width * Math.min(1, modelData.peak); height: parent.height; radius: 2
                                        color: modelData.peak > 0.95 ? "#d94f4f" : "#4fd97b"
                                    }
                                }
                                Label { text: modelData.peak > 0.95 ? "CLIP" : (peakDb > -100 ? peakDb.toFixed(1) + "dB" : "-inf"); font.pixelSize: 7; color: modelData.peak > 0.95 ? "#d94f4f" : palette.text; width: 36; horizontalAlignment: Text.AlignRight }
                            }
                        }
                    }

                    Item { Layout.fillHeight: true }

                    // Audio clip count
                    Label {
                        Layout.fillWidth: true
                        text: qsTr("%1 clips").arg(root.audioClips.length)
                        font.pixelSize: 9
                        horizontalAlignment: Text.AlignHCenter
                        color: palette.text
                        opacity: 0.7
                    }
                }
            }
        }
    }
}
