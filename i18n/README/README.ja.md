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
> このリポジトリは[taisho-guy/AviQtl](https://codeberg.org/taisho-guy/AviQtl)のforkです。2026年5月末、原作者は開発を**リセット**することを決定しました。現在の`main`ブランチは **Qt Widgets + SDL3 + bgfx** をベースとした新しいAviQtlが置かれ、Qt Quickベースの旧バージョンは`legacy`ブランチ（**AviQtl Legacy**）に移動されました。元プロジェクトの初期コアコントリビューターであった[GT610](https://github.com/GT-610)がQt Quick路線を継続し、**AviQtl-Plus**として開発を進めます。
>
> これにより、3つのAviQtl関連プロジェクトが並行して存在することになりました：
> - **[AviQtl](https://codeberg.org/taisho-guy/AviQtl)** — 新しい技術スタックで再構築された元プロジェクト
> - **[AviQtl Legacy](https://codeberg.org/taisho-guy/AviQtl/src/branch/legacy)** — 元のQt Quick版。今后更新されません
> - **AviQtl-Plus（このプロジェクト）** — Qt Quick + QRhi + ECS アプローチを継続するfork

### 元プロジェクトの開発休止理由

原作者は以下の技術的課題を特定しました：

- Qt Quick独自のリソース管理がCompute Shaderと相性が悪く、実装が困難
- 同様に、ECSとの相性が悪く、最適化が困難
- リアルタイム処理においてパフォーマンス上の懸念がある

### AviQtl-Plusの取り組み

元プロジェクトで得られた知見を基に、AviQtl-Plusはこれらの課題に取り組みます：

1. **Compute Shaderの実現**: Qt Quickの制約を回避し、GPU Compute Shaderを活用した高速なエフェクト処理を実現する
2. **ECSアーキテクチャの最適化**: データ駆動型設計の恩恵を最大化しつつ、Qt Quickとの摩擦を解消する
3. **リアルタイム性能の向上**: プレビュー描画におけるボトルネックを排除し、滑らかな編集体験を提供する

ビジョンは変わりません：**AviUtlを踏襲し凌駕する動画編集ソフトの実現**です。Qt Quick + QRhi + ECS アプローチは実行可能で価値のある道であり、実用的な日常の編集ツールに成熟させることに注力します。

### 原作者の新しい方向性

原作者はすでに計画を実行に移しています。新しい`main`ブランチは **Qt Widgets + SDL3 + bgfx** でゼロから再構築されており、Compute Shaderを重視する将来のビジョンにはQt Quickは適さないという確信に基づいています。コア実装フェーズ中はPRを受け付けないとのことです。

旧Qt Quickソースコードは引き続き[`legacy`ブランチ](https://codeberg.org/taisho-guy/AviQtl/src/branch/legacy)で入手可能であり、原作者はQt Quickアプローチを好むユーザーに[明示的にAviQtl-Plusを推奨](https://codeberg.org/taisho-guy/AviQtl)しています。

### 新しいAviQtlとの関係

原作者と私は友好的な関係を維持しています。AviQtl-Plusの貢献が新しいAviQtlに適用可能であれば、喜んで上流に提出します。同様に、両プロジェクトの成果が時間とともに相互に還元され、最終的にはモダンでパワフルなAviUtl後継を求めるすべてのユーザーに利益をもたらすことを願っています。

## [AviQtl-Plus](https://github.com/GT-610/AviQtl-Plus)とは

<img src="../../assets/screenshot.webp">

**AviUtl 1.10** & **ExEdit 0.92**の操作感を踏襲しつつ、**AviUtlを超える性能**を持つ動画編集ソフトを開発するプロジェクトです。

### 主な特徴

- AviUtlに酷似したUI
- GPUを使った**高速で強力なエフェクト**
- VST3やLV2等の**音声エフェクト**に対応
- **LuaJITプラグインシステム**（パッケージ管理、宣言的パラメータ、権限制御付き）
- **Linux**、**Windows**、**macOS**に対応

## インストール手順

1. Linuxの場合、以下の依存関係をインストールします：
   - Qt6全般、LuaJIT、Vulkan実装（Mesa等）、FFmpeg、Carla、libc++
2. [リリースページ](https://github.com/GT-610/AviQtl-Plus/releases)からお使いのPCに最適なビルドをダウンロードします。
3. ファイルを展開し、`AviQtl` に実行権限を付与して実行します。

> [!NOTE]
> Linuxユーザーの場合、Arch Linux相当の最新環境を要求します。Ubuntu等の他のディストリビューションをご利用の方は、[Distrobox](https://distrobox.it/)でArch Linuxコンテナを作成し、その中でAviQtlを実行することを強く推奨致します。

## ビルド手順

`BUILD.py` は現在の OS からビルドターゲットを自動判定します。通常は `python BUILD.py` だけで実行できますが、手動で指定することも可能です。

共通の準備として、リポジトリをクローンしてください：

```bash
git clone https://github.com/GT-610/AviQtl-Plus.git
cd AviQtl-Plus
```

<details>
<summary>Linux</summary>

Linux では既定で distrobox/podman コンテナを使用してビルド環境を分離します。

1. **依存関係のインストール**
   - Pacman: `sudo pacman -S --needed distrobox podman python git`
   - APT: `sudo apt install distrobox podman python3 git`
   - DNF: `sudo dnf install distrobox podman python3 git`
2. **ビルド**
   - `python BUILD.py --arch`
3. **実行**
   - `./build/AviQtl`
</details>

<details>
<summary>macOS</summary>

macOS では `BUILD.py` が Homebrew 経由で CMake、Ninja、Qt6 等の依存関係を確認・インストールし、`macdeployqt` と `codesign` を実行して `.app` バンドルを作成します。

1. **依存関係のインストール**
   - `brew install python git`
2. **ビルド**
   - `python BUILD.py --xcode`
3. **実行**
   - `open ./build/AviQtl.app`
</details>

<details>
<summary>Windows (MSYS2)</summary>

1. **依存関係のインストール**
   - `pacman -S git mingw-w64-ucrt-x86_64-python`
2. **ビルド**
   - `python BUILD.py --msys2`
3. **実行**
   - `./build/AviQtl.exe`
</details>

<details>
<summary>Windows (MSVC - 非推奨)</summary>

MSVC ビルドは環境構築の複雑さから非推奨としています。

1. **追加の準備**
   - Visual Studio 2022 Build Tools の C++ ツールセット
   - 公式 Qt の MSVC x64 版（例: `msvc2022_64`）
   - vcpkg（`VCPKG_ROOT` 環境変数で指定可能。見つからない場合は `BUILD.py` が取得を試みます）
2. **ビルド**
   - `python BUILD.py --msvc --qt-dir <Qtインストールディレクトリ>`
   - `--qt-dir` を省略した場合は `QT_MSVC_DIR` 等から自動検出を試みます。
3. **実行**
   - `.\build\AviQtl.exe`
</details>

## Q & A

> [!NOTE]
> 以下のQ&Aは元の作者（taisho-guy）の個人的な見解と動機を述べたものです。

<details>
<summary>開発のきっかけは？</summary>

### OSの壁
LinuxでAviUtlが動かないことがきっかけです。**AviUtlのためだけにWindows環境を維持し続けること**は受け入れがたいものでした。

### 肥大化したエコシステム
理由は違えど、AviUtlを「仕方なく」使い続けている方は少なくないはずです。長年の拡張によって肥大化した「ハウルの動く城」のようなエコシステムは、不満を抱えながらも手放しにくい存在となっています。

### プロジェクトの目標とミッション
[鹿児島県立甲南高等学校](https://edunet002.synapse-blog.jp/konan/)の課題研究において、この課題を解決すべくAviQtlの独自開発を決意しました。

- **個人的な目標:** Domino、VocalShifter、REAPER、AviUtlをはしごすることなく、Linux上のAviQtlのみで音MADを制作すること。
- **AviQtlのミッション:** AviUtlを「仕方なく」使っている方々の最適解になること。
</details>

<details>
<summary>なぜAviUtlのクローンを開発しているのですか？</summary>

AviQtl-Plusは「AviUtlの再発明」ではありません。AviUtlを強く意識していますが、その中身は全く異なります。

| 項目 | AviQtl-Plus | ExEdit0 | ExEdit2 |
| :--- | :--- | :--- | :--- |
| 基盤技術 | Qt6 | Win32 API | Win32 API |
| 並列処理モデル | データ駆動型（ECS） | シングルスレッド | マルチスレッド |
| メモリ空間 | 64bit | 32bit (最大4GB) | 64bit |
| プレビュー描画 | Vulkan / Metal / DX12 | GDI | DX11 |
| 音声エンジン | Carla (VST3/LV2等) | 標準機能のみ | 標準機能のみ |
| プラグイン方式 | LuaJIT / C++ / QML / GLSL | Lua / C++ | LuaJIT / C++ |
| 対応OS | Linux, Windows, macOS | Windows | Windows |

AviQtl-Plusは構造的な弱点を根本的に解決します：
1. **ECS（Entity Component System）によるデータ指向:** CPUキャッシュ効率を極限まで高め、大量のオブジェクト処理を高速化。
2. **近代的なメモリ管理:** C++23のスマートポインタを採用し、原因不明のクラッシュを構造的に最小化。
3. **UIとレンダリングの分離:** 重い描画中でもタイムライン操作が妨げられず、High-DPI環境でもUIが鮮明に表示されます。
</details>

<details>
<summary>名称やアイコンの由来は？</summary>

名称は「AviUtl」と「Qt」を組み合わせた造語です。
アイコンは、QtとAviUtlのロゴを組み合わせたデザインになっています。

<p align="center">
  <img src="../../assets/qt.svg" width="64" align="middle"> + <img src="../../assets/aviutl.svg" width="64" align="middle"> = <img src="../../assets/icon.svg" width="64" align="middle">
</p>
</details>

<details>
<summary>AviUtlのプラグインは使えますか？</summary>
いいえ。仕組みが異なるため互換性は有りません。互換レイヤーを実装する予定も有りません。
</details>

### AviQtl-Plus Q&A

> [!NOTE]
> 以下のQ&AはAviQtl-Plusメンテナー（[GT610](https://github.com/GT-610)）の見解です。

<details>
<summary>なぜAviQtlの開発を継続するのですか？</summary>

元プロジェクトはQt Quickの技術的課題により休止しましたが、同時に**QtとFFmpegを使えば動画編集ソフトの高品質なプロトタイプを高速に実装できる**ことが証明されました。アーキテクチャは良く設計されており、基盤はしっかりしています。特にQRhiによってCompute Shaderへの実行可能なパスが開かれた今、Qt Quick路線にはまだ価値があると信じています。

初期のコアコントリビューターとして、このプロジェクトの可能性を間近で見てきました。原作者が残した場所から開発を引き継ぎ、単にコードを生かし続けるだけでなく、本来のビジョン——AviUtlの操作感を踏襲しつつそれを超える性能を持つ動画編集ソフトの実現——を成し遂げたいと考えています。
</details>

<details>
<summary>AviQtl-Plusのロードマップは？</summary>

**完了（0.3.0）：**
- ライフサイクルフック、宣言的パラメータ、きめ細かな権限制御を備えたLuaJITプラグインシステム
- リモートリポジトリからのプラグイン・エフェクト・オブジェクトのインストール/更新を行うパッケージマネージャー
- コア、エンジン、スクリプティング、プラグインサブシステムをカバーする24個の単体テスト
- GitHub ActionsによるCI（ビルド＋静的解析）

**次（0.3.x-0.4.0）：**
- エフェクト・オブジェクトプラグインエコシステムの拡大（サードパーティエフェクト登録API、エフェクトプラグインのサンプル）
- オーディオ編集・ミキシングの洗練（トラック別コントロール、ミキサーパネル）
- パフォーマンス最適化とGPU Compute Shaderの改善
- 自動CI/CDを伴う初期パブリックアルファリリース

**長期：**
- AviUtlの代替として十分なフル機能動画編集ソフトへ
- プロジェクトの独立した方向性を反映した改名・リブランド（名前とロゴ）を検討
- 新しいAviQtlの進捗を継続的に監視し、プロジェクト間の相互還元の機会を模索する

本プロジェクトは完全に個人のモチベーションに基づいて進められています。締切や商業的なプレッシャーはありません。着実に、しかし無理のないペースで進めていきます。
</details>

<details>
<summary>AviQtl-Plusは元のプロジェクトとどう違うのですか？</summary>

技術的な方向性はほぼ同じ（Qt Quick + QRhi + ECS）ですが、AviQtl-Plusでは以下をより重視しています：

- **インクリメンタルな成果**: アーキテクチャの完全性を追求するよりも、まず基本的で使える編集ワークフローを届ける
- **プラグマティックな問題解決**: Qt Quickの制約をブロッカーとして捉えるのではなく、その範囲内で最適解を探る
- **コミュニティへの透明性**: forkの関係性、計画、長期的な意図を明確に文書化

プロジェクトが使用可能な状態に達した後は、元のプロジェクトとの差別化を明確にするため、改名およびリブランドを行う可能性があります。
</details>

## 関連リンク

AviQtl-Plusは、多くの素晴らしいプロジェクトの上に成り立っています。

| プロジェクト | ライセンス | 役割 |
| :--- | :--- | :--- |
| AviUtl | 非自由 | リスペクト元 |
| AviQtl (Legacy) | AGPLv3 | 元のQt Quick版プロジェクト；上流の`legacy`ブランチ |
| AviQtl | AGPLv3 | 原作者によるQt Widgets + bgfx版 |
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
