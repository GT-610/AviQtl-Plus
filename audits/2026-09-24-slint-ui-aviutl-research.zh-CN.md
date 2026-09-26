# Slint UI 与 AviUtl、主流剪辑软件的交互调研

调研日期：2026-09-24。代码基线：`634bf26`；`rust/Cargo.lock` 锁定 Slint 1.17.1。

建议继续以 AviUtl 的图层与对象编辑为主体，优先完善参数操作、快捷键、时间线反馈和窗口空间利用，再引入素材管理及可选的剪辑工具。现有 Slint 架构可以支持这条路线，无需先更换 UI 框架或重写全部控件。

本次只调研，未修改产品代码。证据来自当前 Slint/Rust 源码、锁定版本的 Slint 实现、AviUtl 官方发行包说明，以及其他软件的官方资料。没有启动编辑器做桌面交互验证；下文的尺寸问题是布局分析，操作耗时、实际焦点流转、DPI 效果和性能仍需实测。建议与已确认的实现事实分别表述。

项目 README 当前要求 Qt 交互一致性。本报告讨论的是用户要求的后续产品优化；建议实施时区分“修复现有问题”和“有意改变操作”，同步更新验收说明。Qt 一致性不等于 AviUtl 一致性。

**1. 对照基线与应保留的特点。**

“AviUtl”需要区分两个版本：经典 AviUtl 1.10 + 拡張編集 0.92，和 AviUtl ExEdit2。此次官网显示 ExEdit2 最新发行版为 2.1.10，发布于 2026-09-19。两者都支持数值拖动和精调，但窗口组织已有明显区别。[S1–S3]

| 对照产品 | 官方资料确认的特点 | 对 AviQtl 的启发 | 适用边界 |
| --- | --- | --- | --- |
| 经典 AviUtl + ExEdit 0.92 | 图层编号点击切换显示；Ctrl 点击选整层；对象右键插入；起止参数与运动方式；数值拖动、Shift 精调；对象中间点 | 保留图层、对象、帧和起止参数的操作语言，优先补齐手势 | 不能把 Qt 已实现的行为全部视为 AviUtl 原样行为 |
| AviUtl ExEdit2 2.1.10 | 面板可调大小、换位、分离及分组；数值 Ctrl+滚轮；按键录入快捷键；部分属性支持多对象修改 | 可选整合布局、快捷键录入、兼容属性批量调整 | 多选属性修改需要明确相同参数与混合值的语义 |
| DaVinci Resolve | 源素材入出点、明确的插入/覆盖操作、智能修剪、波形辅助定位 | 借鉴修剪反馈、源素材预览和剪辑工具的可发现性 | Ripple/Roll/Slip/Slide 需要编辑模型支持，不是换鼠标图标即可实现 [S4] |
| Final Cut Pro | 主故事线自动避让/闭合；连接片段随主片段移动；Position 工具可暂停磁性行为 | 借鉴关联片段保持同步与可选的连续剪辑体验 | 不建议将磁性时间线作为 AviUtl 图层合成的默认规则 [S5] |
| Kdenlive | 不同制作阶段的工作区；时间线导航；效果收藏、效果栈与关键帧 | 增加布局预设、效果收藏、键盘导航与时间线状态提示 | 先提供低成本工作区预设，完整自由停靠后置 [S6–S8] |
| Shotcut | 可搜索并执行操作、可编辑快捷键、缩略图/波形、素材列表、预览降分辨率 | 操作搜索、轻量素材面板和预览质量快捷入口 | 项目已有波形和预览缩放能力，重点是入口与反馈 [S9] |

尝试访问 Adobe Premiere 官方修剪说明和用户指南时返回 403，因此不以未读取到的页面作为功能结论来源。本次主流商业软件对照以 Resolve、Final Cut Pro 为主，另用 Kdenlive、Shotcut 评估自由软件中可借鉴的实现范围。

**2. 当前代码已经做到了什么。**

