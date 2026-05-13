// SceneRenderer: Filament の描画面を QML に埋め込むラッパー。
// Phase 1: FilamentCanvas (ヘッドレス SwapChain + QSGSimpleTextureNode blit) で
//          Wayland ネイティブ描画を実現する。

import AviQtl.Rendering 1.0
import AviQtl.UI 1.0
import QtQuick

// Phase 2: CoreBridge.currentFrame を FilamentCanvas.currentFrame に接続し、
//          タイムラインのシーク・再生がプレビューに反映されるようにする。
Item {
    id: root

    property int sceneId: -1
    // CoreBridge.currentFrame を直接バインドし、QML 側で計算なし
    property int currentFrame: CoreBridge.currentFrame
    // プロジェクト解像度 (CompositeView から伝播する)
    property int projectWidth: 1920
    property int projectHeight: 1080

    // implicitSize はプロジェクト解像度に合わせる
    // anchors.fill を使うため親が実際のウィンドウサイズを決める
    implicitWidth: projectWidth
    implicitHeight: projectHeight

    FilamentCanvas {
        anchors.fill: parent
        sceneId: root.sceneId
        currentFrame: root.currentFrame
        // プロジェクト解像度を Filament に伝える
        // Filament はこのサイズで固定レンダリングし、
        // ウィンドウリサイズは Qt SG が anchors.fill で scale する
        projectWidth: root.projectWidth
        projectHeight: root.projectHeight
    }

}
