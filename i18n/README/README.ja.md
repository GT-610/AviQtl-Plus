<p align="center">
  <img src="../../assets/splash.svg" width="256">
</p>

<p align="center"><b>AviUtlを踏襲し凌駕する次世代動画編集ソフト</b></p>

<p align="center">
  <a href="https://github.com/GT-610/AviQtl-Plus">GitHub</a>
  / <a href="https://codeberg.org/taisho-guy/AviQtl/releases">リリース</a>
  / <a href="https://codeberg.org/taisho-guy/AviQtl/wiki/Home">Wiki</a>
</p>

<p align="center">
  <a href="../../README.md">English</a> / <b>日本語</b> / <a href="./README.zh_CN.md">简体中文</a>
</p>

> [!IMPORTANT]
> このリポジトリは[taisho-guy/AviQtl](https://codeberg.org/taisho-guy/AviQtl)のforkです。元プロジェクトは2026年5月末に開発を休止しました。元プロジェクトの初期コアコントリビューターであった[GT610](https://github.com/GT-610)が開発を引き継ぎ、**AviQtl-Plus**として継続します。

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

ビジョンは変わりません：**AviUtlを踏襲し凌駕する動画編集ソフトの実現**です。

### 原作者の今後の計画

原作者は構想を諦めておらず、現在**Qt Widgets**と**bgfx**を学習中で、別のアーキテクチャで*AviQtl 0.1.x*として再構築する計画を進めています。ただし、受験生であるため、少なくとも2026年度以降にならないと活発な開発再開は見込めません。

### 元プロジェクトへの貢献について

今後元プロジェクトの開発が再開された場合、原作者の貢献への敬意として、上流に有用な変更を元のリポジトリに提出する予定です。

## [AviQtl-Plus](https://github.com/GT-610/AviQtl-Plus)とは

<img src="../../assets/screenshot.webp">

**AviUtl 1.10** & **ExEdit 0.92**の操作感を踏襲しつつ、**AviUtlを超える性能**を持つ動画編集ソフトを開発するプロジェクトです。

### 主な特徴

- AviUtlに酷似したUI
- GPUを使った**高速で強力なエフェクト**
- VST3やLV2等の**音声エフェクト**に対応
- **Linux**、**Windows**、**macOS**に対応

## インストール手順

1. Linuxの場合、以下の依存関係をインストールします：
   - Qt6全般、LuaJIT、Vulkan実装（Mesa等）、FFmpeg、Carla、libc++
2. [リリースページ](https://codeberg.org/taisho-guy/AviQtl/releases)からお使いのPCに最適なビルドをダウンロードします。
3. ファイルを展開し、`AviQtl` に実行権限を付与して実行します。

> [!NOTE]
> Linuxユーザーの場合、Arch Linux相当の最新環境を要求します。Ubuntu等の他のディストリビューションをご利用の方は、[Distrobox](https://distrobox.it/)でArch Linuxコンテナを作成し、その中でAviQtlを実行することを強く推奨致します。

## ビルド手順

`BUILD.py` は現在の OS からビルドターゲットを自動判定します。通常は `python BUILD.py` だけで実行できますが、手動で指定することも可能です。

共通の準備として、リポジトリをクローンし、必要に応じて仮想環境を作成してください。

```bash
git clone https://github.com/GT-610/AviQtl-Plus.git
cd AviQtl-Plus

# pipでPySide6を用意する場合（推奨）
python3 -m venv .venv
# Linux/macOS/MSYS2: source .venv/bin/activate
# Windows/PowerShell: .venv\Scripts\Activate.ps1
python -m pip install --upgrade pip PySide6
```

<details>
<summary>Linux</summary>

Linux では既定で distrobox/podman コンテナを使用してビルド環境を分離します。

1. **依存関係のインストール**
   - Pacman: `sudo pacman -S --needed distrobox podman python pyside6 git`
   - APT: `sudo apt install distrobox podman python3 python3-pyside6 git`
   - DNF: `sudo dnf install distrobox podman python3 python3-pyside6 git`
2. **ビルド**
   - `python BUILD.py --arch`
3. **実行**
   - `./build/AviQtl`
</details>

<details>
<summary>macOS</summary>

macOS では `BUILD.py` が Homebrew 経由で CMake、Ninja、Qt6 等の依存関係を確認・インストールし、`macdeployqt` と `codesign` を実行して `.app` バンドルを作成します。

1. **依存関係のインストール**
   - `brew install python pyside git`
2. **ビルド**
   - `python BUILD.py --xcode`
3. **実行**
   - `open ./build/AviQtl.app`
</details>

<details>
<summary>Windows (MSYS2)</summary>

1. **依存関係のインストール**
   - `pacman -S git mingw-w64-ucrt-x86_64-pyside6`
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

## 関連リンク

AviQtl-Plusは、多くの素晴らしいプロジェクトの上に成り立っています。

| プロジェクト | ライセンス | 役割 |
| :--- | :--- | :--- |
| AviUtl | 非自由 | リスペクト元 |
| Carla | GPLv2+ | 音声エフェクト（VST3/LV2等）のホスト |
| FFmpeg | GPLv2+ | 動画・音声のデコード / エンコード |
| LuaJIT | MIT | 高速なスクリプトエンジン |
| Qt | GPLv3 | UI/UXフレームワーク |
| Zrythm | AGPLv3 | 音声プラグイン実装の参考 |
| Remix Icon | Remix Icon License | シンボルアイコン |

## ライセンス

AviQtl-Plusは[GNU Affero General Public License](https://www.gnu.org/licenses/agpl-3.0.txt)に基づいて公開されています。

AviQtl-Plus内で使用されている[Remix Icon](https://remixicon.com/)は[Remix Icon License](https://raw.githubusercontent.com/Remix-Design/RemixIcon/refs/heads/master/License)に基づいて提供されています。