| 能力 | 当前证据 | 本次判断 |
| --- | --- | --- |
| 预览、时间线、对象设置分窗 | `main.slint`、`timeline.slint`、`object-settings.slint`，以及 `src/lifecycle.rs` | 符合经典 AviUtl 的基本工作习惯，保留 |
| 标准控件和平台主题 | `theme.slint` 使用 `Palette`、`StyleMetrics`；表单大量使用 Button、LineEdit、Slider、CheckBox、ComboBox、SpinBox | 已有正确基础，优化应继续复用 |
| 起始值—运动方式—结束值 | `object-controls.slint:425` 起的数值行 | 已接近 AviUtl，不必换成只有单值的通用 Inspector |
| 参数关键帧和曲线设置 | `KeyframeTrack`、`easing-config.slint` | 已存在，下一步应改善快捷入口和语义 |
| 上下文搜索、分类菜单 | `timeline.slint:1026` 起；搜索支持选择及键盘导航 | 上轮审计相关问题已修复，不重复列为缺失 |
| 标签溢出滚动 | `main.slint:137` 附近、`timeline.slint:441` 附近的 ScrollView | 已修复；还应实测选中标签自动进入可见区域 |
| 图层左击显隐 | `timeline-items.slint:120` 起调用显隐和选中 | 已恢复；Ctrl 分支仍值得补齐 |
| 音频波形与电平、预览质量设置 | `timeline-items.slint:208`、`StereoAudioMeter`、系统设置与 README | 不建议重复开发；电平刻度与质量入口可改善 |
| 快捷键自定义 | `src/shortcuts.rs:236` 起共 34 项；系统设置为文本输入 | 有能力，但录入、发现和冲突反馈可提升 |
| 窗口几何持久化 | `src/dialogs.rs:266` 起、`src/lifecycle.rs` | 已有保存恢复；布局预设/重置入口可另加 |

旧文件 `audits/2026-09-slint-ui-parity.md` 的修复记录仍有参考价值，但其“MenuItem 没有 icon 和 shortcut，因此必须自绘菜单”的结论不适用于当前锁定版本，详见第 4 项。

**3. 优先优化的交互及代码依据。**

