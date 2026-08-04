import "../common/SettingsHelper.js" as SettingsHelper
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ScrollView {
    id: root

    required property var draftSettings
    required property var themeValues
    required property var themeLabels

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
            title: qsTr("テーマ")
            Layout.fillWidth: true

            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 8
                anchors.fill: parent

                Label {
                    text: qsTr("カラーテーマ")
                }

                ComboBox {
                    Layout.fillWidth: true
                    model: root.themeLabels
                    currentIndex: SettingsHelper.indexOfValue(root.themeValues, root.valueOr("theme", "System"), 2)
                    onActivated: root.setValue("theme", root.themeValues[currentIndex])
                }

            }

        }

        Label {
            Layout.fillWidth: true
            text: qsTr("テーマの変更は「適用」または「OK」を押すとすぐに反映されます")
            color: palette.text
            opacity: 0.7
            wrapMode: Text.WordWrap
        }

        Item {
            Layout.fillHeight: true
        }

    }

}
