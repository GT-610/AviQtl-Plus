# AviQtl 0.1.0 完全仕様書

バージョン: 0.1.0-pre
作成日: 2026-05-03
目標: プレビュー画面(Filament描画)が最低限動作し、タイムラインの操作と同期する状態

---

## 第1章 アーキテクチャの根本思想

AviQtlは「データ指向設計(DOD)」と「ECS(Entity Component System)」を中核とし、
UI・正本データ・補間・描画の各責務を完全に分離した「Dual-State(二重状態)アーキテクチャ」を採用する。
いかなるレイヤーも、自分の責務外のデータを所有・変更してはならない。

---

## 第2章 レイヤー定義と責務

### Layer 1: QML (UI層)
- 責務: ユーザー入力の受付とBridgeへのコマンド発行。画面表示のみ。
- 禁止事項: データの正本を持つこと。補間の計算を行うこと。描画コマンドを発行すること。
- 接点: CoreBridge (QML_SINGLETON) のみを通じてC++と通信する。

### Layer 2: Bridge / DocumentModel (正本層)
- 責務: プロジェクトの全データ(クリップ・キーフレーム・エフェクト定義)を木構造で保持する唯一の正本。
  Undo/Redoスタック・シリアライズ(aviqtlファイルへの保存/読込)・プラグイン定義のロードを担う。
- 禁止事項: 描画処理を行うこと。補間値を自ら計算すること。
- 接点: 構造変化(クリップの追加/削除/構成変更)時のみECS WorldへBakeをトリガーする。

### Layer 3: ECS World (計算キャッシュ層)
- 責務: DocumentModelからBakeされたエンティティと、毎フレームの補間・変換計算を担う。
  データはすべてSoA(Structure of Arrays)形式のPOD(Plain Old Data)フラットバッファとして保持する。
- 禁止事項: QMLと直接通信すること。DocumentModelのデータ構造を変更すること。
- System実行順序: CommandSystem → InterpolationSystem → TransformSystem → RenderSystem

### Layer 4: Filament (描画エンジン層)
- 責務: ECSのRenderSystemから渡された行列・マテリアル・テクスチャをVulkanで描画する。
- 禁止事項: データの加工・補間・ロジックの実行。
- バックエンド: Vulkan(優先)。将来的にOpenGLへのフォールバックを検討。

---

## 第3章 データツリー構造

```
SystemSettings          [永続化: user_settings.json]
└─ ProjectSettings      [永続化: *.aviqtl]
   └─ SceneSettings[]   [複数可・ネスト可]
      └─ Layer[]
         └─ Clip[]
            ├─ [RenderBoundaryComponent付き] → FrameBuffer相当
            ├─ [GroupTransformComponent付き] → GroupControl相当
            └─ Effect[]
               └─ Keyframe[]
```

### SystemSettings
```
bakeStrategy:           "FullBake" | "OnDemand"
previewResolutionScale: float  // 0.25 / 0.5 / 1.0
gpuBackend:             "Vulkan" | "OpenGL"
maxCachedFrames:        int
theme:                  string
language:               string
shortcuts:              Map<string, string>
```

### ProjectSettings
```
name:                   string
defaultSceneWidth:      int    // 1920
defaultSceneHeight:     int    // 1080
defaultFps:             float  // 60.0
audioSampleRate:        int    // 48000
colorSpace:             "BT.709" | "BT.2020"
```

### SceneSettings
```
id:                     int
name:                   string
width:                  int     // ProjectSettingsを上書き可能
height:                 int
fps:                    float
totalFrames:            int
enableSnap:             bool
gridMode:               string
lockedLayers:           int[]
hiddenLayers:           int[]
```

### Clip
```
id:                     int
sceneId:                int
type:                   string   // "video"|"image"|"text"|"rect"|"audio"|"scene"
layer:                  int
startFrame:             int
durationFrames:         int
params:                 Map<string, any>
effects:                Effect[]
```

### Effect
```
id:                     string   // プラグインID (例: "border_blur")
enabled:                bool
params:                 Map<string, any>
keyframes:              Map<string, Keyframe[]>
```

### Keyframe
```
frame:                  int
value:                  float
interpolation: {
    type:               "linear" | "bezier" | "custom"
    cx1, cy1, cx2, cy2: float    // bezierのみ
    expression:         string   // customのみ: "1.0 - pow(1.0 - t, 3.0)"
}
```

---

## 第4章 ECSコンポーネント定義

すべてのコンポーネントはPOD (trivially_copyable) でなければならない。
ポインタ・std::string・QStringの混入を厳禁とする。

```cpp
// エンティティのアクティブ状態
struct ActiveComponent      { bool active; };

// 描画トランスフォーム (補間後の確定値)
struct TransformComponent   { float x, y, z, scaleX, scaleY, rotX, rotY, rotZ, opacity; };

// キーフレーム参照 (DocumentModel上のデータへのIDによる間接参照)
struct KeyframeRefComponent { uint32_t clipId; uint32_t effectId; };

// 描画テクスチャへの参照
struct RenderableComponent  { uint32_t textureId; uint32_t materialId; uint32_t layer; };

// フレームバッファ区切りマーカー
struct RenderBoundaryComponent { bool clearBelow; uint32_t layer; };

// グループTransform伝播マーカー
struct GroupTransformComponent { uint32_t layerCount; };

// グローバル変換行列 (TransformSystemが書き込む)
struct GlobalMatrixComponent   { float m[16]; };
```