| 顺序 | 发现/建议 | 当前依据 | Slint 路径及验收重点 |
| --- | --- | --- | --- |
| P0 | 音频插件数值参数补齐可见名称和无障碍名称 | `object-controls.slint:425–546` 的数值行在 audio-plugin 模式隐藏中央参数名称按钮，只显示滑块、数值、K 等；投影有 label，但没有在该行另绘名称 | 增加 Text 标签，Slider/LineEdit 显式设置 accessible-label；用多个音频插件参数确认每行都能识别 |
| P0 | 一次连续调参形成一个撤销步骤 | Slider.changed 经 `src/object_settings.rs:167` 等进入 workspace.execute；`workspace.rs:2660` 每次提交都入栈。此路径未见拖动会话合并 | 保留标准 Slider，利用 changed/released 组织预览与提交；一次长拖动、一次撤销应回到拖动前，重做回到拖动后 |
| P1 | 参数窗口可压缩、侧栏可折叠/调宽 | `object-settings.slint:7–13` 默认 900×650、最小 680×480；侧栏固定 250；数值行固定 76+150+76px，另有两条滑块、间距和单位 | 先折叠侧栏、可折叠效果组、自适应标签；小范围自定义分隔条。实测窄窗口、中日文、125%/150%/200% 缩放 |
| P1 | 数值拖动、Shift 精调及清楚的提交/取消 | `CommitLineEdit` 只处理 Enter/失焦，未见数值拖动；AviUtl 两代官方说明均描述数值拖动及 Shift 精调 | 标准 LineEdit + 专门的拖动命中区/编辑状态，普通文本选择保持正常；Esc 恢复此次编辑前值，Tab 提交后移到下项 |
| P1 | 运动方式提供常用项快捷菜单 | 当前中央按钮直接打开 `EasingConfigWindow`，后者默认 880×560、最小 760×460 | 常用“无移动/线性/缓入缓出”等使用标准 Menu；“高级曲线…”保留完整窗口；沿用现有起止值规则 |
| P1 | 主菜单显示图标和真实快捷键 | `main.slint:62` 起只有 title/activated；1.17.1 已有相关 API | 接入 MenuItem.icon/shortcut，显示用户实际绑定；菜单与键盘共用命令，检查重复触发和编辑控件焦点 |
| P1 | 快捷键按键录入、搜索和冲突提示 | `system-settings.slint:563` 起要求手写 Ctrl+S 等；解析器存在，但此页没有录制和冲突呈现 | Button/LineEdit/FocusScope + 原有设置模型；新增 AviUtl 操作预设前逐项核对默认键位 |
| P1 | 时间线吸附、略读状态就地可见 | 吸附入口在 `scene-settings.slint:60`，略读在 `system-settings.slint:356`；时间线本身未见对应常驻开关 | 标准 checkable Button/CheckBox；显示开启状态和 Shift 临时忽略吸附提示；状态归属仍由 app/settings 管理 |
| P1 | 缩放到全部、选区与返回上次缩放 | 时间线已有滚轮锚定缩放和百分比输入；34 项快捷键表只见增减缩放 | 按钮/菜单转发视口命令；保留既有滚轮习惯，新增 Fit 命令；长项目定位无需反复缩放 |
| P1 | Ctrl 点击图层头选中整层 | `LayerHeaderItem` 左击没有 modifiers 分支，统一显隐+选中；经典 ExEdit 官方说明明确有 Ctrl 选整层 | 在现有自定义图层头分流修饰键；普通左击语义保留，Ctrl 左击不能误隐藏图层 |
| P1 | 文本编辑能预览，并作为一次编辑撤销 | `CommitTextEdit` 仅失焦提交 | 使用标准 TextEdit，增加输入中的预览草稿/适度合并；中文输入法组字时不抢焦点，不逐字符写历史 |
| P2 | 效果收藏/最近使用，多个选择器交互统一 | 时间线上下文搜索已有上下键、回车和 Esc；对象窗口 effect-picker 仍是 LineEdit + 多个 Button | 共用搜索与选择模型；简单列表优先 StandardListView，复杂项用 ListView；保留上下文目标，不误把效果加到另一对象 |
| P2 | 标记、范围循环、可编辑时间码、缩略图 | 当前 UI 有帧数与波形，但所查文件未见完整这些入口 | 时间码用 LineEdit/菜单，范围与标记需要模型；缩略图需要后台缓存，不能在每次滚动同步解码 |
| P2 | 保留分窗，增加排列/重置/工作区预设 | 已有几何持久化，尚未见用户工作区管理入口 | 标准菜单管理“默认/动画/音频”等布局；先解决窗口找回与常用排列，再评估整合模式 |
| P3 | 素材面板、源素材入出点、多选属性、可选 Ripple/Slip | 当前重点是对象式编辑；这些需要更广的模型与命令支持 | 另立产品功能，不混入一轮控件替换；先明确锁定层、关联音频、嵌套场景及撤销规则 |

P0/P1/P2/P3 在此表示建议顺序，不代表已完成运行时缺陷复现。P0 中参数名称缺失由声明与投影路径直接支持；连续调参历史由调用链支持，仍建议以实际拖动和历史检查验收。

参数布局的具体压力：最小窗口宽 680px，减去侧栏 250px、两端数值框 152px、中央按钮 150px，再减去内容 padding 和间距，两条滑块只能分享很少的空间，单位和滚动条还会继续占用。与其统一缩小所有标准控件，更适合先让侧栏收起、名称宽度可调，并允许效果组折叠。当前每个可动画数值参数还会追加关键帧行，应允许按需展开，而不把所有参数永久铺开。

数值调节还需要区分“滑块常用范围”和“参数合法范围”。经典 ExEdit 部分参数允许输入/拖动超出滑块范围；当前 ObjectControl.parse_text 使用 metadata 的 min/max 限制数值。若要兼容这种习惯，应先检查参数 schema 是否能表达软范围与硬范围，不能简单删除限制。

连续调参的实现不能只在第一下 changed 调用 begin_undo_group、最后 released 调用 end_undo_group。当前 finish_timeline_edit 每次都会裁剪历史，而分组记录的是 undo.len() 的索引，长拖动超过历史上限时可能破坏分组边界。应建立稳定的参数编辑会话，保存原始值和最终值，或者在历史层提供有明确边界的合并策略；处理中途切换对象、关闭窗口、取消和键盘调节。Slint 的 released 也有键盘释放路径，不能一概等同于鼠标释放。

