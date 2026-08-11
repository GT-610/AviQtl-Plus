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

        GroupBox {
            title: qsTr("Preview rendering")
            Layout.fillWidth: true

            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 8
                anchors.fill: parent

                Label {
                    text: qsTr("Render scale")
                }

                ComboBox {
                    textRole: "text"
                    valueRole: "value"
                    model: [
                        { "text": "100%", "value": 1.0 },
                        { "text": "75%", "value": 0.75 },
                        { "text": "50%", "value": 0.5 },
                        { "text": "25%", "value": 0.25 }
                    ]
                    currentIndex: {
                        const value = Number(root.valueOr("previewRenderScale", 1.0));
                        if (value <= 0.25)
                            return 3;
                        if (value <= 0.5)
                            return 2;
                        if (value <= 0.75)
                            return 1;
                        return 0;
                    }
                    onActivated: root.setValue("previewRenderScale", currentValue)
                }

                Label {
                    text: qsTr("Preview antialiasing")
                }

                ComboBox {
                    textRole: "text"
                    valueRole: "value"
                    model: [
                        { "text": qsTr("Disabled"), "value": 0 },
                        { "text": "2x MSAA", "value": 2 },
                        { "text": "4x MSAA", "value": 4 },
                        { "text": "8x MSAA", "value": 8 }
                    ]
                    currentIndex: {
                        const value = Number(root.valueOr("previewMsaaSamples", 0));
                        return value === 2 ? 1 : value === 4 ? 2 : value === 8 ? 3 : 0;
                    }
                    onActivated: root.setValue("previewMsaaSamples", currentValue)
                }
            }
        }

        GroupBox {
            title: qsTr("Timeline baking")
            Layout.fillWidth: true

            GridLayout {
                columns: 2
                columnSpacing: 12
                rowSpacing: 8
                anchors.fill: parent

                Label {
                    text: qsTr("Strategy")
                }

                ComboBox {
                    textRole: "text"
                    valueRole: "value"
                    model: [
                        { "text": qsTr("On demand"), "value": "OnDemand" },
                        { "text": qsTr("Bake all clips"), "value": "FullBake" }
                    ]
                    currentIndex: root.valueOr("bakeStrategy", "OnDemand") === "FullBake" ? 1 : 0
                    onActivated: root.setValue("bakeStrategy", currentValue)
                }

                Label {
                    text: qsTr("Prefetch frames")
                }

                SpinBox {
                    from: 0
                    to: 600
                    stepSize: 10
                    value: Number(root.valueOr("onDemandPrefetchFrames", 30))
                    enabled: root.valueOr("bakeStrategy", "OnDemand") !== "FullBake"
                    onValueModified: root.setValue("onDemandPrefetchFrames", value)
                }
            }
        }

        Item {
            Layout.fillHeight: true
        }

    }

}
