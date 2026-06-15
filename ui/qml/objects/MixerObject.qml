import QtQuick
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseObject {
    id: root

    property string sourcePath: String(evalParam("mixer", "source", ""))
    property string playMode: String(evalParam("mixer", "playMode", "開始時間＋再生速度"))
    property real startTime: Number(evalParam("mixer", "startTime", 0))
    property real speed: Number(evalParam("mixer", "speed", 100))
    property real directTime: Number(evalParam("mixer", "directTime", 0))
    property real volume: Number(evalParam("mixer", "volume", 1))
    property real masterVolume: Number(evalParam("mixer", "masterVolume", 1))
    property real pan: Number(evalParam("mixer", "pan", 0))
    property real fadeIn: Number(evalParam("mixer", "fadeIn", 0))
    property real fadeOut: Number(evalParam("mixer", "fadeOut", 0))
    property bool mute: evalParam("mixer", "mute", false)
    property bool solo: evalParam("mixer", "solo", false)
    property bool limiter: evalParam("mixer", "limiter", true)

    sourceItem: Item {
        width: 1
        height: 1
        visible: false
    }
}
