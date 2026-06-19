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

    function loadPermissions() {
        var perms = PermissionManager.getPluginPermissions(pluginId);
        permissions = {};
        var allPerms = [
            "transport.control", "clip.read", "clip.modify",
            "effect.read", "effect.modify", "project.read",
            "project.save", "project.load", "scene.manage",
            "settings.read", "settings.write", "clipboard.access", "log.output"
        ];
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
                model: ListModel {
                    ListElement { perm: "transport.control"; name: qsTr("再生制御"); desc: qsTr("再生、一時停止、シーク") }
                    ListElement { perm: "clip.read"; name: qsTr("クリップ読み取り"); desc: qsTr("クリップ情報の一覧表示") }
                    ListElement { perm: "clip.modify"; name: qsTr("クリップ変更"); desc: qsTr("クリップの作成、削除、移動") }
                    ListElement { perm: "effect.read"; name: qsTr("エフェクト読み取り"); desc: qsTr("エフェクト情報の一覧表示") }
                    ListElement { perm: "effect.modify"; name: qsTr("エフェクト変更"); desc: qsTr("エフェクトの追加、削除、変更") }
                    ListElement { perm: "project.read"; name: qsTr("プロジェクト読み取り"); desc: qsTr("解像度、FPS等の情報取得") }
                    ListElement { perm: "project.save"; name: qsTr("プロジェクト保存"); desc: qsTr("プロジェクトファイルの保存") }
                    ListElement { perm: "project.load"; name: qsTr("プロジェクト読み込み"); desc: qsTr("プロジェクトファイルの読み込み") }
                    ListElement { perm: "scene.manage"; name: qsTr("シーン管理"); desc: qsTr("シーンの作成、削除、切り替え") }
                    ListElement { perm: "settings.read"; name: qsTr("設定読み取り"); desc: qsTr("プラグイン設定の読み取り") }
                    ListElement { perm: "settings.write"; name: qsTr("設定書き込み"); desc: qsTr("プラグイン設定の保存") }
                    ListElement { perm: "clipboard.access"; name: qsTr("クリップボード"); desc: qsTr("コピー、切り取り、貼り付け") }
                    ListElement { perm: "log.output"; name: qsTr("ログ出力"); desc: qsTr("コンソールへのログ出力") }
                }

                delegate: ItemDelegate {
                    width: ListView.view.width
                    height: 60

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 12

                        CheckBox {
                            checked: root.permissions[model.perm] || false
                            onToggled: {
                                var p = root.permissions;
                                p[model.perm] = checked;
                                root.permissions = p;
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2

                            Label {
                                text: model.name
                                font.bold: true
                            }

                            Label {
                                text: model.desc
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
