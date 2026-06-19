# AviQtl プラグイン開発者ガイド

このガイドでは、AviQtlプラグインの開発方法について説明します。

## 前提条件

- Lua 5.1/LuaJIT の基本知識
- テキストエディタ
- AviQtl のインストール

## プラグインの基本構造

### ディレクトリ形式（推奨）

```
my_plugin/
├── manifest.lua    # プラグインメタデータ（必須）
├── main.lua        # メインエントリポイント（必須）
├── utils.lua       # 追加モジュール（オプション）
└── README.md       # ドキュメント（オプション）
```

### 単一ファイル形式

```
plugins/
└── my_plugin.lua   # 単一ファイルですべて完結
```

## manifest.lua の作成

`manifest.lua` はプラグインのメタデータを定義します。

```lua
return {
    id = "com.yourname.pluginname",     -- 一意のID（逆ドメイン記法）
    name = "My Plugin",                 -- 表示名
    version = "1.0.0",                  -- セマンティックバージョン
    author = "Your Name",              -- 作者名
    description = "Plugin description", -- 簡単な説明
    min_app_version = "0.2.0"          -- 必要なAviQtlバージョン
}
```

### フィールドの説明

| フィールド | 必須 | 説明 |
|-----------|------|------|
| `id` | ✓ | プラグインの一意識別子。逆ドメイン記法を使用 |
| `name` | ✓ | ユーザーに表示される名前 |
| `version` | ✓ | セマンティックバージョン（x.y.z） |
| `author` | | 作者名 |
| `description` | | プラグインの説明 |
| `min_app_version` | | 必要なAviQtlの最小バージョン |

## main.lua の作成

`main.lua` はプラグインのメインコードです。

### 基本テンプレート

```lua
-- プラグインが読み込まれた時に呼ばれる
function AviQtlOnLoad()
    aviqtl.log("My Plugin loaded!")
end

-- 約16msごとに呼ばれる（60fps）
function AviQtlUpdateHook()
    -- 定期的な処理をここに記述
end

-- プロジェクトが開かれた時に呼ばれる
function AviQtlOnProjectOpen(path)
    aviqtl.log("Project opened: " .. path)
end

-- プロジェクトが保存された時に呼ばれる
function AviQtlOnProjectSave(path)
    aviqtl.log("Project saved: " .. path)
end
```

## ライフサイクルフック

以下のグローバル関数を定義すると、対応するイベントで呼び出されます。

| 関数 | 引数 | 説明 |
|------|------|------|
| `AviQtlOnLoad()` | なし | プラグイン読み込み完了時 |
| `AviQtlOnUnload()` | なし | プラグインアンロード時 |
| `AviQtlUpdateHook()` | なし | 約16msごと |
| `AviQtlOnProjectOpen(path)` | `path`: string | プロジェクト開閉時 |
| `AviQtlOnProjectSave(path)` | `path`: string | プロジェクト保存時 |
| `AviQtlOnClipChange()` | なし | クリップ変更時 |

## API の使用

### Transport Control（再生制御）

```lua
-- 再生開始
aviqtl.transport.play()

-- 一時停止
aviqtl.transport.pause()

-- 再生/一時停止の切り替え
aviqtl.transport.toggle()

-- 特定のフレームにシーク
aviqtl.transport.seek(100)

-- 現在のフレームを取得
local frame = aviqtl.transport.get_frame()

-- 再生中かどうか
local playing = aviqtl.transport.is_playing()
```

### Clip Operations（クリップ操作）

```lua
-- クリップ一覧を取得
local clips = aviqtl.clip.list()
for i, clip in ipairs(clips) do
    aviqtl.log(string.format("Clip %d: %s at layer %d", 
        clip.id, clip.type, clip.layer))
end

-- 新しいクリップを作成
aviqtl.clip.create("text", 0, 0)  -- type, startFrame, layer

-- クリップを削除
aviqtl.clip.delete(clipId)

-- クリップを更新
aviqtl.clip.update(clipId, newLayer, newStart, newDuration)

-- クリップを選択
aviqtl.clip.select(clipId)

-- クリップを分割
aviqtl.clip.split(clipId, frame)

-- クリップボード操作
aviqtl.clip.copy(clipId)
aviqtl.clip.cut(clipId)
aviqtl.clip.paste(clipId, layer)
```

### Effect Operations（エフェクト操作）

```lua
-- クリップにエフェクトを追加
aviqtl.effect.add(clipId, "blur")

-- エフェクトを削除
aviqtl.effect.remove(clipId, effectIndex)

-- エフェクトパラメータを設定
aviqtl.effect.set_param(clipId, effectIndex, "radius", 10.0)
```

### Project Information（プロジェクト情報）

```lua
-- プロジェクト情報を取得
local width = aviqtl.project.width()
local height = aviqtl.project.height()
local fps = aviqtl.project.fps()

-- プロジェクトを保存
aviqtl.project.save("/path/to/project.aviqtl")

-- プロジェクトを読み込み
aviqtl.project.load("/path/to/project.aviqtl")
```

