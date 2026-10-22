# Slint desktop acceptance — macOS, first candidate run

Date: 2026-10-22. Host: macOS 26, Apple M5, Metal (no discrete GPU).
Build: `rust/target/debug/aviqtl-slint` from `release/v0.7.0-rc.1` (`0.7.0-rc.1`).
Frontend language: Chinese. Interface driven by real mouse and keyboard events
(`CGEvent`) and inspected through the accessibility tree (`AXUIElement`), not by
unit tests.

This is the first recorded desktop session for the Slint frontend. It does not
complete the deferred suite in [QT_UI_PARITY.md](../rust/aviqtl-slint/QT_UI_PARITY.md);
it records what was actually operated, what failed, and what could not be
verified on this host.

Severity: **High** = a workflow loses data or a command cannot be reached;
**Medium** = extra steps, misleading or unreachable UI; **Low** = cosmetic or
wording only.

## Verified working

| Behavior | Evidence |
| --- | --- |
| Startup on a GPU host | Reports `AviQtl Slint GPU: Apple M5 (Metal)`, loads settings, `Effect catalog · 61 definitions`, `Script plugins · 4 loaded` |
| Launcher | New-project form (1920x1080, 60 fps, 48000 Hz defaults), empty recent list, open-project button |
| Recovery window | Lists the leftover snapshot with its timestamp and offers 恢复 / 放弃 / 关闭 |
| New project | Creates the preview and timeline windows; the preview carries a project tab with its own close button |
| Native menu bar | 文件 / 编辑 / 设置 / 显示 / 工具 in Qt's order, with the Qt item set (`新建项目`, `打开项目`, `覆盖保存项目`, `项目另存为...`, `管理缺失媒体`, `插入片段...`, `导出片段...`, `从 AviUtl 物件文件新建项目...`, `导入 AviUtl 物件文件（兼容）...`, `导出媒体...`, `退出`) and ⌘ accelerators rendered in the items |
| Timeline context menu | `搜索目录…`, `浏览对象目录…`, `添加对象 ▸`, `撤销`, `重做`, `粘贴`, `场景设置…`, `项目设置…`, `首选项…` |
| Catalog search | Opens with the field focused, filters as text is typed (`文本` → `其他 / 文本`), Return accepts |
| Object insertion | Inserted at the clicked layer and playhead frame: layer 3, frame 90, 100-frame duration; scene duration grew to 190 frames |
| Clip selection and drag | Clicking selects; dragging 100 px at 100 % zoom moved the clip from frame 240 to frame 340, exactly 1 px per frame, no snap drift |
| Trim handles | The clip exposes accessible 调整片段起点 / 调整片段终点 handles |
| Layer left click | `显示，未锁定` became `隐藏，未锁定` on the clicked layer only, and that layer became the selected layer |
| Seek | Clicking the progress bar at 50 % produced frame `095 / 190` |
| Playback | Play ran the counter from `001` to `190` (the scene end) in about three seconds at 60 fps, then stopped |
| Text rendering | The inserted text object draws in the preview canvas |
| Close with unsaved changes | Raises 保存确认 with the project name and 保存 / 放弃 / 取消; Escape dismisses it and keeps the project; 放弃 closes the project and returns to the launcher |
| Save dialog | ⌘S on an untitled project opens the native save panel and appends `.aviqtl` to the name |

## Deviations

### P1. Editor shortcuts are unreliable (High)

⌘Z and ⌃Z left the model unchanged in four of six attempts, in both the preview
and the timeline window. Two attempts did undo (a moved clip returned from frame
340 to 240; an inserted object was removed), and ⌘⇧Z redid afterwards.

Reproduction: insert an object from the timeline context catalog, click inside
the timeline viewport, press ⌘Z. The clip keeps its position and the accessible
description (`剪辑：标准绘制，图层 3，从第 90 帧开始，持续 100 帧`) does not change.
Clicking the preview window and pressing ⌘Z undid the same edit in two earlier
attempts and did nothing in two later ones.

