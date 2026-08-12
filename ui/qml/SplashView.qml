import QtQuick

Rectangle {
    color: "transparent"

    Image {
        anchors.fill: parent
        source: "qrc:/assets/splash.svg"
        fillMode: Image.PreserveAspectFit
        asynchronous: false
    }
}