---

## 第5章 Systemの実行順序と責務

### 1. CommandSystem (毎フレーム先頭)
- CoreBridgeのコマンドキューを消化し、ECSの状態(ActiveComponent等)を更新する。

### 2. InterpolationSystem (毎フレーム)
- KeyframeRefComponentを持つエンティティに対し、DocumentModelのキーフレーム配列と
  現在フレーム(t)を入力として補間エンジン(Interpolation Engine)を呼び出す。
- 結果をTransformComponentの各フィールドに書き込む。

### 3. TransformSystem (毎フレーム)
- GroupTransformComponentを持つエンティティから子レイヤーへTransformを伝播させる。
- TransformComponentの値からGlobalMatrixComponentを計算する。

### 4. RenderSystem (毎フレーム)
- RenderBoundaryComponentを持つエンティティをスキャンしRenderGraphのDAGを構築する。
- RenderableComponent + GlobalMatrixComponentを持つエンティティをFilamentのSceneに登録する。
- Filamentの描画コマンドを発行する。

---

## 第6章 補間エンジン (Interpolation Engine)

- DocumentModelが保持するキーフレーム配列の「生データ」を入力として受け取る独立モジュール。
- 線形(linear)・ベジェ(bezier)はC++でハードコード実装。
- カスタム補間(custom)はJSONの `expression` フィールドの数式をエンジン起動時にASTへプリコンパイルし、
  評価時のパース処理を排除して毎フレームの評価コストをほぼゼロにする。
- ECSのコンポーネントはキーフレームの概念を持たない。補間後の「確定したスカラー値」のみを受け取る。

---

## 第7章 Bake戦略

### Full-Bakeモード (SystemSettings.bakeStrategy = "FullBake")
- プロジェクトロード時に全クリップのECSエンティティを生成しメモリに常駐させる。
- シーク時はActiveComponentのフラグ切り替えのみ。プレビューシークが最速。
- 大規模プロジェクトではVRAM/RAM消費が増大する。ハイスペックPC向け。

### On-Demandモード (SystemSettings.bakeStrategy = "OnDemand")
- 再生ヘッド周辺(前後Nフレーム)のクリップのみを動的にエンティティ化・破棄する。
- メモリ消費を最小化。ローエンドPC・大規模プロジェクト向け。

### Bakeのトリガー条件
- クリップの追加・削除・レイヤー移動・エフェクトの着脱 (DocumentModelの構造変化時のみ)
- フレームごとのシークではBakeしない。

---

## 第8章 エフェクト・オブジェクトのプラグイン定義

### JSONスキーマ (オブジェクト)
```json
{
    "id":         "video",
    "name":       "動画ファイル",
    "type":       "object",
    "generator":  "VideoGenerator",
    "params":     { "file": "", "speed": 1.0 },
    "ui":         { "controls": [...] }
}
```

### JSONスキーマ (エフェクト)
```json
{
    "id":         "border_blur",
    "name":       "縁ぼかし",
    "type":       "effect",
    "shader":     "borderblur.frag",
    "params":     { "size": 10, "blur": 5.0 },
    "ui":         { "controls": [...] }
}
```

- `generator` / `shader` の実装はC++またはSPIR-Vとして本体に登録する。
- `params` と `ui.controls` はリビルドなしでJSON変更のみで拡張可能。

---

## 第9章 Video / Image / Text APIの責務

### Video API
- FFmpegによる非同期デコード
- VA-API (Linux) / DXVA2 (Windows) によるハードウェアアクセラレーション
- dmabufによるVRAMへのゼロコピー転送
- LRUキャッシュによるフレーム管理

### Image API
- stb_imageによるPNG/JPEG/WebP読み込み
- MipMap自動生成
- FilamentのTexture::Builderによる登録

### Text API
- Qt QuickのTextアイテムをオフスクリーンQQuickRenderControlで描画
- 描画結果をFilamentのTextureへ転送
- 将来的にFreeType/SDF実装へ切り替え可能 (API境界は変えない)

---

## 第10章 0.1.0 リリース要件 (最低限動作の定義)

以下が全て動作した時点でバージョン0.1.0とする。

- [ ] Filamentがウィンドウに描画できる (FilamentCanvasの初期化成功)
- [ ] タイムラインの操作(シーク・再生・停止)がCoreBridgeを通じてECSに反映される
- [ ] 動画クリップ1本がFilamentのテクスチャとしてプレビューに表示される
- [ ] 画像クリップ1枚がFilamentのテクスチャとしてプレビューに表示される
- [ ] 矩形(Rect)クリップがFilamentで描画される
- [ ] テキストクリップがQt Quick Textオフスクリーン描画 → Filamentテクスチャで表示される
- [ ] エフェクト(最低1種: border_blur)がFilamentマテリアルとして適用される
- [ ] キーフレームによるパラメータアニメーションがInterpolationSystemで動作する
- [ ] Undo/Redoが正常に動作する
- [ ] aviqtlファイルへの保存・読み込みが正常に動作する
