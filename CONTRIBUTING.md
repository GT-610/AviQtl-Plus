## はじめに

AviQtlを気に入って頂けたならば、是非気軽にご参加下さい！
AviQtlには魅力的な仕事がたくさんあります。

### AviQtlの開発に最低限必要なこと
- Gitの最低限の知識
- C++、Qt、QML、GLSLのいずれかの経験 / 学習
- Linux、Windows、macOSのいずれか
- Forgejo WebUIの操作（すぐに慣れます）

### AviUtlの開発にあったら便利なこと
- FFmpeg、Carla等への深い知識
- CachyOS環境
- Fishシェル

本文では、主にAviQtl本体の開発に関する情報をまとめています。
その他の貢献方法ついては[AviQtl プロジェクトへの貢献方法](https://codeberg.org/taisho-guy/AviQtl/wiki/Contributing.ja.md)をご覧下さい。

## ライセンスへの同意

本プロジェクトへプルリクエストやその他の貢献を送信した場合、あなたは自分の貢献物を[GNU Affero General Public License](https://www.gnu.org/licenses/agpl-3.0.txt)の下で本プロジェクトに提供することに同意したものとみなします。

- あなたの貢献物の著作権はあなたに残ります。
- 受理されたコードはGNU Affero General Public Licenseの条件に従って利用・改変・再配布されます。

## コミュニケーション

バグ報告、機能提案、議論、質問等、全て[Issues](https://codeberg.org/taisho-guy/AviQtl/issues)でお受けしております。

### 良いイシューを書くために

以下の情報をイシューに含めて下さい。

- バグ報告の場合
    - OS / ディストリビューション情報（CachyOS, Windows, macOS 等）
    - 再現手順（どの操作で問題が発生したか）
    - 期待される動作と実際の動作
    - クラッシュログやスクリーンショット（可能な場合）

- 機能提案の場合
    - 機能名（仮名）
    - 機能の概要
    - 機能にどうアクセスできるようにしたいか（本体ウィンドウのメニューバー等）
    - AviUtlやその他ソフトウェアではどう実装されているか（可能な場合）

- 議論 / 質問の場合
    - 起（議題 / 概要）
    - 承（主張 / 詳細）
    - 転（考えられる反論 / 返答について言及）
    - 結（適当な挨拶）

言語は日本語か英語が好ましいですが、基本何でも構いません。

## ソースコードの構成

<details>
<summary>0.0.90現在</summary>

```text
.
├── BUILD.py
├── CMakeLists.txt
├── CONTRIBUTING.md
├── LICENSE.md
├── README.md
├── assets
│   ├── aviutl.svg
│   ├── avutl_rounded.svg
│   ├── icon.ico
│   ├── icon.png
│   ├── icon.svg
│   ├── qt.svg
│   ├── screenshot.webp
│   ├── splash.svg
│   └── windows
│       └── aviqtl.rc
├── check.py
├── core
│   ├── include
│   │   ├── audio_decoder.hpp
│   │   ├── aviqtl_context.hpp
│   │   ├── compute_effect.hpp
│   │   ├── compute_render_node.hpp
│   │   ├── document_model.hpp
│   │   ├── effect_model.hpp
│   │   ├── effect_registry.hpp
│   │   ├── ffmpeg_video_buffer.hpp
│   │   ├── image_decoder.hpp
│   │   ├── media_decoder.hpp
│   │   ├── package_manager.hpp
│   │   ├── project_serializer.hpp
│   │   ├── settings_manager.hpp
│   │   ├── theme_controller.hpp
│   │   ├── version.hpp
│   │   ├── video_decoder.hpp
│   │   ├── video_encoder.hpp
│   │   ├── video_frame_provider.hpp
│   │   └── video_frame_store.hpp
│   └── src
│       ├── audio_decoder.cpp
│       ├── compute_effect.cpp
│       ├── compute_render_node.cpp
│       ├── document_model.cpp
│       ├── effect_metadata_i18n.cpp
│       ├── effect_registry.cpp
│       ├── image_decoder.cpp
│       ├── main.cpp
│       ├── media_decoder.cpp
│       ├── package_manager.cpp
│       ├── project_serializer.cpp
│       ├── settings_manager.cpp
│       ├── theme_controller.cpp
│       ├── video_decoder.cpp
│       ├── video_encoder.cpp
│       ├── video_frame_provider.cpp
│       └── video_frame_store.cpp
├── count.py
├── engine
│   ├── audio_mixer.cpp
│   ├── audio_mixer.hpp
│   ├── plugin
│   │   ├── audio_plugin_chain.cpp
│   │   ├── audio_plugin_chain.hpp
│   │   ├── audio_plugin_host.hpp
│   │   ├── audio_plugin_manager.cpp
│   │   └── audio_plugin_manager.hpp
│   └── timeline
│       ├── bake_controller.cpp
│       ├── bake_controller.hpp
│       ├── ecs.hpp
│       ├── ecs_profiler.hpp
│       └── ecs_system.cpp
├── export.py
├── format.fish
├── i18n
│   ├── AviQtl_en_US.ts
│   ├── AviQtl_ja_JP.ts
│   └── AviQtl_zh_CN.ts
├── iconjsmaker.py
├── plugins
│   └── placeholder
├── repos
│   └── AviQtl.json
├── scripting
│   ├── lua_host.cpp
│   ├── lua_host.hpp
│   ├── mod_engine.cpp
│   └── mod_engine.hpp
├── tests
│   ├── CMakeLists.txt
│   ├── test_audio_plugin_chain.cpp
│   ├── test_bake_controller.cpp
│   ├── test_dense_component_map.cpp
│   ├── test_document_model.cpp
│   ├── test_effect_model.cpp
│   ├── test_effect_registry.cpp
│   ├── test_lua_host.cpp
│   ├── test_media_decoder.cpp
│   ├── test_project_service.cpp
│   ├── test_selection_service.cpp
│   ├── test_settings_manager.cpp
│   ├── test_theme_controller.cpp
│   └── test_transport_service.cpp
├── tree.txt
├── ui
│   ├── include
│   │   ├── bridge
│   │   │   └── core_bridge.hpp
│   │   ├── commands.hpp
│   │   ├── project_service.hpp
│   │   ├── selection_service.hpp
│   │   ├── timeline_controller.hpp
│   │   ├── timeline_export_manager.hpp
│   │   ├── timeline_media_manager.hpp
│   │   ├── timeline_service.hpp
│   │   ├── timeline_types.hpp
│   │   ├── transport_service.hpp
│   │   ├── window_manager.hpp
│   │   └── workspace.hpp
│   ├── qml
│   │   ├── AboutWindow.qml
│   │   ├── CompositeView.qml
│   │   ├── ExportDialog.qml
│   │   ├── MainWindow.qml
│   │   ├── PackageManagerWindow.qml
│   │   ├── ProjectLauncherWindow.qml
│   │   ├── ProjectSettingsWindow.qml
│   │   ├── SceneRenderer.qml
│   │   ├── SceneSettingsWindow.qml
│   │   ├── SettingDialog.qml
│   │   ├── SystemSettingsWindow.qml
│   │   ├── TimelineWindow.qml
│   │   ├── common
│   │   │   ├── AudioPluginMenu.qml
│   │   │   ├── AviQtlIcon.qml
│   │   │   ├── AviQtlWindow.qml
│   │   │   ├── BaseComputeEffect.qml
│   │   │   ├── BaseEffect.qml
│   │   │   ├── BaseObject.qml
│   │   │   ├── ControlLoader.qml
│   │   │   ├── EasingConfigWindow.qml
│   │   │   ├── IconMenuItem.qml
│   │   │   ├── Icons.js
│   │   │   ├── Logger.js
│   │   │   ├── NodeLoader.qml
│   │   │   ├── ObjectRenderer.qml
│   │   │   ├── ParamControl.qml
│   │   │   └── shaders
│   │   │       ├── blend.glsl
│   │   │       ├── color.glsl
│   │   │       ├── math.glsl
│   │   │       ├── noise.glsl
│   │   │       └── utils.glsl
│   │   ├── effects
│   │   │   ├── BorderBlur.qml
│   │   │   ├── ChromaticAberration.qml
│   │   │   ├── Clipping.qml
│   │   │   ├── ColorCorrection.qml
│   │   │   ├── DiagonalClipping.qml
│   │   │   ├── DiffuseLight.qml
│   │   │   ├── DirectionalBlur.qml
│   │   │   ├── DropShadow.qml
│   │   │   ├── ImageLoop.qml
│   │   │   ├── LensBlur.qml
│   │   │   ├── Mosaic.qml
│   │   │   ├── MotionBlur.qml
│   │   │   ├── PixelSorter.qml
│   │   │   ├── RadialBlur.qml
│   │   │   ├── Transform.qml
│   │   │   ├── Vibration.qml
│   │   │   ├── blend_layer.frag
│   │   │   ├── border_blur.json
│   │   │   ├── borderblur.frag
│   │   │   ├── chromatic_aberration.frag
│   │   │   ├── chromatic_aberration.json
│   │   │   ├── clipping.frag
│   │   │   ├── clipping.json
│   │   │   ├── color_correction.json
│   │   │   ├── colorcorrection.frag
│   │   │   ├── diagonal_clipping.frag
│   │   │   ├── diagonal_clipping.json
│   │   │   ├── diffuse_light.frag
│   │   │   ├── diffuse_light.json
│   │   │   ├── directional_blur.json
│   │   │   ├── directionalblur.frag
│   │   │   ├── drop_shadow.frag
│   │   │   ├── drop_shadow.json
│   │   │   ├── image_loop.frag
│   │   │   ├── image_loop.json
│   │   │   ├── lens_blur.json
│   │   │   ├── lensblur.frag
│   │   │   ├── mask.frag
│   │   │   ├── mosaic.frag
│   │   │   ├── mosaic.json
│   │   │   ├── motion_blur.json
│   │   │   ├── motionblur.frag
│   │   │   ├── pixelsorter.comp
│   │   │   ├── pixelsorter.json
│   │   │   ├── radial_blur.json
│   │   │   ├── radialblur.frag
│   │   │   ├── transform.frag
│   │   │   ├── transform.json
│   │   │   ├── vibration.frag
│   │   │   └── vibration.json
│   │   ├── objects
│   │   │   ├── AudioObject.json
│   │   │   ├── AudioObject.qml
│   │   │   ├── CameraControlObject.qml
│   │   │   ├── FrameBufferObject.qml
│   │   │   ├── GroupControlObject.qml
│   │   │   ├── ImageObject.qml
│   │   │   ├── RectObject.qml
│   │   │   ├── SceneObject.json
│   │   │   ├── SceneObject.qml
│   │   │   ├── TextObject.qml
│   │   │   ├── VideoObject.qml
│   │   │   ├── camera_control.json
│   │   │   ├── frame_buffer.json
│   │   │   ├── group_control.json
│   │   │   ├── image.json
│   │   │   ├── rect.json
│   │   │   ├── text.json
│   │   │   └── video.json
│   │   ├── settings
│   │   │   ├── AppearanceSettingsPage.qml
│   │   │   ├── DecodeAudioSettingsPage.qml
│   │   │   ├── ExportSettingsPage.qml
│   │   │   ├── GeneralSettingsPage.qml
│   │   │   ├── PerformanceSettingsPage.qml
│   │   │   ├── PluginSettingsPage.qml
│   │   │   ├── ProjectSettingsPage.qml
│   │   │   ├── ShortcutSettingsPage.qml
│   │   │   └── TimelineSettingsPage.qml
│   │   └── timeline
│   │       ├── ClipItem.qml
│   │       ├── LayerHeader.qml
│   │       ├── Ruler.qml
│   │       ├── TimelineGrid.qml
│   │       └── TimelineView.qml
│   ├── resources
│   │   ├── remixicon.css
│   │   └── remixicon.ttf
│   ├── resources.qrc
│   └── src
│       ├── bridge
│       │   └── core_bridge.cpp
│       ├── project_service.cpp
│       ├── selection_service.cpp
│       ├── timeline
│       │   ├── timeline_clip.cpp
│       │   ├── timeline_commands.cpp
│       │   ├── timeline_controller_clip.cpp
│       │   ├── timeline_controller_export.cpp
│       │   ├── timeline_controller_project.cpp
│       │   ├── timeline_controller_scene.cpp
│       │   ├── timeline_effect.cpp
│       │   ├── timeline_export_manager.cpp
│       │   ├── timeline_layer.cpp
│       │   ├── timeline_media_manager.cpp
│       │   └── timeline_scene.cpp
│       ├── timeline_controller.cpp
│       ├── timeline_service.cpp
│       ├── transport_service.cpp
│       ├── window_manager.cpp
│       └── workspace.cpp
├── 移行書.md
└── 仕様書.md
```

</details>


### 設計指針

- **データ指向型 (ECS):** パフォーマンスを最優先します。
- **モダン C++:** C++23 の機能を活用し、安全で効率的なコードを目指します。
- **クロスプラットフォーム:** 特定の OS API への依存を避けてください。

### 技術スタック

| カテゴリ | 採用技術 |
|----------|----------|
| 言語 | C++23, QML, JavaScript, GLSL |
| UI フレームワーク | Qt 6.11+ |
| 描画エンジン | Qt Quick 3D |
| メディア処理 | FFmpeg, stb_image |
| 音声エンジン | Carla |
| スクリプト | LuaJIT |
| ビルド & ツール | CMake, Ninja, Python 3, PySide6 (ビルドスクリプト), Clang, Mold |

## 早速初めましょう！

### 1. 準備

- [Codeberg のリポジトリ](https://codeberg.org/taisho-guy/AviQtl)をフォークします。
- フォークしたリポジトリをローカルにクローンします。

### 2. 環境構築

- `README.md` の「ビルド手順」に従ってください。
- `BUILD.py` を使ってビルドが通ることを確認してください。

AviQtlはLinux, Windows, macOSでの開発をサポートしています。

### 3. イシュー

- [Issues](https://codeberg.org/taisho-guy/AviQtl/issues)にはAviQtlに関する様々な問題が集まっています。気になるものを探して下さい。
- 気になるものが無い場合、AviQtlを試用して気になった点をイシューで報告して下さい。必ず新規作成前に、同様の報告や提案がないか確認してください。
- 特に、新機能の実装や大規模なリファクタリングを行う場合は、実装前にイシューを作成し、設計方針についてメンテナーと合意形成することを強く推奨します。
- すぐにできる小さな修正などはイシューを作成しなくても構いませんが、コンフリクトが発生する可能性が高まります。

### 4. ブランチの作成

変更内容に合わせて適切な名称のブランチを作成してください。

```bash
git switch main
git pull origin main                # 必ず行って下さい
git switch -c fix/bug-description   # バグ修正の例
git switch -c feat/feature-name     # 新機能追加の例
```

### 5. コミットとプッシュ

- 変更内容が伝わるコミットメッセージを記述してください。
- 変更を自分のフォークへプッシュします。

### 6. プルリクエストの作成

- Codeberg 上でオリジナルリポジトリに対してプルリクエストを作成します。
- 説明には、変更の理由と修正内容を簡潔に記載してください。

## お問い合わせ

些細なことでも、まずはイシューの作成からお知らせください。