对象级“中间点”需要单独设计。经典 ExEdit 的中间点是对象内部的时间分段，默认 P 添加，并影响多个参数区间；当前 KeyframeTrack 操作的是单个 effect/param 的关键帧。建议保留现有参数级能力，新增对象中间点时明确哪些参数联动、如何插值、移动时是否带动后续分段，以及序列化/撤销语义。不要直接把“所有参数各加一帧”命名为完整 AviUtl 中间点兼容。

预览直接操纵也是后续价值较高的方向：当前主预览区域基本是 Image，没有对应的对象选择和变换命中区。可以在 GPU 预览上叠加 Slint 选择框、控制柄和安全区；命中测试、坐标映射、多选变换和撤销由 app/core 管理。这个工作量高于标准控件替换，建议放在参数与时间线基本手感稳定之后。

**4. “尽可能 Slint 原生”的准确范围。**

这里建议将“原生”分成 Slint 自带的标准控件/内建元素和操作系统本地窗口/对话框。当前使用 winit 与 GPU 渲染；使用 std-widgets 不意味着每个按钮都是 Win32 控件。rfd 文件选择器已承担平台文件对话框，不必为了统一外观重写。

| 区域 | 优先使用 | 必须保留/补充的逻辑 |
| --- | --- | --- |
| 菜单栏和右键菜单 | 内建 MenuBar、Menu、MenuItem、ContextMenuArea | 上下文目标、动态分类、命令启用条件 |
| 播放、上一帧、下一帧、添加/关闭按钮 | 标准 Button，icon、checkable、enabled 等能力 | 先核对焦点、热键及平台最小尺寸；专有标签交互可继续自定义 |
| 参数输入 | Slider + LineEdit；整数按空间需求选择 SpinBox；CheckBox、ComboBox、TextEdit | 提交/取消、数值拖动、单位、软硬范围和撤销会话 |
| 简单选择列表 | StandardListView | 过滤、当前项、Enter 执行与目标验证 |
| 效果栈、字体预览、复杂资源行 | ListView + 自定义 delegate，内部继续使用标准控件 | 排序、多选和预览；保留长寿命模型，别在拖动中整体替换 |
| 固定设置分页 | TabWidget 或标准可选中按钮 | 页面大小、翻译和键盘遍历；不必为替换而重写现有可用页面 |
| 动态项目/场景标签 | 现有 ScrollView + 标签组件 | 关闭、根场景不可关闭、右键设置、自动滚动到当前项；TabWidget 不能直接覆盖所有语义 |
| 时间线、波形、关键帧、曲线 | Slint Rectangle/Path/TouchArea 等自定义编辑表面 | 帧定位、修剪、选择、拖动和吸附，这些没有现成时间线控件 |
| 调整分隔条、自由停靠 | 少量自定义组件；完整停靠需独立设计 | 1.17.1 标准控件清单中未见通用 DockManager/SplitView，不应承诺直接换控件即可 |
| 颜色/字体选择与提示层 | 标准输入控件组合，加统一弹层基础组件 | 本版本没有在所查标准导出中找到通用 ColorDialog/FontDialog；需处理 Esc、焦点约束与回焦 |

已核对的版本能力：

- Slint 1.17.1 `MenuItem` 有 `icon`、`shortcut`、`checkable`、`checked`；`Menu` 也有 `icon`。源码内注明 `shortcut` 仅适用于 MenuBar 内的条目。通用菜单实现会显示 `entry.shortcut.to-string()` 和 `entry.icon`。[S10]
- Rust API 提供 `slint::Keys::from_parts`，可以把项目保存的快捷键转换为运行时 Keys。需要转换 Ctrl/Control、按键名称、大小写和 `+` 等特殊符号；不能把设置字符串不加处理直接塞给 shortcut。该 API 当前表达单组按键组合。[S11]
- MenuItem.shortcut 同时涉及执行，不能只当一个装饰标签。应避免与现有 FocusScope 路由双重执行，并保留文本框中的复制、删除、撤销及弹层优先级。右键菜单不能直接复用这个属性；可以通过操作搜索/快捷键页呈现绑定，暂不为此放弃标准菜单。
- 标准 Slider 是浮点值，提供 changed 和 released；项目目前的参数滑块只用了 changed。[S12]
- 标准 SpinBox 的 value、minimum、maximum、step-size 是整数。因此浮点参数宜继续组合 Slider 与 LineEdit，不应宣称可以无损替换成原生“浮点 SpinBox”。[S13]

