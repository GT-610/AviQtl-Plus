import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ListView {
    id: root

    property var packages: []
    property bool showInstallButton: true

    signal permissionRequested(string pluginId, string pluginName)

    clip: true
    spacing: 8
    model: packages

    BusyIndicator {
        anchors.centerIn: parent
        running: PackageManager && PackageManager.isBusy
        visible: running
    }

    Label {
        anchors.centerIn: parent
        visible: root.count === 0 && !PackageManager.isBusy
        text: qsTr("このカテゴリにパッケージがありません。")
        color: palette.mid
    }

    delegate: Frame {
        readonly property string installedVer: modelData.installed_version || ""
        readonly property string latestVer: modelData.latest_version || ""
        readonly property bool hasUpdate: installedVer !== "" && latestVer !== "" && installedVer !== latestVer

        width: root.width
        padding: 12

        ColumnLayout {
            anchors.fill: parent
            spacing: 4

            RowLayout {
                Layout.fillWidth: true

                Label {
                    text: modelData.display_name || modelData.id
                    font.bold: true
                    font.pixelSize: 14
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                }

                Label {
                    text: modelData.version || modelData.latest_version || ""
                    font.pixelSize: 10
                    color: palette.mid
                }
            }

            Label {
                text: modelData.description || ""
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                font.pixelSize: 12
                color: palette.text
                opacity: 0.8
                visible: text !== ""
            }

            RowLayout {
                Layout.fillWidth: true

                Label {
                    text: modelData.author ? qsTr("Author: ") + modelData.author : ""
                    font.pixelSize: 11
                    color: palette.mid
                    Layout.fillWidth: true
                    visible: modelData.author !== undefined && modelData.author !== ""
                }

                Label {
                    text: qsTr("インストール済み: ") + installedVer
                    font.pixelSize: 11
                    color: "#44cc88"
                    visible: installedVer !== "" && !hasUpdate
                }

                Label {
                    text: qsTr("アップデートあり: ") + latestVer
                    font.pixelSize: 11
                    color: palette.highlight
                    visible: hasUpdate
                }

                Button {
                    text: qsTr("権限")
                    visible: installedVer !== "" && modelData.type === "mod"
                    enabled: PackageManager && !PackageManager.isBusy
                    onClicked: root.permissionRequested(modelData.id, modelData.display_name || modelData.id)
                }

                Button {
                    text: qsTr("削除")
                    visible: installedVer !== "" && modelData.id !== "org.aviqtl.app"
                    enabled: PackageManager && !PackageManager.isBusy
                    onClicked: PackageManager.removePackage(modelData.id)
                }

                Button {
                    text: hasUpdate ? qsTr("アップデート") : qsTr("インストール")
                    highlighted: true
                    enabled: PackageManager && !PackageManager.isBusy && (installedVer === "" || hasUpdate) && latestVer !== ""
                    visible: root.showInstallButton && (installedVer === "" || hasUpdate)
                    onClicked: PackageManager.installPackage(modelData.id, modelData._primary_repo || "")
                }
            }
        }
    }
}
