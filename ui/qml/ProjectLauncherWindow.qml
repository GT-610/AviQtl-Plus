import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import "common" as Common

Common.AviQtlWindow {
    id: root

    width: 700
    height: 500
    title: qsTr("AviQtl Plus - プロジェクトランチャー")

    function refreshLauncherData() {
        if (SettingsManager && SettingsManager.settings) {
            var recent = SettingsManager.settings.recentProjects || [];
            recentModel.clear();
            var maxRecent = SettingsManager ? SettingsManager.value("recentProjectMaxCount", 10) : 10;
            for (var i = 0; i < Math.min(recent.length, maxRecent); i++) {
                recentModel.append(recent[i]);
            }
            // 設定からデフォルト値を読み込む
            widthField.text = SettingsManager.settings.defaultProjectWidth || "" + DefaultWidth;
            heightField.text = SettingsManager.settings.defaultProjectHeight || "" + DefaultHeight;
            fpsField.text = SettingsManager.settings.defaultProjectFps || "" + DefaultFps;
            sampleRateField.text = SettingsManager.settings.defaultProjectSampleRate || "" + DefaultSampleRate;
        }
    }

    Component.onCompleted: refreshLauncherData()
    onVisibleChanged: {
        if (!visible) {
            recoveryWindow.close();
            return;
        }
        refreshLauncherData();
        if (Workspace && Workspace.recoveries && Workspace.recoveries.length > 0)
            Qt.callLater(function() { recoveryWindow.open(); });
    }

    FontLoader {
        source: "qrc:/resources/remixicon.ttf"
    }

    ListModel {
        id: recentModel
    }

    ProjectRecoveryWindow {
        id: recoveryWindow
        ownerWindow: root
    }

    Connections {
        target: Workspace

        function onTabsChanged() {
            if (Workspace.tabs.length > 0)
                root.close();
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 20
        spacing: 20

        // 左側：新規プロジェクト
        ColumnLayout {
            Layout.fillHeight: true
            Layout.preferredWidth: parent.width * 0.45
            spacing: 15

            Label {
                text: qsTr("新規プロジェクト")
                font.pixelSize: 18
                font.bold: true
            }

            GroupBox {
                title: qsTr("プロジェクト設定")
                Layout.fillWidth: true

                GridLayout {
                    columns: 2
                    rowSpacing: 10
                    columnSpacing: 10
                    anchors.fill: parent

                    Label {
                        text: qsTr("幅 (横):")
                    }

                    TextField {
                        id: widthField

                        Layout.fillWidth: true

                        validator: IntValidator {
                            bottom: 1
                            top: 8000
                        }

                    }

                    Label {
                        text: qsTr("高さ (縦):")
                    }

                    TextField {
                        id: heightField

                        Layout.fillWidth: true

                        validator: IntValidator {
                            bottom: 1
                            top: 8000
                        }

                    }

                    Label {
                        text: qsTr("フレームレート (FPS):")
                    }

                    TextField {
                        id: fpsField

                        Layout.fillWidth: true

                        validator: DoubleValidator {
                            bottom: 1
                            top: 240
                        }

                    }

                    Label {
                        text: qsTr("サンプリングレート:")
                    }

                    TextField {
                        id: sampleRateField

                        Layout.fillWidth: true

                        validator: IntValidator {
                            bottom: 8000
                            top: 192000
                        }

                    }

                }

            }

            Button {
                id: newProjectBtn

                highlighted: true
                Layout.fillWidth: true
                enabled: widthField.acceptableInput && heightField.acceptableInput && fpsField.acceptableInput && sampleRateField.acceptableInput
                onClicked: {
                    // C++ 0引数の newProject() のみ使用（QML はオーバーロード不可）
                    Workspace.newProject();
                    // newProject() は setCurrentIndex() まで同期完了するため
                    // 直後に currentTimeline.project へ代入可能
                    var proj = Workspace.currentTimeline ? Workspace.currentTimeline.project : null;
                    if (proj) {
                        proj.width = parseInt(widthField.text);
                        proj.height = parseInt(heightField.text);
                        proj.fps = parseFloat(fpsField.text);
                        proj.sampleRate = parseInt(sampleRateField.text);
                    }
                    root.close();
                }

                contentItem: RowLayout {
                    spacing: 8

                    Common.AviQtlIcon {
                        iconName: "file_add_line"
                        color: newProjectBtn.palette.buttonText
                    }

                    Text {
                        text: qsTr("新規プロジェクトを作成")
                        color: newProjectBtn.palette.buttonText
                        font: newProjectBtn.font
                    }

                }

            }

            Item {
                Layout.fillHeight: true
            }

        }

        // 右側：最近使ったプロジェクト
        ColumnLayout {
            Layout.fillHeight: true
            Layout.fillWidth: true
            spacing: 15

            Label {
                text: qsTr("最近使ったプロジェクト")
                font.pixelSize: 18
                font.bold: true
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                ListView {
                    id: recentListView

                    model: recentModel
                    spacing: 5

                    Label {
                        anchors.centerIn: parent
                        visible: recentModel.count === 0
                        text: qsTr("最近使ったプロジェクトはありません")
                        color: palette.text
                        opacity: 0.65
                    }

                    delegate: ItemDelegate {
                        width: recentListView.width
                        height: 60
                        hoverEnabled: true
                        ToolTip.visible: hovered && Boolean(model.path)
                        ToolTip.text: model.path || ""
                        onClicked: {
                            Workspace.loadProject(model.path);
                            root.close();
                        }

                        ColumnLayout {
                            anchors.fill: parent
                            anchors.margins: 5

                            Label {
                                text: model.name || qsTr("無題のプロジェクト")
                                font.bold: true
                            }

                            Label {
                                text: model.path || ""
                                font.pixelSize: 10
                                color: palette.text
                                opacity: 0.65
                                elide: Text.ElideMiddle
                                Layout.fillWidth: true
                            }

                            Label {
                                text: qsTr("%1 × %2 @ %3 fps").arg(model.width || DefaultWidth).arg(model.height || DefaultHeight).arg(model.fps || DefaultFps)
                                font.pixelSize: 10
                            }

                        }

                    }

                }

            }

            Button {
                id: openProjectBtn

                Layout.fillWidth: true
                onClicked: fileDialog.open()

                contentItem: RowLayout {
                    spacing: 8

                    Common.AviQtlIcon {
                        iconName: "folder_open_line"
                        color: openProjectBtn.palette.buttonText
                    }

                    Text {
                        text: qsTr("既存プロジェクトを開く...")
                        color: openProjectBtn.palette.buttonText
                        font: openProjectBtn.font
                    }

                }

            }

        }

    }

    FileDialog {
        id: fileDialog

        title: qsTr("プロジェクトファイルを開く")
        nameFilters: [qsTr("AviQtl Plus Project (*.aviqtl)"), qsTr("All files (*)")]
        onAccepted: {
            Workspace.loadProject(fileDialog.selectedFile);
            root.close();
        }
    }

}