这些判断以本地锁定依赖和官方 v1.17.1 源码为准。在线 latest 文档会变化，不能据此假定项目已经具有新版本功能。

**5. 推荐的布局与工作流。**

默认继续使用“预览 + 时间线 + 对象设置”三个可独立放置的窗口。时间线增加一条尽量紧凑的操作区：吸附、略读、缩放到全部/选区、当前帧/时间码。大量低频操作仍放在菜单中，避免常驻工具挤压图层区。

对象窗口保留左右起止值，中间显示参数名称与运动方式。常用运动方式就地选择，高级曲线另开窗口。效果标题可折叠，左侧效果列表允许收起；没有动画时减少冗余关键帧区域，启用动画后清楚展示当前区间。音频参数采用“名称 + 数值/滑块 + 动画入口”的清晰行式布局。

借鉴主流剪辑软件时，先补“正在操作什么”的反馈：拖动片段显示起点、终点、时长与变化量；吸附时显示目标线；播放头和鼠标略读位置使用不同视觉编码。当前快捷键分割/粘贴路径会读取 skimmer，操作提示应明确本次目标是鼠标位置还是播放头，并提供兼容设置。不要在没有说明的情况下改掉旧项目用户的肌肉记忆。

后续可提供整合工作区，但保持与分窗共享同一组内容组件和命令。先做预设布局与简单分隔调整，可以控制成本；自由拖拽停靠涉及窗口所有权、GPU 预览承载、焦点和快捷键作用域，应单独验收。ExEdit2 可停靠说明“接近 AviUtl”并不只允许分窗，但不构成立刻全面重做窗口系统的理由。

磁性时间线与波纹操作应作为可选编辑模式或显式命令。AviUtl 的图层常用于字幕、音效和合成；自动移动其他图层可能破坏精确同步。引入前需定义受影响的层、锁定层、关联音频、场景对象以及冲突预览。

**6. 建议实施批次和验收任务。**

| 批次 | 内容 | 工作边界 | 完成标志 |
| --- | --- | --- | --- |
| 第一批 | 参数标签、连续调参撤销、窄窗口布局、标准菜单图标/快捷键 | 主要前端 + 必要的编辑会话/历史支持 | 能识别每个参数；一次拖动一次撤销；无控件挤压；快捷键只执行一次 |
| 第二批 | 数值拖动精调、运动方式快选、Ctrl 图层选择、吸附/略读/Fit、快捷键录入 | 高频交互与少量新命令 | 常用操作在当前窗口完成；新旧操作模式有明确说明 |
| 第三批 | 效果收藏、统一选择器、文本预览、布局预设、时间线提示 | 工作流与呈现完善 | 搜索→选择→执行→返回焦点全过程可用键盘完成 |
| 第四批 | 对象中间点、预览直接操纵、素材面板、多选属性、可选剪辑工具和整合工作区 | 编辑模型和窗口架构扩展 | 新增语义有独立方案、项目兼容规则及行为测试 |

后续验收建议用小项目执行以下任务，记录点击/按键次数和结果，不预先声称节省百分比：

1. 导入视频并添加文本，把 X 从起始值调整为结束值，设置缓动；全程不丢失输入焦点，预览与提交一致。
2. 连续拖动一个参数数秒并跨越 32 次变化，一次撤销/重做；再验证 Esc、切换对象及关闭窗口中断。
3. 建立多个音频插件参数，逐项用鼠标和键盘调节，确认名称、单位、当前值和动画状态可读。
4. 普通点击图层显隐，Ctrl 点击选整层，然后复制/删除；验证隐藏、锁定和多选规则。
5. 在开启/关闭略读时于播放头以外按分割键，确认落点可预测；Shift 临时忽略吸附后释放，状态恢复。
6. 在 680px 宽对象窗口、多项目/多场景、中日文和多种系统缩放下操作，检查截断、滚动和当前标签可见性。
7. 在数值输入、中文组字、文本选区和弹层中使用 Space、S、Delete、Ctrl+Z，确认编辑命令没有穿透。
8. 开启大量效果/字体/片段后测滚动和搜索；只有出现性能问题再决定虚拟化、可见区裁剪和缩略图预算。

