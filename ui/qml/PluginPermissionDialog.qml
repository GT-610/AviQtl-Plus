import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root

    property string pluginId: ""
    property string pluginName: ""

    title: qsTr("権限管理: %1").arg(pluginName)
    modal: true
    standardButtons: Dialog.Ok | Dialog.Cancel

    width: 500
    height: 600

    property var permissions: ({})
    property var allPerms: []
    property var permissionMetadata: ({
        "transport.control": { name: qsTr("再生制御"), desc: qsTr("再生、一時停止、シーク") },
        "clip.read": { name: qsTr("クリップ読み取り"), desc: qsTr("クリップ情報の一覧表示") },
        "clip.modify": { name: qsTr("クリップ変更"), desc: qsTr("クリップの作成、削除、移動") },
        "effect.modify": { name: qsTr("エフェクト変更"), desc: qsTr("エフェクトの追加、削除、変更") },
        "project.read": { name: qsTr("プロジェクト読み取り"), desc: qsTr("解像度、FPS等の情報取得") },
        "project.save": { name: qsTr("プロジェクト保存"), desc: qsTr("プロジェクトファイルの保存") },
        "project.load": { name: qsTr("プロジェクト読み込み"), desc: qsTr("プロジェクトファイルの読み込み") },
        "scene.manage": { name: qsTr("シーン管理"), desc: qsTr("シーンの作成、削除、切り替え") },
        "settings.read": { name: qsTr("設定読み取り"), desc: qsTr("プラグイン設定の読み取り") },
        "settings.write": { name: qsTr("設定書き込み"), desc: qsTr("プラグイン設定の保存") },
        "clipboard.access": { name: qsTr("クリップボード"), desc: qsTr("コピー、切り取り、貼り付け") },
        "history.control": { name: qsTr("履歴操作"), desc: qsTr("元に戻す、やり直し、コマンドのグループ化") },
        "log.output": { name: qsTr("ログ出力"), desc: qsTr("コンソールへのログ出力") }
    })

    function loadPermissions() {
        var perms = PermissionManager.getPluginPermissions(pluginId);
        permissions = {};
        allPerms = PermissionManager.getAllPermissionNames();
        for (var i = 0; i < allPerms.length; i++) {
            permissions[allPerms[i]] = perms.includes(allPerms[i]);
        }
    }

    function savePermissions() {
        PermissionManager.revokeAllPermissions(pluginId);
        for (var key in permissions) {
            if (permissions[key]) {
                PermissionManager.grantPermission(pluginId, key);
            }
        }
    }

    onAboutToShow: loadPermissions()

    onAccepted: savePermissions()

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Label {
            text: qsTr("このプラグインに許可する権限を選択してください:")
            wrapMode: Text.WordWrap
            Layout.fillWidth: true
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true

            ListView {
                model: root.allPerms

                delegate: ItemDelegate {
                    id: permissionDelegate
                    required property string modelData
                    readonly property var metadata: root.permissionMetadata[modelData] || {}
                    width: ListView.view.width
                    height: 60

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 12

                        CheckBox {
                            checked: root.permissions[permissionDelegate.modelData] || false
                            onToggled: {
                                var p = root.permissions;
                                p[permissionDelegate.modelData] = checked;
                                root.permissions = p;
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2

                            Label {
                                text: permissionDelegate.metadata.name || permissionDelegate.modelData
                                font.bold: true
                            }

                            Label {
                                text: permissionDelegate.metadata.desc || permissionDelegate.modelData
                                font.pixelSize: 11
                                opacity: 0.7
                            }
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true

            Button {
                text: qsTr("すべて許可")
                onClicked: {
                    var p = {};
                    for (var key in root.permissions) {
                        p[key] = true;
                    }
                    root.permissions = p;
                }
            }

            Button {
                text: qsTr("すべて拒否")
                onClicked: {
                    var p = {};
                    for (var key in root.permissions) {
                        p[key] = false;
                    }
                    root.permissions = p;
                }
            }

            Item { Layout.fillWidth: true }
        }
    }
}
