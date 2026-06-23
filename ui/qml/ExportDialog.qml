import AviQtl.Core 1.0
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs as Dialogs
import QtQuick.Layouts
import "common" as Common

Common.AviQtlWindow {
    id: root

    property var project: Workspace.currentTimeline ? Workspace.currentTimeline.project : null
    property var ownerWindow: null
    readonly property double pFps: project ? project.fps : 60
    readonly property string _platformDefaultCodec: "libx264"
    property string defaultCodec: SettingsManager ? SettingsManager.value("exportDefaultCodec", _platformDefaultCodec) : _platformDefaultCodec
    property int defaultBitrateMbps: SettingsManager ? SettingsManager.value("exportDefaultBitrateMbps", 15) : 15
    property int defaultCrf: SettingsManager ? SettingsManager.value("exportDefaultCrf", 20) : 20
    property string defaultAudioCodec: SettingsManager ? SettingsManager.value("exportDefaultAudioCodec", "aac") : "aac"
    property int defaultAudioKbps: SettingsManager ? SettingsManager.value("exportDefaultAudioBitrateKbps", 192) : 192
    property bool isImageSequence: formatCombo.currentIndex === 1
    property string imageFormat: imageFormatCombo.currentIndex === 0 ? "PNG" : "JPEG"

    property var availableVideoCodecs: []
    property var availableAudioCodecs: []

    function show() {
        refreshAvailableCodecs();
        visible = true;
    }

    function open() {
        refreshAvailableCodecs();
        visible = true;
    }

    function refreshAvailableCodecs() {
        if (Workspace.currentTimeline) {
            availableVideoCodecs = Workspace.currentTimeline.availableVideoEncoders();
            availableAudioCodecs = Workspace.currentTimeline.availableAudioEncoders();
        }
    }

    function isCodecAvailable(codecValue) {
        return availableVideoCodecs.length === 0 || availableVideoCodecs.indexOf(codecValue) >= 0;
    }

    function isAudioCodecAvailable(codecValue) {
        return availableAudioCodecs.length === 0 || availableAudioCodecs.indexOf(codecValue) >= 0;
    }

    function isValidExportPath(path) {
        if (!path || path.trim() === "")
            return false;

        if (Qt.platform.os === "windows") {
            // Reject illegal Windows characters
            if (/[<>"|?*]/.test(path))
                return false;
            // Reject stray colons: only allow drive-letter prefix like C:\ or C:/
            var colonIdx = path.indexOf(":");
            if (colonIdx >= 0) {
                if (colonIdx !== 1 || !/^[A-Za-z]$/.test(path[0]))
                    return false;
                if (path.length < 3 || (path[2] !== "\\" && path[2] !== "/"))
                    return false;
                // Reject any additional colons
                if (path.indexOf(":", colonIdx + 1) >= 0)
                    return false;
            }
        } else {
            if (/[\0]/.test(path))
                return false;
        }

        return true;
    }

    title: qsTr("メディアの書き出し")
    width: 620
    height: 580
    modality: ownerWindow ? Qt.WindowModal : Qt.ApplicationModal
    transientParent: ownerWindow
    flags: Qt.Dialog | Qt.WindowTitleHint | Qt.WindowSystemMenuHint | Qt.WindowCloseButtonHint

    // 進捗オーバーレイ
    Rectangle {
        id: progressOverlay

        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.75)
        visible: Workspace.currentTimeline && Workspace.currentTimeline.isExporting
        z: 100

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 16
            width: parent.width * 0.7

            Label {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("書き出し中...")
                font.pixelSize: 16
                font.bold: true
                color: "white"
            }

            ProgressBar {
                id: exportProgressBar

                Layout.fillWidth: true
                from: 0
                to: 100
                value: 0
            }

            Label {
                id: progressLabel

                Layout.alignment: Qt.AlignHCenter
                text: qsTr("0 / 0 フレーム")
                color: palette.mid
                font.pixelSize: 11
            }

            Button {
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("キャンセル")
                onClicked: cancelConfirmDialog.open()
            }

        }

    }

    // 進捗シグナルの受信
    Connections {
        function onExportProgressChanged(progress, current, total, etaSeconds) {
            exportProgressBar.value = progress;
            if (etaSeconds > 0) {
                var etaText = "";
                if (etaSeconds >= 3600) {
                    var hours = Math.floor(etaSeconds / 3600);
                    var minutes = Math.floor((etaSeconds % 3600) / 60);
                    etaText = qsTr(" (残り %1時間%2分)").arg(hours).arg(minutes);
                } else if (etaSeconds >= 60) {
                    var minutes = Math.floor(etaSeconds / 60);
                    var seconds = etaSeconds % 60;
                    etaText = qsTr(" (残り %1分%2秒)").arg(minutes).arg(seconds);
                } else {
                    etaText = qsTr(" (残り %1秒)").arg(etaSeconds);
                }
                progressLabel.text = qsTr("%1 / %2 フレーム%3").arg(current).arg(total).arg(etaText);
            } else {
                progressLabel.text = qsTr("%1 / %2 フレーム").arg(current).arg(total);
            }
        }

        function onExportFinished(success, message) {
            resultPopup.message = message;
            resultPopup.success = success;
            resultPopup.open();
        }

        target: Workspace.currentTimeline
    }

    // 完了ポップアップ
    Dialog {
        id: resultPopup

        property string message: ""
        property bool success: true

        title: success ? qsTr("完了") : qsTr("エラー")
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok

        Label {
            text: resultPopup.message
        }

    }

    // メインUI
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12

        // ファイルパス
        RowLayout {
            Layout.fillWidth: true

            Label {
                text: qsTr("形式:")
            }

            ComboBox {
                id: formatCombo
                model: [qsTr("動画ファイル"), qsTr("画像シーケンス")]
                currentIndex: 0
            }

            ComboBox {
                id: imageFormatCombo
                visible: root.isImageSequence
                model: ["PNG", "JPEG"]
                currentIndex: 0
            }

            TextField {
                id: filePathField

                Layout.fillWidth: true
                placeholderText: root.isImageSequence ? qsTr("保存先フォルダ...") : qsTr("保存先ファイルパス...")
            }

            Button {
                text: qsTr("参照...")
                onClicked: root.isImageSequence ? folderDialog.open() : fileDialog.open()
            }

        }

        // ビデオ設定
        GroupBox {
            title: qsTr("映像")
            Layout.fillWidth: true
            visible: !root.isImageSequence

            GridLayout {
                columns: 4
                rowSpacing: 8
                columnSpacing: 12
                width: parent.width

                Label {
                    text: qsTr("解像度:")
                }

                Label {
                    text: ((project ? project.width : DefaultWidth)) + " × " + ((project ? project.height : DefaultHeight))
                    font.bold: true
                    Layout.columnSpan: 3
                }

                Label {
                    text: qsTr("FPS:")
                }

                Label {
                    text: pFps.toFixed(3)
                    font.bold: true
                    Layout.columnSpan: 3
                }

                Label {
                    text: qsTr("コーデック:")
                }

                ComboBox {
                    id: codecCombo

                    Layout.columnSpan: 3
                    Layout.fillWidth: true
                    model: {
                        var allCodecs = [{
                            "text": "H.264 – libx264 (SW)",
                            "value": "libx264"
                        }, {
                            "text": "H.264 – NVENC (NVIDIA)",
                            "value": "h264_nvenc"
                        }, {
                            "text": "H.264 – AMF (AMD)",
                            "value": "h264_amf"
                        }, {
                            "text": "H.264 – QSV (Intel)",
                            "value": "h264_qsv"
                        }, {
                            "text": "H.264 – VAAPI (Linux)",
                            "value": "h264_vaapi"
                        }, {
                            "text": "HEVC – libx265 (SW)",
                            "value": "libx265"
                        }, {
                            "text": "HEVC – NVENC (NVIDIA)",
                            "value": "hevc_nvenc"
                        }, {
                            "text": "HEVC – AMF (AMD)",
                            "value": "hevc_amf"
                        }, {
                            "text": "HEVC – QSV (Intel)",
                            "value": "hevc_qsv"
                        }, {
                            "text": "HEVC – VAAPI (Linux)",
                            "value": "hevc_vaapi"
                        }, {
                            "text": "AV1 – libaom (SW)",
                            "value": "libaom-av1"
                        }, {
                            "text": "AV1 – NVENC (NVIDIA)",
                            "value": "av1_nvenc"
                        }, {
                            "text": "AV1 – AMF (AMD)",
                            "value": "av1_amf"
                        }, {
                            "text": "AV1 – VAAPI (Linux)",
                            "value": "av1_vaapi"
                        }];
                        if (root.availableVideoCodecs.length === 0)
                            return allCodecs;
                        return allCodecs.filter(function(c) {
                            return root.availableVideoCodecs.indexOf(c.value) >= 0;
                        });
                    }
                    textRole: "text"
                    Component.onCompleted: {
                        var idx = -1;
                        for (var i = 0; i < model.length; i++) {
                            if (model[i].value === root.defaultCodec) {
                                idx = i;
                                break;
                            }
                        }
                        if (idx >= 0)
                            currentIndex = idx;
                        else
                            currentIndex = 0; // Fallback to libx264 (software, works everywhere)
                    }
                }

                Label {
                    text: qsTr("品質モード:")
                }

                ButtonGroup {
                    id: qualityModeGroup
                }

                RadioButton {
                    id: crfRadio

                    text: qsTr("CRF")
                    ButtonGroup.group: qualityModeGroup
                    checked: true
                }

                RadioButton {
                    text: qsTr("ビットレート")
                    ButtonGroup.group: qualityModeGroup
                }

                Item {
                    Layout.columnSpan: 1
                }

                // CRF スライダー
                Label {
                    text: qsTr("CRF:")
                    visible: crfRadio.checked
                }

                RowLayout {
                    visible: crfRadio.checked
                    Layout.columnSpan: 3
                    Layout.fillWidth: true

                    Slider {
                        id: crfSlider

                        Layout.fillWidth: true
                        from: 0
                        to: 51
                        value: root.defaultCrf
                        stepSize: 1
                    }

                    Label {
                        text: crfSlider.value.toFixed(0)
                        Layout.preferredWidth: 28
                    }

                    Label {
                        text: crfSlider.value <= 17 ? qsTr("高品質") : crfSlider.value <= 28 ? qsTr("標準") : qsTr("低品質")
                        font.pixelSize: 10
                        color: crfSlider.value <= 17 ? "#44cc88" : crfSlider.value <= 28 ? palette.text : "#cc4444"
                    }

                }

                // ビットレート入力
                Label {
                    text: qsTr("ビットレート:")
                    visible: !crfRadio.checked
                }

                RowLayout {
                    visible: !crfRadio.checked
                    Layout.columnSpan: 3

                    SpinBox {
                        id: bitrateSpin

                        from: 1
                        to: 500
                        value: root.defaultBitrateMbps
                        stepSize: 1
                        editable: true
                        textFromValue: (v) => {
                            return qsTr("%1 Mbps").arg(v);
                        }
                        valueFromText: (t) => {
                            return parseInt(t);
                        }
                    }

                }

                // エンコードプリセット
                Label {
                    text: qsTr("プリセット:")
                }

                ComboBox {
                    id: presetCombo

                    Layout.columnSpan: 3
                    Layout.fillWidth: true
                    model: [{
                        "text": qsTr("最速 (ultrafast)"),
                        "value": "ultrafast"
                    }, {
                        "text": qsTr("高速 (fast)"),
                        "value": "fast"
                    }, {
                        "text": qsTr("標準 (medium)"),
                        "value": "medium"
                    }, {
                        "text": qsTr("高品質 (slow)"),
                        "value": "slow"
                    }, {
                        "text": qsTr("最高品質 (veryslow)"),
                        "value": "veryslow"
                    }]
                    textRole: "text"
                    currentIndex: 2 // medium
                }

                // H.264プロファイル
                Label {
                    text: qsTr("プロファイル:")
                }

                ComboBox {
                    id: profileCombo

                    Layout.columnSpan: 3
                    Layout.fillWidth: true
                    model: [{
                        "text": qsTr("自動"),
                        "value": ""
                    }, {
                        "text": "Baseline",
                        "value": "baseline"
                    }, {
                        "text": "Main",
                        "value": "main"
                    }, {
                        "text": "High",
                        "value": "high"
                    }]
                    textRole: "text"
                    currentIndex: 0
                }

            }

        }

        // オーディオ設定
        GroupBox {
            title: qsTr("音声")
            Layout.fillWidth: true
            visible: !root.isImageSequence

            GridLayout {
                columns: 4
                rowSpacing: 8
                columnSpacing: 12
                width: parent.width

                Label {
                    text: qsTr("コーデック:")
                }

                ComboBox {
                    id: audioCodecCombo

                    Layout.columnSpan: 3
                    Layout.fillWidth: true
                    model: {
                        var allCodecs = [{
                            "text": "AAC",
                            "value": "aac"
                        }, {
                            "text": "Opus",
                            "value": "libopus"
                        }, {
                            "text": "MP3",
                            "value": "libmp3lame"
                        }, {
                            "text": "FLAC (可逆)",
                            "value": "flac"
                        }, {
                            "text": "PCM 16-bit",
                            "value": "pcm_s16le"
                        }];
                        if (root.availableAudioCodecs.length === 0)
                            return allCodecs;
                        return allCodecs.filter(function(c) {
                            return root.availableAudioCodecs.indexOf(c.value) >= 0;
                        });
                    }
                    textRole: "text"
                    Component.onCompleted: {
                        var idx = -1;
                        for (var i = 0; i < model.length; i++) {
                            if (model[i].value === root.defaultAudioCodec) {
                                idx = i;
                                break;
                            }
                        }
                        if (idx >= 0)
                            currentIndex = idx;
                        else
                            currentIndex = 0;
                    }
                }

                Label {
                    text: qsTr("ビットレート:")
                }

                ComboBox {
                    id: audioBitrateCombo

                    property int bitrate: [96000, 128000, 192000, 256000, 320000][currentIndex]

                    Layout.columnSpan: 3
                    enabled: audioCodecCombo.currentIndex < 3 // PCM/FLACは無効
                    model: ["96 kbps", "128 kbps", "192 kbps", "256 kbps", "320 kbps"]
                    Component.onCompleted: {
                        var bitrateList = [96, 128, 192, 256, 320];
                        var idx = bitrateList.indexOf(root.defaultAudioKbps);
                        if (idx >= 0)
                            currentIndex = idx;
                        else
                            currentIndex = 2;
                    }
                }

            }

        }

        // 範囲設定
        GroupBox {
            title: qsTr("範囲")
            Layout.fillWidth: true

            RowLayout {
                width: parent.width
                spacing: 12

                CheckBox {
                    id: fullRangeCheck

                    text: qsTr("タイムライン全体")
                    checked: true
                }

                Label {
                    text: qsTr("開始:")
                }

                SpinBox {
                    id: startFrameSpin

                    enabled: !fullRangeCheck.checked
                    from: 0
                    to: endFrameSpin.value - 1
                    value: 0
                    editable: true
                }

                Label {
                    text: qsTr("終了:")
                }

                SpinBox {
                    id: endFrameSpin

                    enabled: !fullRangeCheck.checked
                    from: startFrameSpin.value + 1
                    to: 999999
                    value: DefaultTotalFrames
                    editable: true
                    Component.onCompleted: {
                        if (Workspace.currentTimeline && Workspace.currentTimeline.timelineDuration > 0)
                            value = Workspace.currentTimeline.timelineDuration;

                    }

                    Connections {
                        function onClipsChanged() {
                            if (Workspace.currentTimeline && Workspace.currentTimeline.timelineDuration > 0)
                                endFrameSpin.value = Workspace.currentTimeline.timelineDuration;

                        }

                        target: Workspace.currentTimeline
                    }

                }

                Label {
                    text: {
                        var s = fullRangeCheck.checked ? 0 : startFrameSpin.value;
                        var e = fullRangeCheck.checked ? (Workspace.currentTimeline ? Workspace.currentTimeline.timelineDuration : 300) : endFrameSpin.value;
                        var sec = (e - s) / pFps;
                        return qsTr("(%1 フレーム / %2 秒)").arg(e - s).arg(sec.toFixed(2));
                    }
                    font.pixelSize: 11
                    color: palette.mid
                }

            }

        }

        Item {
            Layout.fillHeight: true
        }

        // ボタン
        RowLayout {
            Layout.fillWidth: true

            Item {
                Layout.fillWidth: true
            }

            Button {
                text: qsTr("キャンセル")
                onClicked: root.close()
            }

            Button {
                text: qsTr("書き出し開始")
                enabled: isValidExportPath(filePathField.text)
                highlighted: true
                onClicked: {
                    if (root.isImageSequence) {
                        var sf = fullRangeCheck.checked ? 0 : startFrameSpin.value;
                        var ef = fullRangeCheck.checked ? -1 : endFrameSpin.value;
                        Workspace.currentTimeline.exportImageSequence(filePathField.text, 100, root.imageFormat, sf, ef);
                    } else {
                        var codec = codecCombo.model[codecCombo.currentIndex].value;
                        var audioCodec = audioCodecCombo.model[audioCodecCombo.currentIndex].value;
                        var preset = presetCombo.model[presetCombo.currentIndex].value;
                        var profile = profileCombo.model[profileCombo.currentIndex].value;
                        Workspace.currentTimeline.exportVideoAsync({
                            "width": (project ? project.width : DefaultWidth),
                            "height": (project ? project.height : DefaultHeight),
                            "fps_num": pFps === Math.floor(pFps) ? pFps * 1000 : Math.round(pFps * 1001),
                            "fps_den": pFps === Math.floor(pFps) ? 1000 : 1001,
                            "bitrate": bitrateSpin.value * 1e+06,
                            "crf": crfRadio.checked ? crfSlider.value : -1,
                            "codecName": codec,
                            "audioCodecName": audioCodec,
                            "audioBitrate": audioBitrateCombo.bitrate,
                            "outputUrl": filePathField.text,
                            "startFrame": fullRangeCheck.checked ? 0 : startFrameSpin.value,
                            "endFrame": fullRangeCheck.checked ? -1 : endFrameSpin.value,
                            "preset": preset,
                            "profile": profile
                        });
                    }
                }
            }

        }

    }

    Dialogs.FileDialog {
        id: fileDialog

        title: qsTr("保存先を指定")
        fileMode: Dialogs.FileDialog.SaveFile
        nameFilters: ["MP4 Video (*.mp4)", "MKV Video (*.mkv)", "All files (*)"]
        onAccepted: {
            var path = selectedFile.toString();
            filePathField.text = Qt.platform.os === "windows" ? path.replace(/^file:\/{3}/, "") : path.replace(/^file:\/\//, "");
        }
    }

    Dialogs.FolderDialog {
        id: folderDialog

        title: qsTr("保存先フォルダを指定")
        onAccepted: {
            var path = folder.toString();
            filePathField.text = Qt.platform.os === "windows" ? path.replace(/^file:\/{3}/, "") : path.replace(/^file:\/\//, "");
        }
    }

    Dialog {
        id: cancelConfirmDialog

        title: qsTr("書き出しキャンセル")
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Yes | Dialog.No
        width: 300

        contentItem: Label {
            text: qsTr("書き出しをキャンセルしますか？\n進捗は失われます。")
            wrapMode: Text.WordWrap
            padding: 12
        }
        onAccepted: {
            if (Workspace.currentTimeline)
                Workspace.currentTimeline.cancelExport();
        }
    }

}
