import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "common" as Common

Common.AviQtlWindow {
    id: root

    property var ownerWindow: null
    property string pendingDiscardId: ""
    property string pendingDiscardName: ""
    property string recoveryError: ""

    function open() {
        recoveryError = "";
        visible = true;
        raise();
        requestActivate();
    }

    title: qsTr("Recover unsaved projects")
    width: 660
    height: 440
    minimumWidth: 520
    minimumHeight: 320
    modality: Qt.NonModal
    transientParent: ownerWindow
    flags: Qt.Window | Qt.WindowTitleHint | Qt.WindowSystemMenuHint | Qt.WindowCloseButtonHint
    visible: false

    Connections {
        target: Workspace

        function onRecoveriesChanged() {
            if (root.visible && Workspace.recoveries.length === 0)
                root.close();
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 16
        spacing: 12

        Label {
            Layout.fillWidth: true
            text: qsTr("AviQtl found recovery snapshots left by an interrupted session. Recovering opens a new unsaved project and never overwrites the original file.")
            wrapMode: Text.WordWrap
        }

        Label {
            Layout.fillWidth: true
            visible: root.recoveryError !== ""
            text: root.recoveryError
            color: palette.text
            font.bold: true
            wrapMode: Text.WordWrap
        }

        ListView {
            id: recoveryList

            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 8
            model: Workspace ? Workspace.recoveries : []

            delegate: Frame {
                required property var modelData

                width: recoveryList.width
                padding: 12

                contentItem: RowLayout {
                    spacing: 10

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 3

                        Label {
                            Layout.fillWidth: true
                            text: modelData.name
                            font.bold: true
                            elide: Text.ElideRight
                        }

                        Label {
                            Layout.fillWidth: true
                            text: modelData.valid ? qsTr("Saved %1").arg(Qt.formatDateTime(modelData.savedAt, Qt.DefaultLocaleShortDate)) : qsTr("Invalid recovery: %1").arg(modelData.error)
                            color: palette.text
                            opacity: modelData.valid ? 0.7 : 0.85
                            elide: Text.ElideRight
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: Boolean(modelData.originalProjectUrl)
                            text: modelData.originalProjectUrl || ""
                            color: palette.text
                            opacity: 0.6
                            font.pixelSize: 10
                            elide: Text.ElideMiddle
                        }
                    }

                    Button {
                        text: qsTr("Recover")
                        enabled: modelData.valid
                        highlighted: true
                        onClicked: {
                            root.recoveryError = "";
                            if (Workspace.recoverProject(modelData.id)) {
                                root.close();
                            } else {
                                root.recoveryError = qsTr("The selected recovery snapshot could not be opened.");
                            }
                        }
                    }

                    Button {
                        text: qsTr("Discard")
                        onClicked: {
                            root.pendingDiscardId = modelData.id;
                            root.pendingDiscardName = modelData.name;
                            discardConfirmDialog.open();
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true

            Item {
                Layout.fillWidth: true
            }

            Button {
                text: qsTr("Close")
                onClicked: root.close()
            }
        }
    }

    Dialog {
        id: discardConfirmDialog

        title: qsTr("Discard recovery snapshot")
        modal: true
        anchors.centerIn: parent.contentItem
        standardButtons: Dialog.Yes | Dialog.No

        Label {
            text: qsTr("Discard the recovery snapshot for “%1”? This cannot be undone.").arg(root.pendingDiscardName)
            wrapMode: Text.WordWrap
            padding: 12
        }

        onAccepted: {
            Workspace.discardRecovery(root.pendingDiscardId);
            root.pendingDiscardId = "";
            root.pendingDiscardName = "";
        }
    }
}
