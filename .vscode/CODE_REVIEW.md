# AviQtl-Plus 代码质量审查报告

> 审查日期: 2026-06-30
> 审查范围: core/、engine/、ui/、scripting/、构建脚本与辅助工具
> 方法: 静态分析 + 代码阅读，未修改任何文件

---

## 目录

- [一、Core 模块](#一core-模块)
- [二、Engine 模块](#二engine-模块)
- [三、UI/QML/Scripting 模块](#三uiqmlscripting-模块)
- [四、构建脚本与辅助工具](#四构建脚本与辅助工具)
- [五、全局汇总 Top 10](#五全局汇总-top-10)

---

## 一、Core 模块

### 1.1 安全/正确性/崩溃风险

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P0** | `core/src/audio_decoder.cpp` | 229 | `getSamples` 返回 `std::vector<float>` 拷贝 | 每次调用都堆分配整个音频片段。被 `AudioMixer::mix` 在每个音频帧调用，是实时音频路径上的热路径。建议改为返回 `std::span<const float>` 视图，或接受输出缓冲区参数。 |
| **P0** | `core/src/audio_decoder.cpp` | 112-169 | `startDecoding` 全量解码整个文件到内存 | 一次性读取整个音频文件并解码到 `m_fullAudioData`。1 小时 48kHz 立体声 ≈ 1.3 GB。建议采用流式/分块解码，按需加载。 |
| **P0** ✅ | `core/src/settings_manager.cpp` | 258-270 | `save` 非原子写入 | 直接 `file.open(WriteOnly)` + `file.write()`，崩溃/断电时文件截断丢失所有设置。建议先写临时文件再 `QFile::rename`。 |
| **P0** ✅ | `core/src/project_serializer.cpp` | 150-268 | `load` 无输入校验 | `width`/`height`/`fps`/`layer` 无范围校验，未知版本不拒绝。建议增加字段范围校验（`width/height > 0`，`fps > 0`）。 |
| **P1** ✅ | `core/src/video_encoder.cpp` | 80-147 | `availableVideoEncoders` 非线程安全缓存 | `static QStringList cachedResult` + `static bool initialized` 不是原子操作。建议改用 `std::call_once`。 |

### 1.2 性能/内存

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P0** | `core/src/audio_decoder.cpp` | 219-225 | `setSampleRate` 触发全量重解码 | 切换采样率会完全重新解码整个文件，即使只需重采样。建议保留原始数据仅更新 swr 上下文。 |
| **P1** | `core/src/video_frame_store.cpp` | 11-13 | `VideoFrameStore` 无界内存增长 | `m_frames` 和 `m_lastVideoFrames` 只增不减，长时间编辑内存持续增长。建议改用 LRU 缓存。 |
| **P1** | `core/src/video_decoder.cpp` | 335-371 | `buildIndex` 全量扫描整个容器 | 对整个文件做 `av_read_frame` 遍历构建帧索引，大文件需数秒到数十秒。建议使用 FFmpeg 索引 API 或缓存索引到磁盘。 |
| **P1** ✅ | `core/src/image_decoder.cpp` | 118-134 | `decodeImage` 清理代码重复 5 次 | 每次错误分支都手动 free。建议使用 `qScopeGuard` 或 RAII wrapper。 |

### 1.3 冗余/死代码

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P2** | `core/src/video_decoder.cpp` | 243-248 | `goto hwinitdone` | 使用 goto 跳出双重循环。建议改用 `std::optional` 返回或 helper 函数。 |
| **P2** | `core/src/main.cpp` | 43 | `ffmpeg_log_callback` 固定 1024 字节缓冲区 | 长日志被截断。建议使用 `vsnprintf` 返回值动态分配。 |
| **P2** | `core/src/effect_registry.cpp` | 173-174 | 每次扫描都做 `canonicalPath()` | 磁盘 I/O 操作，大量效果文件时很慢。建议仅在路径含 `..` 时做规范化。 |
| **P2** | `core/src/package_manager.cpp` | 1055-1058, 1134-1136 | 路径安全检查重复 3 处 | 相同的 `packageId` 校验逻辑。建议提取为 `isValidPackageId()` 函数。 |
| **P2** | `core/src/package_manager.cpp` | 1015-1051 | `extractZip` 依赖外部 unzip/powershell | 跨平台不一致。建议引入 Qt 的 `QZipReader` 或 minizip。 |
| **P2** | `core/src/video_frame_store.cpp` | 23-24 | `setVideoFrameSafe` lambda 按值捕获 QVideoFrame | 每次跨线程调用都拷贝一帧（可能几十 MB）。建议 `std::move`。 |
| **P2** | `core/src/video_encoder.cpp` | 656-733, 819-886 | `processVideo`/`processAudio` 代码大量重复 | drain + write 逻辑几乎相同。建议提取公共函数。 |

### 1.4 测试覆盖缺口

| 文件 | 缺失测试 |
|------|----------|
| `core/src/audio_decoder.cpp` | `getSamples`/`getPeaks` 无单元测试 |
| `core/src/video_decoder.cpp` | GOP 缓存、硬件回退逻辑无测试 |
| `core/src/project_serializer.cpp` | 异常输入处理（畸形 JSON/缺失字段/超大数值）无测试 |
| `core/src/settings_manager.cpp` | `save/load` 原子性无测试 |

---

## 二、Engine 模块

### 2.1 安全/正确性

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P0** | `engine/audio_mixer.cpp` | 292 | `mix()` 返回引用指向成员 buffer | UI/音频线程和导出线程并发调用时数据竞争。建议返回拷贝或 triple-buffer。 |
| **P0** | `engine/audio_mixer.cpp` | 54 | `m_decoders` 存储裸指针 `AudioDecoder*` | decoder 被 UI 删除后未 unregister 会悬空。建议用 `QPointer`。 |
| **P0** | `engine/audio_mixer.cpp` | 165 | `fps <= 0` 未统一保护 | `relTime` 用 `currentFrame/fps`，fps 为 0 时除零。建议统一校验。 |
| **P0** | `engine/plugin/audio_plugin_manager.cpp` | 278-306 | `getParamInfo`/`setParam` 固定 [0,1] 范围 | 忽略插件真实参数范围。建议读取 `CarlaParameterInfo` 的真实 min/max。 |
| **P1** | `engine/audio_mixer.cpp` | 124 | `mix()` 持 `unique_lock` 访问 QHash | 实时音频线程应避免锁竞争。建议拆分播放状态到无锁/ringbuffer。 |
| **P1** | `engine/audio_mixer.cpp` | 273-294 | `processFrame` 中 `m_lastFrame` 未加锁 | 与 `reset()` 并发可能 torn write。 |
| **P1** | `engine/timeline/ecs.hpp` | 25-37 | `operator[]` 对负 clipId 不安全 | `ensureSparseSize(clipId)` 返回后仍用负 clipId 转 `size_t` 访问。建议 assert 或抛出。 |
| **P1** | `engine/timeline/bake_controller.cpp` | 145-169 | `paramName` UTF-8 截断边界 | 20 字节 `char[20]` 可能截断多字节字符中间。建议用 `QString::left` 按字符截断。 |
| **P1** | `engine/timeline/bake_controller.cpp` | 174 | `render.effectCount = effectIdx` 含 disabled 效果 | 语义上应表示实际生效数量。 |

### 2.2 性能

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P1** | `engine/audio_mixer.cpp` | 186 | `AudioDecoder::getSamples` 每次返回 vector 拷贝 | 实时音频路径每次堆分配。 |
| **P1** | `engine/plugin/audio_plugin_manager.cpp` | 222-232 | `CarlaHostedPlugin::process` 每次 deinterleave | 固定开销，可预分配复用 buffer。 |
| **P1** | `engine/plugin/audio_plugin_manager.cpp` | 640-653 | 插件扫描每次新建 `QThreadPool` | 低频操作但可复用 `QThreadPool::globalInstance()`。 |
| **P1** | `engine/audio_mixer.cpp` | 189 | `m_clipSamples.resize` 导致内存震荡 | 与 `m_rawSamples` 交替 swap，建议预分配最大块。 |
| **P1** | `engine/audio_mixer.cpp` | 288-289 | `outputSamples` 计算可能溢出 int | `m_playbackSpeed` 极小时溢出。建议 `std::clamp`。 |
| **P2** | `engine/timeline/bake_controller.cpp` | 132-141 | transform effect 字段求值高度重复 | 8 个字段都是 `evalFloat(rt, key, relFrame)`。建议反射表循环。 |
| **P2** | `engine/timeline/bake_controller.cpp` | 176-186 | `hasTransform` 为 false 时重复默认赋值 | 与 `RenderComponent` 默认构造函数重复。 |
| **P2** | `engine/plugin/audio_plugin_manager.cpp` | 388-421 | `normalizeCategoryTitle` 与 `toCategoryStr` 逻辑重复 | 两份类别映射表，容易漂移。 |

### 2.3 冗余/死代码

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P2** | `engine/timeline/keyframe_evaluator.cpp` | 8-35 | 整个文件是 Core::KeyframeUtils 的转发 wrapper | 无新语义，建议删除或直接用 `keyframe_utils.hpp`。 |
| **P2** | `engine/timeline/ecs.hpp` | 190-191 | `isRenderGraphDirty`/`markRenderGraphClean` 仅测试用 | dead API，建议移除或真正接入渲染桥。 |
| **P2** | `engine/timeline/ecs.hpp` | 148 | `ParamType` 枚举只用到 Float 和 Color | `Vec2/Vec3/Vec4` 无读写路径。 |
| **P2** | `engine/timeline/ecs_profiler.hpp` | 42-44 | `ECS_TIMER_SCOPE` 宏定义但未使用 | 0 处调用。 |
| **P2** | `engine/timeline/ecs_system.cpp` | 154-160 | `m_lastAckedGeneration` 仅被 dead API 使用 | 配套状态。 |
| **P2** | `engine/audio_mixer.cpp` | 127-130 | 零填充分支重复 | `assign(newSize, 0.0F)` 与 `std::fill(..., 0.0F)` 可统一。 |
| **P2** | `engine/audio_mixer.cpp` | 235-258 | 左右声道处理完全镜像 | 可写成 lambda `processChannel(i, vol)` 或循环模板。 |
| **P2** | `engine/plugin/audio_plugin_manager.cpp` | 70-78 | `ensureBuffers` 对四个 vector 重复 resize | 可模板化或用 `std::array`。 |
| **P2** | `engine/plugin/audio_plugin_manager.cpp` | 103-109 | Carla host callback 空实现 stub | 应注释说明 "TODO or intentionally disabled"。 |
| **P2** | `engine/audio_mixer.hpp` | 54 | `std::unordered_map` 与 `m_chains` 的 `QHash` 混用 | 统一容器类型减少模板实例化。 |

### 2.4 C++23 可利用点

| 文件 | 建议 |
|------|------|
| `engine/audio_mixer.cpp:124` | `mix()` 返回 `std::span<const float>` 避免依赖容器类型 |
| `engine/audio_mixer.cpp:140,150` | `for` 循环可改 `std::views::filter` + `std::ranges::for_each` |
| `engine/timeline/ecs.hpp:80-85` | 为 `DenseComponentMap` 加 `std::ranges` 兼容迭代器 |
| `engine/plugin/audio_plugin_manager.cpp:805` | `createPlugin` 可用 `std::expected` 表达失败原因 |
| `engine/timeline/bake_controller.cpp:200` | `std::find_if` → `std::ranges::find_if` |

### 2.5 测试覆盖缺口

| 缺失测试 |
|----------|
| `AudioMixer` 无任何单元测试 |
| `AudioPluginManager` / `CarlaHostedPlugin` 无单元测试 |
| `ECS::syncClipIds` 与 `commit` 并发正确性无压力测试 |
| `test_keyframe_evaluator.cpp` 只测了 wrapper 转发，实际价值有限 |

---

## 三、UI/QML/Scripting 模块

### 3.1 安全/正确性

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P0** ✅ | `ui/src/timeline/timeline_scene.cpp` | 22-37 | 返回 `thread_local dummy` | 空场景时返回可修改的 dummy，数据竞争。建议返回 `nullptr`。 |
| **P0** ✅ | `ui/src/timeline/timeline_layer.cpp` | 32 | `setLayerStateInternal` 误发 `clipsChanged()` | 隐藏/锁定层触发 bake/decoder 重建。建议单独发 `layerStateChanged`。 |
| **P0** | `ui/src/selection_service.cpp` | 42-73 | `toggleSelection` 主选数据不一致 | 取消主选时 `m_selectedClipData` 未更新。 |
| **P0** | `ui/qml/MainWindow.qml` | 907-919 | `isScrubbing` 属性未定义 | `TransportService` 没有该属性，QML 动态创建但无逻辑。 |
| **P0** ✅ | `scripting/mod_engine.cpp` | 47-95 | `checkPermission` 默认 allow unknown API | 新 API 忘记加入映射会被默认允许。建议 deny-by-default。 |
| **P0** ✅ | `scripting/mod_engine.cpp` | 13 | `g_ctrl` 全局裸指针暴露给 Lua | TimelineController 销毁后 dangling。建议用 `QPointer`。 |
| **P0** | `scripting/mod_engine.cpp` | 712-763 | `loadManifest` 直接执行 `manifest.lua` | 即使沙箱也允许执行任意 Lua。建议 manifest 使用静态解析。 |
| **P1** | `ui/src/timeline/timeline_controller_clip.cpp` | 523-556 | `applySelectionIds` 仅读取第一个 clip 的 effect params | 多选时 UI 始终显示第一个 clip 属性。 |
| **P1** | `ui/src/timeline_controller.cpp` | 212-217 | `hasUnsavedChanges` 未加锁 | 导出线程可能并发修改文档状态。 |
| **P1** | `scripting/mod_engine.cpp` | 27-44 | `g_settings_buf` 全局静态缓冲区 | 线程不安全，建议 `thread_local` 或返回 `QByteArray`。 |
| **P1** | `scripting/mod_engine.cpp` | 817-836 | `onPluginDirectoryChanged` 全量重载 | 频繁文件变化导致反复执行插件加载。建议增加防抖。 |
| **P1** | `ui/src/window_manager.cpp` | 139-161 | `spawnWindow` obj 不是 QQuickWindow 时未处理 nullptr | 空窗口。 |
| **P1** | `ui/qml/common/ControlLoader.qml` | 519-546 | `FileDialog` 路径直接写入设置 | 无白名单/沙箱校验。 |
| **P1** | `ui/qml/timeline/TimelineView.qml` | 505-519 | DropArea 直接导入 URL | 需确认 C++ 端是否校验 scheme。 |
| **P2** | `ui/src/timeline/timeline_scene.cpp` | 137-161 | `updateSceneSettings` 参数列表 15+ 个 | NOLINT 压不住。建议改为传入结构体。 |
| **P2** | `ui/src/timeline/timeline_controller_project.cpp` | 215-224 | `getAvailableObjects` 硬编码对象分类 | 新增对象需同时改 registry 和这里。 |
| **P2** | `ui/src/timeline/timeline_media_manager.hpp` | 55 | `m_imageDecoders` 似乎未使用 | 确认是否 dead code。 |
| **P2** | `ui/src/timeline/timeline_clip.cpp` | 948, 963 | 使用 `2147483647` 作为哨兵值 | 应使用 `std::numeric_limits<int>::max()`。 |

### 3.2 性能

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P1** | `ui/src/timeline/timeline_controller_clip.cpp` | 875-982 | `getWaveformPeaks` 逐像素调用 `evaluatedParam` | 1920px 宽度 = 数万次调用。建议采样步长或缓存。 |
| **P1** | `ui/qml/timeline/TimelineView.qml` | 302-372 | Repeater 为每个 clip 创建完整 ClipItem | 1000+ clip 时内存和初始化开销巨大。建议 ListView + cacheBuffer。 |
| **P1** | `ui/src/timeline/timeline_clip.cpp` | 666-702 | `findClipById` 线性搜索被调用上百次 | 应维护 `QHash<int, ClipData*>` 索引。 |
| **P1** | `ui/src/timeline/timeline_export_manager.cpp` | 124-164, 266-306 | 导出线程 `BlockingQueuedConnection` + 固定 8ms sleep | 主线程阻塞 + 浪费时间。建议离屏 FBO + 信号驱动。 |
| **P1** | `ui/src/timeline/timeline_controller_clip.cpp` | 1096-1153 | `audioPluginEvaluatedParam` 逐关键帧线性插值 | 高频调用时成为瓶颈。 |
| **P1** | `ui/qml/timeline/TimelineView.qml` | 182-239 | Timer 16ms 在 GUI 线程跑大量逻辑 | 播放时频繁读写 contentX。建议改用 Animation。 |
| **P2** | `ui/qml/CompositeView.qml` | 124-132 | `offscreenRenderHost` 尺寸固定 8192 | 即使项目分辨率 1920×1080。建议按项目分辨率设置。 |
| **P2** | 多处 QML | - | `ShaderEffectSource.live: true` 大量启用 | 静态效果也每帧重新抓取纹理。建议静态效果设 `live: false`。 |
| **P2** | `ui/src/timeline/timeline_clip.cpp` | 880-926 | `findVacantFrame` 在拖拽循环中被反复调用 | 每次迭代 O(n)，可用空间索引。 |
| **P2** | `ui/qml/timeline/TimelineView.qml` | 762-998 | `contextMenu` 每次重建大量动态对象 | 右键菜单每次打开都 `createObject` 数十个。建议预创建或缓存。 |

### 3.3 冗余/死代码

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P2** | `ui/qml/common/TimelineHelper.js` | - | `prop`/`invoke`/`invokeWith` 未使用 | 无调用者。 |
| **P2** | `ui/qml/timeline/TimelineView.qml` | 119-131 | 时间线缩放转换逻辑重复 | 与 `TimelineHelper.js:30-36` 完全相同。 |
| **P2** | `ui/qml/common/BlendModeUtils.js` + `BaseObject.qml:48-66` | - | Blend mode 映射重复 | 两套字符串常量。 |
| **P2** | 5 个 QML 文件 | - | 时间线外观常量重复定义 | `layerCount`/`layerHeight`/`clipResizeHandleWidth` 等默认值不一致。 |
| **P2** | `ui/qml/timeline/ClipItem.qml` | 300-306 | 开发调试注释残留 | `// [FIX] REMOVED...` 等。 |
| **P2** | `ui/qml/common/ControlLoader.qml` | 729-765 | `fontComponent` 未使用 | 无定义使用 `font` 类型。 |
| **P2** | `ui/qml/SettingDialog.qml` | - | 超 2300 行 | 单文件承担过多职责。建议拆分。 |
| **P2** | `ui/qml/timeline/TimelineView.qml` | 16-18 | `getLayerLocked` 默认值始终返回 false | stub 未使用。 |
| **P2** | `ui/qml/common/Icons.js` | - | 缺少 `.pragma library` | 每次 import 重复实例化大对象。 |
| **P2** | `ui/src/timeline/timeline_controller_clip.cpp` | 422-465, `timeline_controller_scene.cpp:25-76` | `clips()` 与 `getSceneClips()` 数据扁平逻辑重复 | 抽出 `clipToVariantMap()`。 |
| **P2** | `ui/src/timeline_controller.cpp` | - | 大量简单转发函数 | 可考虑直接注册 `TimelineService` 减少转发。 |

### 3.4 C++/QML 代码质量

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P2** | `ui/include/timeline_controller.hpp` | - | 包含大量头文件 | 公开头文件应尽量减少依赖，使用前置声明。 |
| **P2** | `ui/src/timeline_controller.cpp` | 15-27 | 构造函数直接调用 bake | 单例未初始化时有启动顺序问题。建议延迟到首次渲染。 |
| **P2** | 多处 QML | - | 魔法数字分散 | `layerCount: 128`、`layerHeight: 30`、`0.001` 浮点比较等。建议提取到 Settings。 |
| **P2** | `ui/qml/common/BaseEffect.qml:14,51-61` | - | `_tmRev` 人工 revision 模式 | 触发多个依赖属性重新求值，可能产生绑定循环。 |
| **P2** | `ui/src/workspace.cpp` | 52 | `new TimelineController(this)` | 父对象为 Workspace，生命周期依赖 Workspace。 |
| **P2** | `ui/src/timeline/timeline_commands.cpp` | 102 | NOLINT(bugprone-easily-swappable-parameters) | 压住告警但未解决问题。 |

---

## 四、构建脚本与辅助工具

### 4.1 安全

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P0** | `BUILD.py` | 257 | `git clone` 下载 Carla 头文件无 commit/tag 校验 | 可能引入未预期上游变更。应固定 commit。 |
| **P0** | `BUILD.py` | 281-287 | 下载 zip 无 SHA256 校验 | 可能被中间人篡改。 |
| **P0** | `BUILD.py` | 1387-1388 | macOS 仅 ad-hoc 签名未公证 | Gatekeeper 会阻止分发。应增加 `xcrun notarytool`。 |
| **P0** | `BUILD.py` | 850 | vcpkg clone 无 commit 固定 | 每次拉取最新 master。 |
| **P0** | `export.py` | 142-153 | 无差别导出项目文件 | 可能泄露敏感文件。应排除 `.env`/`*secret*` 等。 |
| **P0** | `clean.py` | 100-104 | 用户输入正则直接拼接 | 恶意正则可导致 ReDoS。建议限制或校验输入。 |
| **P1** | `BUILD.py` | 316-345 | `download_and_extract` 临时文件命名非独占 | 多进程同时写入冲突。建议 `tempfile.NamedTemporaryFile`。 |
| **P1** | `BUILD.py` | 326 | `urlopen` 未显式校验证书 | 可能被 MITM。 |
| **P1** | `BUILD.py` | 342 | `shutil.unpack_archive` 未校验路径 | 含 `..` 组件可能写出到预期外目录。 |
| **P1** | `BUILD.py` | 1041-1046 | `tempfile.NamedTemporaryFile` 写 bat 未引号转义 | 特殊字符路径可能命令注入。 |

### 4.2 性能/逻辑

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P1** | `BUILD.py` | 581-592, 742-760, 1265-1285 | 三套 Carla DLL 拷贝逻辑重复 | 应统一。 |
| **P1** | `CMakeLists.txt` | 312-360 | shader 编译逻辑在 CMake 中重复两次 | 内置 + effect-packages。 |
| **P1** | `CMakeLists.txt` | 390-405 | POST_BUILD 三次 copy_directory 同一目录 | 冗余拷贝拖慢增量构建。 |
| **P1** | `check.py` | 265-275 | clang-tidy 多进程 `--fix` 竞态 | 多进程同时修改文件。 |
| **P1** ✅ | `check.py` | 96, 105 | 格式化失败不返回错误码 | 静默忽略。 |
| **P1** | `BUILD.py` | 663-682 | `copy_msys2_dependency_dlls` 对每个二进制调 objdump | 可缓存输出。 |
| **P1** | `BUILD.py` | 1372-1385 | macOS `carla-discovery-native` 用 glob 扫描整个 Homebrew prefix | 建议直接构造已知路径。 |
| **P2** | `BUILD.py` | 162-205 | 容器内 `cwd = os.getcwd()` 取宿主机目录 | 应使用 `self.config.source_dir`。 |
| **P2** | `BUILD.py` | 611-620 | `find_runtime_dll` 可能返回错误 DLL | `shutil.which` 返回 PATH 中第一个匹配，不一定在 MSYS2 bin。 |
| **P2** | `BUILD.py` | 951 | 比较 mtime 决定是否跳过生成 | .lib 比 DLL 新但 .def 缺失会漏掉。 |
| **P2** | `BUILD.py` | 973-975 | `dumpbin /exports` 解析简单 | 导出名含空格或 forwarder 时误判。 |
| **P2** | `BUILD.py` | 1320-1325 | `XcodeBuilder` fallback 固定 `/opt/homebrew` | Intel Mac 上不正确。 |
| **P2** | `format.fish` | 12-44 | C++ 与 QML 两段格式化几乎镜像 | 可合并为通用函数。 |
| **P2** | `format.fish` | 21-23, 37-39 | 每个文件单独调用格式化工具 | 大量小文件时频繁 fork。 |
| **P2** | `tests/CMakeLists.txt` | 36-40 | Windows 测试 PATH 只加 Carla lib | 未加 Qt/vcpkg/MSYS2 运行时 DLL。 |

### 4.3 可维护性/配置

| 优先级 | 文件 | 行号 | 问题 | 说明与建议 |
|--------|------|------|------|------------|
| **P2** | 多处 | - | 版本号分散 3 处 | `CMakeLists.txt`(0.4.0) + `BUILD.py`(0.0.0) + `version.hpp`。应统一。 |
| **P2** | `.clang-tidy` | - | 禁用 50+ 条规则 | 几乎关闭所有严格检查。建议定期评审。 |
| **P2** | `BUILD.py` | 69-76 | `CARLA_DLL_PATTERNS`/`CARLA_DLL_ALIASES` 在基类定义 | 仅 Windows 子类使用。 |
| **P2** | `BUILD.py` | 85-87 | `container_name`/`use_container` 在基类初始化 | 仅 Linux 子类使用。 |
| **P2** | `BUILD.py` | 131-135 | `get_cmake_cmd()`/`install_dependencies()`/`package()` 基类空实现 | 应改为抽象方法。 |
| **P2** | `BUILD.py` | 1317 | `# macOS uses Universal binaries` 悬空注释 | 后面无代码。 |
| **P2** | `check.py` | 13-22 | 文档 `--clazy-level` choices 1-3 但实际 1-2 | 注释与代码不一致。 |
| **P2** ✅ | `check.py` | 174 | `run_cppcheck` 返回 `returncode` 但签名声明 `-> list[str]` | 类型不匹配。 |
| **P2** | `check.py` | 218-221 | `--full` 模式 `level2` 默认包含 `level1` | 重复指定无意义。 |
| **P2** | `CMakeLists.txt` | 438 | `if(TS_FILES)` 恒为真 | 条件分支无意义。 |
| **P2** | `BUILD.py` | 318-346 | `import tempfile` 但未使用 | 死导入。 |
| **P2** | `BUILD.py` | 1444-1481 | `BuildWorker` 继承 `threading.Thread` 日志通过 queue 回传 | 结构复杂，可用 `concurrent.futures` 简化。 |
| **P2** | `BUILD.py` | 1531-1533 | `--version` 默认 `0.0.0` | 与 CMake 项目版本 `0.4.0` 不一致。 |
| **P2** | `BUILD.py` | 156-160 | `archive()` 总是生成 zip | macOS app bundle 用 zip 会丢失符号链接/权限。建议用 tar。 |

---

## 五、全局汇总 Top 10

| # | 模块 | 问题 | 严重性 | 文件 |
|---|------|------|--------|------|
| 1 | Core | `AudioDecoder` 全量解码 + `getSamples` 返回拷贝 | P0 性能/内存 | `core/src/audio_decoder.cpp` |
| 2 | Core | `SettingsManager::save` 非原子写入 | P0 正确性 | `core/src/settings_manager.cpp` |
| 3 | Core | `ProjectSerializer::load` 无输入校验 | P0 安全 | `core/src/project_serializer.cpp` |
| 4 | Engine | `AudioMixer::mix` 返回引用 + 裸指针悬空 | P0 正确性 | `engine/audio_mixer.cpp` |
| 5 | Engine | `getParamInfo` 固定 [0,1] 范围 | P0 安全 | `engine/plugin/audio_plugin_manager.cpp` |
| 6 | UI | `thread_local dummy` 返回可修改全局 | P0 正确性 | `ui/src/timeline/timeline_scene.cpp` |
| 7 | UI | `checkPermission` 默认 allow unknown | P0 安全 | `scripting/mod_engine.cpp` |
| 8 | Build | 下载无校验 + ad-hoc 签名未公证 | P0 安全 | `BUILD.py` |
| 9 | Core | `VideoFrameStore` 无界内存增长 | P1 性能 | `core/src/video_frame_store.cpp` |
| 10 | UI | `findClipById` 线性搜索 + `getWaveformPeaks` 逐像素 | P1 性能 | `ui/src/timeline/timeline_clip.cpp` |

---

## 附录：统计概览

| 模块 | P0 问题 | P1 问题 | P2 问题 | 总计 |
|------|---------|---------|---------|------|
| Core | 5 | 4 | 12 | 21 |
| Engine | 5 | 11 | 18 | 34 |
| UI/QML/Scripting | 8 | 11 | 20 | 39 |
| Build/Tools | 6 | 7 | 16 | 29 |
| **总计** | **24** | **33** | **66** | **123** |