The failure is silent: no status message, no history entry consumed. For an
editor whose timeline workflow is keyboard driven this is the most important
finding of the session. It is consistent with the `foundation` status of the
"Timeline focus" row, but it also means that row cannot be promoted yet.

A related observation: typing into the catalog search field after a ⌘Z attempt
left a literal `z` in the query (`z文本`), so the shortcut is being delivered to
whichever text field still holds focus instead of reaching the command.

### P2. Status messages stay English in a Chinese UI (Medium)

The status bar shows `Ready`, `Edit applied`, and
`Move the edit frame inside at least one selected clip before splitting.` while
every other string is Chinese. The refusal text is the only feedback for a
rejected split.

### P3. The inserted text object's default content is Japanese (Low)

Inserting `其他 / 文本` from a Chinese UI produces an object whose text renders as
`テキスト` in the preview. Default object content is not localized with the
interface.

### P4. Frame counter padding is inconsistent (Low)

An empty project shows `0 / 1`; a scene with objects shows `001 / 190`.

### P5. Recovery write failures are only logged (Low)

Under this host's sandbox the application could not write its recovery
directory. The failure appeared on stderr once per second
(`Project recovery failed: Operation not permitted (os error 1) at path
"~/Library/Application Support/AviQtl Plus/recovery/.aviqtl-save-…"`) and
nowhere in the interface, so a user whose recovery is not working has no way to
learn it. This is a general property of the retry loop, not a sandbox artifact:
any persistent write failure behaves the same way.

## Not verified on this host

- **Saving and reopening a project.** The host denies writes outside the working
  tree, so the save failed. The failure itself was correct and informative: the
  alert named the target file and the atomic temporary file
  (`~/Documents/.aviqtl-save-VD16vk`), which matches the documented
  write-then-rename design. A round trip still has to be performed on an
  unsandboxed desktop.
- **Media import.** The file menu has no import-media command and the host had no
  drag source, so no decoder, waveform, or media clip path was exercised.
- **Export.** Not run; it needs a project with media.
- **Object settings, package manager, and export windows.** Not opened.
- **IME, VoiceOver traversal, and window close/quit ordering.** Out of scope for
  a single session on a host without an assistive client.
- **The menu bar disappearing.** Twice the 文件/编辑/设置/显示/工具 items vanished
  while the timeline window held focus and came back after the application was
  deactivated and reactivated, but both later attempts to reproduce it showed the
  full menu bar on every window switch, so it is recorded as unconfirmed.

## Follow-up

`QT_UI_PARITY.md` keeps its statuses except where this session provides evidence;
the rows it exercises but does not clear are listed in the audit rather than
promoted. The undo route deserves a code-level investigation before the tag.

Reading the two dispatch paths next to the observations narrows the failure to
two mechanisms that both have to change:

1. **The in-window path cannot match on macOS.** `parse_shortcut` maps `Ctrl` to
   the literal control flag (`"cmd"` is the one that becomes control on macOS),
   and `shortcut_matches` compares `input.text` character by character. Pressing
   ⌃Z makes the backend deliver the control character as the text, and ⌘Z arrives
   as `meta`, so neither ever equals the recorded pattern. That is why ⌃Z did
   nothing in every attempt and no in-window shortcut fired.
2. **The native menu accelerator is the only working route, and it depends on
   which window is key.** The `MenuBar` lives in `MainWindow`, and ⌘Z worked in
   exactly the attempts made right after the preview window was focused. While
   the timeline held focus the accelerator was gone, and ⌘Q from the always
   present application menu quit reliably, which shows the menu path itself
   works.

A third condition can disable the accelerators without any visible sign:
`menu-accelerators-enabled` (`!speed-input.has-focus` and
`!missing-media-dialog-visible`) and the per-item `save-confirmation-visible`
gate clear the shortcut of the affected items.

The literal `z` left in the catalog search field after a ⌘Z attempt is the same
event reaching a focused text control instead of the command.
