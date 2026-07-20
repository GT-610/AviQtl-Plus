pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: root

    property var controller: null
    property var allowedKinds: ["object", "effect", "transition"]
    property string currentKind: "object"
    property string searchText: ""
    property string selectedCategory: ""
    property var categoryValues: [""]
    property var catalogItems: []
    property var selectedItem: null

    signal itemChosen(var item)

    modal: true
    title: qsTr("Object and Effect Catalog")
    standardButtons: Dialog.NoButton
    closePolicy: Popup.CloseOnEscape
    width: Math.min(760, parent ? parent.width - 40 : 760)
    height: Math.min(560, parent ? parent.height - 40 : 560)
    anchors.centerIn: parent

    function kindLabel(kind) {
        if (kind === "object")
            return qsTr("Objects");
        if (kind === "effect")
            return qsTr("Effects");
        if (kind === "transition")
            return qsTr("Transitions");
        return kind;
    }

    function rebuildCategories() {
        var values = [""];
        if (controller) {
            var categories = controller.getCatalogCategories(currentKind) || [];
            for (var i = 0; i < categories.length; ++i)
                values.push(categories[i]);
        }
        categoryValues = values;
        selectedCategory = "";
        categoryCombo.currentIndex = 0;
    }

    function refresh() {
        selectedItem = null;
        itemList.currentIndex = -1;
        catalogItems = controller ? (controller.queryCatalog(currentKind, searchText, selectedCategory) || []) : [];
    }

    function openForKinds(kinds, initialKind) {
        allowedKinds = kinds && kinds.length > 0 ? kinds : ["object", "effect", "transition"];
        var newKind = allowedKinds.indexOf(initialKind) >= 0 ? initialKind : allowedKinds[0];
        var kindChanged = newKind !== currentKind;
        currentKind = newKind;
        searchText = "";
        searchField.text = "";
        if (!kindChanged) {
            rebuildCategories();
            refresh();
        }
        open();
        Qt.callLater(function() { searchField.forceActiveFocus(); });
    }

    function acceptSelection() {
        if (!selectedItem)
            return;
        itemChosen(selectedItem);
        close();
    }

    onCurrentKindChanged: {
        rebuildCategories();
        refresh();
    }
    onSelectedCategoryChanged: refresh()

    contentItem: ColumnLayout {
        spacing: 10

        TabBar {
            id: kindTabs
            Layout.fillWidth: true
            visible: root.allowedKinds.length > 1
            onCurrentIndexChanged: {
                if (currentIndex >= 0 && currentIndex < root.allowedKinds.length)
                    root.currentKind = root.allowedKinds[currentIndex];
            }

            Repeater {
                model: root.allowedKinds

                TabButton {
                    required property string modelData
                    text: root.kindLabel(modelData)
                }
            }

            function syncToCurrentKind() {
                var idx = root.allowedKinds.indexOf(root.currentKind);
                if (idx >= 0)
                    kindTabs.currentIndex = idx;
            }

            Connections {
                target: root
                function onCurrentKindChanged() { kindTabs.syncToCurrentKind(); }
                function onAllowedKindsChanged() { kindTabs.syncToCurrentKind(); }
            }

            Component.onCompleted: syncToCurrentKind()
        }

        RowLayout {
            Layout.fillWidth: true

            TextField {
                id: searchField
                Layout.fillWidth: true
                placeholderText: qsTr("Search by name, category, ID, or package...")
                onTextChanged: {
                    root.searchText = text;
                    searchRefreshTimer.restart();
                }

                Timer {
                    id: searchRefreshTimer
                    interval: 250
                    repeat: false
                    onTriggered: root.refresh()
                }
            }

            ComboBox {
                id: categoryCombo
                Layout.preferredWidth: 220
                model: root.categoryValues.map(function(value) { return value === "" ? qsTr("All categories") : value; })
                onActivated: root.selectedCategory = root.categoryValues[currentIndex]
            }
        }

        Label {
            Layout.fillWidth: true
            text: qsTr("%1 items").arg(root.catalogItems.length)
            opacity: 0.7
        }

        ListView {
            id: itemList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.catalogItems
            spacing: 4

            delegate: Rectangle {
                id: itemDelegate
                required property int index
                required property var modelData
                width: itemList.width
                height: 72
                radius: 6
                color: ListView.isCurrentItem ? root.palette.highlight : (itemMouse.containsMouse ? root.palette.alternateBase : root.palette.base)
                border.width: 1
                border.color: ListView.isCurrentItem ? root.palette.highlight : root.palette.mid

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 3

                    RowLayout {
                        Layout.fillWidth: true

                        Label {
                            Layout.fillWidth: true
                            text: itemDelegate.modelData.name || itemDelegate.modelData.id
                            font.bold: true
                            elide: Text.ElideRight
                        }

                        Label {
                            text: itemDelegate.modelData.source === "built-in" ? qsTr("Built-in") : (itemDelegate.modelData.packageId || itemDelegate.modelData.source)
                            opacity: 0.75
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        text: (itemDelegate.modelData.categories || []).join("  •  ")
                        opacity: 0.75
                        elide: Text.ElideRight
                    }

                    Label {
                        Layout.fillWidth: true
                        text: itemDelegate.modelData.id + (itemDelegate.modelData.version ? "  ·  v" + itemDelegate.modelData.version : "")
                        opacity: 0.55
                        elide: Text.ElideMiddle
                    }
                }

                MouseArea {
                    id: itemMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: {
                        itemList.currentIndex = itemDelegate.index;
                        root.selectedItem = itemDelegate.modelData;
                    }
                    onDoubleClicked: {
                        itemList.currentIndex = itemDelegate.index;
                        root.selectedItem = itemDelegate.modelData;
                        root.acceptSelection();
                    }
                }
            }

            Label {
                anchors.centerIn: parent
                visible: root.catalogItems.length === 0
                text: qsTr("No matching catalog items")
                opacity: 0.7
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            Button {
                text: qsTr("Cancel")
                onClicked: root.close()
            }
            Button {
                text: qsTr("Add")
                enabled: root.selectedItem !== null
                highlighted: true
                onClicked: root.acceptSelection()
            }
        }
    }
}
