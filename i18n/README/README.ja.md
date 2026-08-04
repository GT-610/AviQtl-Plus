<p align="center">
  <img src="../../assets/splash.svg" width="256">
</p>

<p align="center"><b>AviUtlを踏襲し凌駕する次世代動画編集ソフト</b></p>

<p align="center">
  <a href="https://github.com/GT-610/AviQtl-Plus">GitHub</a>
  / <a href="https://github.com/GT-610/AviQtl-Plus/releases">リリース</a>
</p>

<p align="center">
  <a href="../../README.md">English</a> / <b>日本語</b> / <a href="./README.zh_CN.md">简体中文</a>
</p>

> [!IMPORTANT]
> AviQtl-Plusは[taisho-guy/NeoUtl](https://codeberg.org/taisho-guy/NeoUtl)のforkであり、上流プロジェクトが**Rust + Slint + wgpu**で再構築された後も**Qt Quick + QRhi + ECS**の開発路線を継続しています。詳しい経緯は[プロジェクトについて](https://aviqtl.gt610.dpdns.org/guide/start-here/about)をご覧ください。

## [AviQtl-Plus](https://github.com/GT-610/AviQtl-Plus)とは

<img src="../../assets/screenshot.webp" alt="AviQtl-Plusのスクリーンショット">

**AviUtl 1.10** & **ExEdit 0.92**の操作感を踏襲しつつ、**AviUtlを超える性能**を持つ動画編集ソフトを開発するプロジェクトです。

開発を導く編集モデルの互換目標については、[AviUtl操作感の目標](https://aviqtl.gt610.dpdns.org/developer/operability-targets)をご覧ください。

### 主な特徴

- AviUtlに酷似したUI
- GPUを使った**高速で強力なエフェクト**
- VST3やLV2等の**音声エフェクト**に対応
- **LuaJITプラグインシステム**（パッケージ管理、宣言的パラメータ、権限制御付き）
- **Linux**、**Windows**、**macOS**に対応

## ドキュメント

- [インストール](https://aviqtl.gt610.dpdns.org/guide/start-here/installation)
- [ユーザーガイド](https://aviqtl.gt610.dpdns.org/guide/start-here/)
- [ソースからのビルド](https://aviqtl.gt610.dpdns.org/developer/building)
- [開発者ドキュメント](https://aviqtl.gt610.dpdns.org/developer/)
- [よくある質問](https://aviqtl.gt610.dpdns.org/guide/help-and-reference/faq)
- [プロジェクトについて](https://aviqtl.gt610.dpdns.org/guide/start-here/about)

## エフェクトパッケージ

AviQtl-Plus はモジュラーなエフェクトシステムを備えています。`effect-packages/` ディレクトリには、拡張システムを示す即利用可能なエフェクトパッケージが含まれています：

| パッケージ | タイプ | 内容 |
|-----------|--------|------|
| [stylize-effects](../../effect-packages/stylize-effects/) | エフェクト | グリッチ、ピクセルソート、色収差、モザイク、ノイズ、エッジ、ラスター |
| [advanced-blur](../../effect-packages/advanced-blur/) | エフェクト | レンズブラー、放射ブラー、方向ブラー、モーションブラー |
| [weather-objects](../../effect-packages/weather-objects/) | オブジェクト | 雨、雪アニメーション |

これらのパッケージは、実用的な追加機能であると同時に、カスタムエフェクト作成のための開発者リファレンスとしても活用できます。詳細は [effect-packages/README.md](../../effect-packages/README.md) を参照してください。

## 関連リンク

AviQtl-Plusは、多くの素晴らしいプロジェクトの上に成り立っています。

| プロジェクト | ライセンス | 役割 |
| :--- | :--- | :--- |
| AviUtl | 非自由 | リスペクト元 |
| AviQtl | AGPLv3 | 元のQt Quick版プロジェクト；上流の`aviqtl`ブランチ |
| NeoUtl | AGPLv3 | 原作者によるRust + Slint + wgpu版 |
| AviQtl-Plus | AGPLv3 | このプロジェクト — Qt Quick + QRhi + ECS 開発の継続 |
| Carla | GPLv2+ | 音声エフェクト（VST3/LV2等）のホスト |
| FFmpeg | GPLv2+ | 動画・音声のデコード / エンコード |
| LuaJIT | MIT | 高速なスクリプトエンジン |
| Qt | GPLv3 | UI/UXフレームワーク |
| Zrythm | AGPLv3 | 音声プラグイン実装の参考 |
| Remix Icon | Remix Icon License | シンボルアイコン |

## ライセンス

AviQtl-Plusは[GNU Affero General Public License](https://www.gnu.org/licenses/agpl-3.0.txt)に基づいて公開されています。

AviQtl-Plus内で使用されている[Remix Icon](https://remixicon.com/)は[Remix Icon License](https://raw.githubusercontent.com/Remix-Design/RemixIcon/refs/heads/master/License)に基づいて提供されています。
