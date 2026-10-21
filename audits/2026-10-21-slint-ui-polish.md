# Slint UI polish audit — final interaction pass

Baseline: `main` at `57ea39a`. Scope: the remaining presentation and interaction defects in the
Slint frontend (`rust/aviqtl-slint/`) after the September parity work. Qt/QML stays the behavioral
baseline; colors, spacing, and typography may differ, commands and focus rules may not.

Verification on this host: `cargo check -p aviqtl-slint --all-targets` (clean),
`cargo fmt --all -- --check` (clean), `cargo test --workspace` (all pass, including a new
regression test), and a debug `--validate-gpu --frames 100000` run inspected with screenshots and
synthetic clicks. Native VoiceOver traversal and the deferred GUI suite list in
[`QT_UI_PARITY.md`](../rust/aviqtl-slint/QT_UI_PARITY.md) still apply.

Severity: **High** = a Qt workflow is lost or behaves differently; **Medium** = extra steps,
misleading or unreachable UI; **Low** = cosmetic or wording only.

---

## High severity

### P1. Live parameter preview refreshed the wrong control
- Qt applies a live drag to the control under the pointer and repaints that control.
- Slint's `ObjectSettingsWindow::sync_parameter_preview` (`src/object_settings.rs`) found the
  refreshed row with `row.row_kind == current.row_kind` only. Every numeric control projects one
  `number` row plus one `keyframes` row, so dragging the second effect's numeric parameter replaced
  its label, value, range, and keyframe markers with the first numeric parameter in the stack.
- Fixed by matching kind **and** `effect_index`, `param_name`, and `audio_plugin`. A regression
  test builds two effects with one numeric control each and asserts that the matcher stays inside
  the edited control while the old kind-only rule would pick the sibling effect.

### P2. One-way widget bindings discarded later projections
- `object-controls.slint` bound `Slider.value`, `CheckBox.checked`, and `ComboBox.current-index`
  directly to `row-data`. Interaction assigns those properties inside the widget, which removes the
  binding, so the next projection (Undo, preset load, playhead-driven keyframe values) rendered
  stale state.
- Fixed by mirroring each projection through a private `projected` property and re-applying it in
  `changed projected`. The host callbacks already ignore unchanged values, so no feedback loop is
  introduced.

### P3. Trim handles answered only the left button
- Qt's `MouseArea` on a trim handle accepts the left button, so a right press over the handle fell
  through to the clip body and opened the clip context menu.
- A Slint `TouchArea` consumes every button, so the handles had no context menu at all.
- Fixed by forwarding a right press to `context-activate` with window coordinates, matching the
  clip body.

---

## Medium severity

### P4. Status messages were projected but never rendered
- `main.status-text`, `timeline.action-status`, and `object-settings.plugin-scan-status` were set by
  the host (`sync_transport`, plugin discovery) but no element displayed them. Refused commands and
  completed edits were silent, and an empty audio-plugin list had no explanation.
- Fixed by adding the `AppTheme.status-height` bar to the main window (the token existed but had no
  user) and by showing the scan status in the audio sidebar. The duplicate timeline property and
  its setter were removed.

### P5. Modal overlays could only be answered with the pointer
- Save confirmation, export cancel confirmation, export result, effect-preset save, color picker,
  and font picker were plain `Rectangle`s: no initial focus, no Escape, no Return.
- Converted to `FocusScope`s with `init => { self.focus(); }` (preset and font focus their text
  field) and `key-pressed` handling: Escape closes; save confirmation also accepts Return as Save.
  This follows the existing missing-media overlay pattern and Qt's `Dialog` behavior.

### P6. Ruler labels ignored the zoom
- `timeline.slint` used `duration / 60` ticks regardless of scale. Qt's `Ruler.qml:160-175` uses
  10/30/60/300 frames by scale, so labels overlapped when zoomed out and were sparse when zoomed in.
- Fixed with the same thresholds. The zoom text field also no longer overwrites itself while
  focused (`Ruler.qml:107-111`).

### P7. Package Manager offered Install for installed local-file plugins
- The `visible` predicate used `!can-remove || has-update`. A local-file plugin is installed but not
  removable, so it was offered for installation again.
- Fixed to Qt's `installed-version == "" || has-update` predicate
  (`PackageListView.qml:114`).

### P8. Speed multiplier clipped at the 640px default width
- Screenshot-confirmed: the 130px speed group forced the standard `SpinBox` into 80px, where the
  theme drew the lower spin button outside the box and over the `1.0x` label; the multiplier also
  ran into the window edge.
- Fixed by letting the spin box use its natural width, tightening the row spacing to 8px, and
  narrowing the transport audio meter to 58px. The 640px Qt default width (`MainWindow.qml:121`)
  now keeps the right padding. Qt shows the multiplier inside an 80px box; Slint's `SpinBox` has no
  text formatter, so the percent value and the multiplier label stay separate.

---

## Low severity

### P9. Untranslated literals
- `FPS:` (export, project, scene), `BPM:` (scene), `Image format`, `CRF:`, and the two new trim
  handle labels had no catalog entry. Added to both `zh_CN` and `ja_JP`; all `@tr` strings in the
  Slint sources are now covered (msgctxt entries included).

### P10. Accessible labels on form fields
- Export, project-settings, scene-settings, package-manager, object-settings, and system-settings
  fields that relied on adjacent text now carry explicit `accessible-label`s (format, image
  format, destination, codec, quality mode, CRF, bitrate, preset, profile, audio codec/bitrate,
  range start/end, width/height/FPS/sample rate, scene name, snap, grid mode, BPM, subdivision,
  offset, interval, search, repository URL, preset name, font search).
- The system-settings frame-rate label now uses the same 240px width as its neighbours.

---

## Deliberately not changed

- **Playback viewport follow and drag auto-scroll** (`TimelineView.qml:400-462`). They change
  pointer feel and need real-trackpad verification; still listed for the deferred native suite.
- **Multi-clip drag preview.** The command and commit path are correct; the preview shows only the
  anchor. This is a visual approximation, not a behavior difference.
- **Sidebar scroll-to-effect** and **sticky clip labels** — cosmetic.
- **Audio-plugin keyframe easing/interpolation** — no Rust command exists; belongs to a model task.
- **Empty-click seek.** Qt selects the layer and clears the clip selection but does not seek; the
  Slint handler also seeks. Changing it without confirming the intended Qt behavior would be a
  regression risk, so it is left as-is.
