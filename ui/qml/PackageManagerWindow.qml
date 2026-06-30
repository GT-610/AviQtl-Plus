import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import "common" as Common

Common.AviQtlWindow {
    id: root

    property string searchQuery: ""

    title: qsTr("パッケージマネージャー")
    width: 650
    height: 450
    minimumWidth: 500
    minimumHeight: 350

    MessageDialog {
        id: selfUpdateDialog

        property string newVersion: ""
        property string downloadUrl: ""

        title: qsTr("AviQtl Plus アップデート")
        text: qsTr("新しいバージョンのAviQtl Plus (%1) が利用可能です。\nダウンロードURL: %2\n\nアプリケーションを再起動して適用してください。").arg(newVersion).arg(downloadUrl)
        buttons: MessageDialog.Ok
    }

    MessageDialog {
        id: errorDialog

        title: qsTr("パッケージマネージャーエラー")
        buttons: MessageDialog.Ok
    }

    Connections {
        function onErrorOccurred(message) {
            errorDialog.text = message;
            errorDialog.open();
        }

        function onSelfUpdateAvailable(newVersion, downloadUrl) {
            selfUpdateDialog.newVersion = newVersion;
            selfUpdateDialog.downloadUrl = downloadUrl;
            selfUpdateDialog.open();
        }

        function onAssetsReady(packageId, assets) {
            assetSelectionDialog.packageId = packageId;
            assetSelectionDialog.assets = assets;
            assetSelectionDialog.open();
        }

        target: PackageManager
    }

    Dialog {
        id: assetSelectionDialog

        property string packageId: ""
        property var assets: []

        title: qsTr("ダウンロードするファイルを選択")
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Cancel

        ListView {
            implicitWidth: 400
            implicitHeight: Math.min(300, contentHeight)
            model: assetSelectionDialog.assets
            clip: true

            delegate: ItemDelegate {
                width: parent.width
                text: modelData.name + " (" + (modelData.size / 1024 / 1024).toFixed(2) + " MB)"
                onClicked: {
                    PackageManager.installPackage(assetSelectionDialog.packageId, modelData.url);
                    assetSelectionDialog.close();
                }
            }
        }
    }

    function packageTypeForTab(index) {
        var types = ["effect", "object", "transition", "mod", "installed", "application"];
        if (index < types.length)
            return types[index];
        return "";
    }

    function filteredPackages(tabIndex) {
        if (!PackageManager) return [];
        var tabType = packageTypeForTab(tabIndex);
        if (tabType === "") return [];
        var _ = PackageManager.packageList;
        var all;
        if (tabType === "installed")
            all = PackageManager.getPackagesByType("installed");
        else
            all = PackageManager.getPackagesByType(tabType);
        if (root.searchQuery === "")
            return all;
        var result = [];
        for (var i = 0; i < all.length; i++) {
            var p = all[i];
            var name = p.display_name || "";
            var id = p.id || "";
            if (name.toLowerCase().indexOf(root.searchQuery.toLowerCase()) !== -1 ||
                id.toLowerCase().indexOf(root.searchQuery.toLowerCase()) !== -1)
                result.push(p);
        }
        return result;
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 0

        TabBar {
            id: tabBar

            Layout.fillWidth: true
            Layout.bottomMargin: 12
            currentIndex: 0

            onCurrentIndexChanged: {
                root.searchQuery = "";
                searchField.text = "";
            }

            TabButton { text: qsTr("エフェクト") }
            TabButton { text: qsTr("オブジェクト") }
            TabButton { text: qsTr("トランジション") }
            TabButton { text: "MOD" }
            TabButton { text: qsTr("インストール済み") }
            TabButton { text: qsTr("アプリケーション") }
            TabButton { text: qsTr("リポジトリ") }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.bottomMargin: 8
            spacing: 8
            visible: tabBar.currentIndex < 6

            TextField {
                id: searchField
                Layout.fillWidth: true
                placeholderText: qsTr("検索...")
                onTextChanged: root.searchQuery = text
            }

            Button {
                text: qsTr("リポジトリを同期")
                icon.name: "refresh-line"
                enabled: PackageManager && !PackageManager.isBusy
                onClicked: PackageManager.refreshRepositories()
                visible: tabBar.currentIndex < 4
            }
        }

        StackLayout {
            currentIndex: tabBar.currentIndex
            Layout.fillWidth: true
            Layout.fillHeight: true

            // Page 0: Effect
            Common.PackageListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                packages: root.filteredPackages(0)
                onPermissionRequested: (id, name) => { permissionDialog.pluginId = id; permissionDialog.pluginName = name; permissionDialog.open(); }
            }

            // Page 1: Object
            Common.PackageListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                packages: root.filteredPackages(1)
                onPermissionRequested: (id, name) => { permissionDialog.pluginId = id; permissionDialog.pluginName = name; permissionDialog.open(); }
            }

            // Page 2: Transition
            Common.PackageListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                packages: root.filteredPackages(2)
                onPermissionRequested: (id, name) => { permissionDialog.pluginId = id; permissionDialog.pluginName = name; permissionDialog.open(); }
            }

            // Page 3: MOD
            Common.PackageListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                packages: root.filteredPackages(3)
                onPermissionRequested: (id, name) => { permissionDialog.pluginId = id; permissionDialog.pluginName = name; permissionDialog.open(); }
            }

            // Page 4: Installed
            Common.PackageListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                packages: root.filteredPackages(4)
                showInstallButton: false
                onPermissionRequested: (id, name) => { permissionDialog.pluginId = id; permissionDialog.pluginName = name; permissionDialog.open(); }
            }

            // Page 5: Application
            Common.PackageListView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                packages: root.filteredPackages(5)
                showInstallButton: false
            }

            // Page 6: Repository Management
            ColumnLayout {
                spacing: 12

                GroupBox {
                    title: qsTr("リポジトリを追加")
                    Layout.fillWidth: true

                    RowLayout {
                        anchors.fill: parent

                        TextField {
                            id: repoUrlField
                            Layout.fillWidth: true
                            placeholderText: "https://example.com/repo.json"
                            selectByMouse: true
                            onAccepted: addRepoBtn.clicked()
                        }

                        Button {
                            id: addRepoBtn
                            text: qsTr("追加")
                            enabled: repoUrlField.text.length > 0
                            onClicked: {
                                PackageManager.addRepository(repoUrlField.text);
                                repoUrlField.text = "";
                            }
                        }
                    }
                }

                GroupBox {
                    title: qsTr("設定済みリポジトリ")
                    Layout.fillWidth: true
                    Layout.fillHeight: true

                    ListView {
                        anchors.fill: parent
                        clip: true
                        model: PackageManager ? PackageManager.repositories : []

                        delegate: ItemDelegate {
                            width: parent.width
                            height: 48
                            padding: 0

                            contentItem: RowLayout {
                                spacing: 8

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Label {
                                        text: modelData.name || modelData.url
                                        font.bold: true
                                        font.pixelSize: 13
                                        Layout.fillWidth: true
                                        elide: Text.ElideRight
                                    }

                                    Label {
                                        text: modelData.url
                                        font.pixelSize: 11
                                        color: palette.mid
                                        Layout.fillWidth: true
                                        elide: Text.ElideRight
                                    }
                                }

                                Switch {
                                    checked: modelData.enabled !== false
                                    onToggled: PackageManager.setRepositoryEnabled(modelData.url, checked)
                                }

                                Button {
                                    flat: true
                                    Layout.preferredWidth: 32
                                    Layout.fillHeight: true
                                    onClicked: PackageManager.removeRepository(modelData.url)

                                    contentItem: Common.AviQtlIcon {
                                        iconName: "delete_bin_line"
                                        size: 14
                                        color: parent.hovered ? "red" : palette.text
                                    }
                                }
                            }
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true

                    Button {
                        text: qsTr("リポジトリを同期")
                        icon.name: "refresh-line"
                        enabled: PackageManager && !PackageManager.isBusy
                        onClicked: PackageManager.refreshRepositories()
                    }

                    Button {
                        text: qsTr("すべてアップグレード")
                        icon.name: "upload-cloud-line"
                        highlighted: true
                        enabled: PackageManager && !PackageManager.isBusy && PackageManager.hasUpdatesAvailable
                        onClicked: PackageManager.upgradeAllPackages()
                    }
                }
            }
        }

        ProgressBar {
            Layout.fillWidth: true
            Layout.topMargin: 8
            visible: PackageManager && PackageManager.isBusy
            value: PackageManager ? PackageManager.progress : 0
        }
    }

    PluginPermissionDialog {
        id: permissionDialog
        anchors.centerIn: parent
    }
}
