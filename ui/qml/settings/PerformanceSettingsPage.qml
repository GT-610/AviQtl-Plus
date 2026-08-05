import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root

    required property var draftSettings

    signal valueChanged(string key, var value)

    function setValue(key, value) {
        valueChanged(key, value);
    }

    function valueOr(key, fb) {
        return draftSettings[key] !== undefined ? draftSettings[key] : fb;
    }


    Layout.fillWidth: true
    Layout.fillHeight: true
    contentWidth: availableWidth
    clip: true

    ColumnLayout {
        width: root.availableWidth
        spacing: 14

        GroupBox {
            title: qsTr("メモリとキャッシュ")
            Layout.fillWidth: true

            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 8
                anchors.fill: parent

                Label {
                    text: qsTr("最大画像サイズ")
                }

                SpinBox {
                    from: 1024
                    to: 16384
                    stepSize: 512
                    value: Number(root.valueOr("maxImageSize", 8192))
                    onValueModified: root.setValue("maxImageSize", value)
                }

                Label {
                    text: qsTr("キャッシュ容量")
                }

                SpinBox {
                    from: 64
                    to: 8192
                    stepSize: 64
                    value: root.valueOr("cacheSize", 512)
                    onValueModified: root.setValue("cacheSize", value)
                }

            }

        }

        Item {
            Layout.fillHeight: true
        }

    }

}
