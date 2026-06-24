import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseObject {
    id: root

    property real sizeW: evalNumber("color_chip", "sizeW", 200)
    property real sizeH: evalNumber("color_chip", "sizeH", 200)
    property color fillColor: evalColor("color_chip", "color", "#f472b6")
    property real cornerRadius: evalNumber("color_chip", "cornerRadius", 8)
    property real opacity: evalNumber("color_chip", "opacity", 1)
    outputModelOpacity: root.opacity

    sourceItem: sourceItem

    Item {
        id: sourceItem
        visible: false
        width: root.sizeW
        height: root.sizeH

        Rectangle {
            anchors.fill: parent
            color: root.fillColor
            radius: root.cornerRadius
        }
    }

    Common.DisplayModel {
        baseObject: root
    }
}