本轮没有运行构建或测试，因为未修改可执行代码，也没有把静态分析写成实际 UI 验收结果。现有测试通过记录不能替代以上新增行为的验证。

**7. 来源与复核入口。**

网页及官方说明访问日期均为 2026-09-24。发行包仅用于读取说明文档，没有执行其中程序。

| 编号 | 来源 | 支持的结论 |
| --- | --- | --- |
| S1 | [AviUtl 官方网站](https://spring-fragrance.mints.ne.jp/aviutl/) | 经典版本、ExEdit2 当前发行信息及官方包链接 |
| S2 | [ExEdit 0.92 官方发行包](https://spring-fragrance.mints.ne.jp/aviutl/exedit92.zip)，内含 `exedit.txt` | 图层操作、数值拖动、Shift 精调、运动方式、P 添加中间点 |
| S3 | [ExEdit2 2.1.10 官方发行包](https://spring-fragrance.mints.ne.jp/aviutl/aviutl2_v2.1.10.zip)，内含 `aviutl2.txt` | 窗口分组/分离、数值操作、快捷键录入、多对象属性修改 |
| S4 | [DaVinci Resolve Edit 官方介绍](https://www.blackmagicdesign.com/products/davinciresolve/edit) | 源素材 I/O、编辑方式、智能修剪、音频定位 |
| S5 | [Final Cut Pro：Magnetic Timeline](https://support.apple.com/guide/final-cut-pro/intro-to-the-magnetic-timeline-verb8fcfc133/mac) | 自动避让/闭合、连接片段、Position 工具 |
| S6 | [Kdenlive：Timeline](https://docs.kdenlive.org/en/user_interface/timeline.html) | 导航、缩放、波形和关键帧可见性 |
| S7 | [Kdenlive：Effects and Filters](https://docs.kdenlive.org/en/effects_and_filters.html) | 效果收藏、效果栈与关键帧 |
| S8 | [Kdenlive：Workspace Layouts](https://docs.kdenlive.org/en/user_interface/workspace_layouts.html) | 剪辑、音频、效果、调色等工作区 |
| S9 | [Shotcut 官方功能说明](https://shotcut.org/features/) | 操作搜索、可编辑快捷键、素材列表、缩略图及预览缩放 |
| S10 | [Slint 1.17.1 builtins.slint](https://github.com/slint-ui/slint/blob/v1.17.1/internal/compiler/builtins.slint#L1296)、[菜单实现](https://github.com/slint-ui/slint/blob/v1.17.1/internal/compiler/widgets/common/menu-base.slint#L138) | MenuItem 图标/快捷键及限制、实际呈现路径 |
| S11 | [Slint 1.17.1 input.rs](https://github.com/slint-ui/slint/blob/v1.17.1/internal/core/input.rs#L735) | Keys::from_parts 与单组按键组合 |
| S12 | [Slint 1.17.1 Slider](https://github.com/slint-ui/slint/blob/v1.17.1/internal/compiler/widgets/fluent/slider.slint#L7)、[SliderBase](https://github.com/slint-ui/slint/blob/v1.17.1/internal/compiler/widgets/common/slider-base.slint) | 浮点数值、changed/released、键盘路径 |
| S13 | [Slint SpinBox 文档](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/spinbox/)、[1.17.1 实现](https://github.com/slint-ui/slint/blob/v1.17.1/internal/compiler/widgets/fluent/spinbox.slint#L41) | SpinBox 为整数值控件 |

本地主要复核入口：[前端 README](../rust/aviqtl-slint/README.md)、[对象控件](../rust/aviqtl-slint/ui/object-controls.slint#L425)、[对象窗口](../rust/aviqtl-slint/ui/object-settings.slint#L116)、[时间线](../rust/aviqtl-slint/ui/timeline.slint#L414)、[图层头](../rust/aviqtl-slint/ui/timeline-items.slint#L60)、[快捷键路由](../rust/aviqtl-slint/src/shortcuts.rs#L236)、[参数命令桥接](../rust/aviqtl-slint/src/object_settings.rs#L167)、[历史提交](../rust/aviqtl-app/src/workspace.rs#L2660)。