### Scene Operations（シーン操作）

```lua
-- シーンを作成
aviqtl.scene.create("New Scene")

-- シーンを削除
aviqtl.scene.remove(sceneId)

-- シーンを切り替え
aviqtl.scene.switch(sceneId)
```

### Settings（設定）

```lua
-- 設定を保存
aviqtl.settings.set("my_key", "my_value")

-- 設定を読み込み
local value = aviqtl.settings.get("my_key")
if value == "" then
    aviqtl.settings.set("my_key", "default_value")
end
```

### Logging（ログ出力）

```lua
aviqtl.log("Hello from my plugin!")
aviqtl.log(string.format("Current frame: %d", aviqtl.transport.get_frame()))
```

## 権限の要求

プラグインが特定のAPIを使用するには、対応する権限が必要です。権限はパッケージマネージャーでユーザーが付与します。

### 権限とAPIの対応

| 権限 | 必要なAPI |
|------|----------|
| `transport.control` | `transport.*` |
| `clip.read` | `clip.list`, `clip.select` |
| `clip.modify` | `clip.create`, `clip.delete`, `clip.update`, `clip.split` |
| `effect.modify` | `effect.add`, `effect.remove`, `effect.set_param` |
| `project.read` | `project.width`, `project.height`, `project.fps` |
| `project.save` | `project.save` |
| `project.load` | `project.load` |
| `scene.manage` | `scene.*` |
| `settings.read` | `settings.get` |
| `settings.write` | `settings.set` |
| `clipboard.access` | `clip.copy`, `clip.cut`, `clip.paste` |
| `log.output` | `log` |

### エラーハンドリング

権限がないAPIを呼び出すと、Luaエラーが発生します。

```lua
-- 権限チェックの例
local ok, err = pcall(function()
    aviqtl.transport.play()
end)
if not ok then
    aviqtl.log("Permission denied: " .. err)
end
```

## ベストプラクティス

### 1. 一意なIDの使用

逆ドメイン記法を使用して、グローバルに一意なIDを設定します。

```lua
-- 良い例
id = "com.yourname.autosave"

-- 悪い例
id = "autosave"  -- 他のプラグインと衝突する可能性
```

### 2. 軽量なUpdateHook

`AviQtlUpdateHook` は高頻度で呼ばれるため、重い処理は避けてください。

```lua
-- 良い例: フレームカウンターで間引き
local frame_count = 0
function AviQtlUpdateHook()
    frame_count = frame_count + 1
    if frame_count % 60 == 0 then  -- 約1秒ごと
        -- 重い処理をここに
    end
end

-- 悪い例: 毎フレーム heavy_work()
function AviQtlUpdateHook()
    heavy_work()  -- パフォーマンスに影響
end
```

### 3. 設定の名前空間

設定キーにはプラグインIDプレフィックスを使用します。

```lua
-- 良い例
aviqtl.settings.set("com.yourname.plugin.interval", "100")

-- 悪い例
aviqtl.settings.set("interval", "100")  -- 他のプラグインと衝突
```

### 4. エラーハンドリング

エラーを適切に処理し、プラグインがクラッシュしないようにします。

```lua
function AviQtlOnLoad()
    local ok, err = pcall(function()
        -- 初期化コード
    end)
    if not ok then
        aviqtl.log("Initialization failed: " .. err)
    end
end
```

### 5. ドキュメントの提供

README.md に使用方法とAPI要件を記述します。

```markdown
# My Plugin

## 概要
このプラグインは...

## 必要な権限
- transport.control
- clip.read

## 使用方法
1. プラグインをインストール
2. 権限を付与
3. ...
```

## デバッグ

### ログの確認

AviQtlのコンソールにログが出力されます。`aviqtl.log()` を使用してデバッグ情報を出力できます。

### よくあるエラー

| エラー | 原因 | 解決方法 |
|--------|------|----------|
| `Permission denied` | 権限が不足 | パッケージマネージャーで権限を付与 |
| `controller not ready` | TimelineController未初期化 | プロジェクトを開いてから使用 |
| `syntax error` | Lua構文エラー | コードの構文を確認 |

## 配布

### リポジトリの作成

1. GitHub/Codeberg にリポジトリを作成
2. リリースを作成してZIPファイルをアップロード
3. リポジトリJSONにパッケージ情報を追加

### リポジトリJSONの形式

```json
{
    "repository_name": "My Repository",
    "packages": [
        {
            "id": "com.yourname.pluginname",
            "display_name": "My Plugin",
            "type": "mod",
            "release_feed": "https://github.com/you/plugin/releases.atom"
        }
    ]
}
```

## リファレンス

- [APIリファレンス](API_REFERENCE.md) - 完全なAPIドキュメント
- [ユーザーガイド](USER_GUIDE.md) - エンドユーザー向けガイド
- サンプルプラグイン: `plugins/example_*/`
