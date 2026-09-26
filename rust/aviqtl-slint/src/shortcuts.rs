//! Window-scoped shortcuts and timeline navigation.

use crate::playback::AudioPlaybackRuntime;
use crate::projection::sync_weak_windows;
use crate::settings::timeline_last_layer;
use crate::{MainWindow, TimelineViewportData, TimelineWindow};
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::{ApplicationModel, WorkspaceModel};
use slint::ComponentHandle;
use slint::platform::Key;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShortcutAction {
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ExportMedia,
    Quit,
    SystemSettings,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Delete,
    Duplicate,
    PlayPause,
    NextFrame,
    PreviousFrame,
    JumpStart,
    JumpEnd,
    ZoomIn,
    ZoomOut,
    ShowTimeline,
    ShowObjectSettings,
    ProjectSettings,
    Split,
    MoveUp,
    MoveDown,
    NudgeLeft,
    NudgeRight,
    AddScene,
    SceneSettings,
    RemoveScene,
    ToggleLayerLock,
    ToggleLayerVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShortcutInput {
    pub(super) text: String,
    pub(super) alt: bool,
    pub(super) control: bool,
    pub(super) shift: bool,
    pub(super) meta: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShortcutPattern {
    pub(super) text: String,
    pub(super) alt: bool,
    pub(super) control: bool,
    pub(super) shift: bool,
    pub(super) meta: bool,
    pub(super) ignore_shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TimelineScrollInput {
    pub(super) delta_x: f32,
    pub(super) delta_y: f32,
    pub(super) content_x: f32,
    pub(super) viewport_x: f32,
    pub(super) viewport_y: f32,
    pub(super) visible_width: f32,
    pub(super) viewport_width: f32,
    pub(super) visible_height: f32,
    pub(super) viewport_height: f32,
    pub(super) duration_frames: f32,
    pub(super) alt: bool,
    pub(super) control: bool,
    pub(super) shift: bool,
    pub(super) ruler_zoom: bool,
    pub(super) pixels_per_frame: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TimelineScrollPlan {
    pub(super) pixels_per_frame: f32,
    pub(super) viewport_x: f32,
    pub(super) viewport_y: f32,
}

pub(super) fn install_keyboard_shortcuts(
    main: &MainWindow,
    timeline: &TimelineWindow,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    audio_playback: Rc<RefCell<AudioPlaybackRuntime>>,
) {
    let main_window = main.as_weak();
    let main_timeline = timeline.as_weak();
    let main_model = model.clone();
    let main_settings = settings.clone();
    let main_audio = audio_playback.clone();
    main.on_keyboard_shortcut(move |text, alt, control, shift, meta| {
        handle_keyboard_shortcut(
            ShortcutInput {
                text: text.to_string(),
                alt,
                control,
                shift,
                meta,
            },
            true,
            false,
            &main_window,
            &main_timeline,
            &main_model,
            &main_settings,
            &main_audio,
        )
    });

    let timeline_main = main.as_weak();
    let timeline_window = timeline.as_weak();
    let timeline_model = model.clone();
    let timeline_settings = settings.clone();
    let timeline_audio = audio_playback;
    timeline.on_keyboard_shortcut(move |text, alt, control, shift, meta| {
        handle_keyboard_shortcut(
            ShortcutInput {
                text: text.to_string(),
                alt,
                control,
                shift,
                meta,
            },
            true,
            true,
            &timeline_main,
            &timeline_window,
            &timeline_model,
            &timeline_settings,
            &timeline_audio,
        )
    });

    let scroll_window = timeline.as_weak();
    let scroll_settings = settings.clone();
    timeline.on_timeline_scrolled(
        move |delta_x,
              delta_y,
              content_x,
              viewport_x,
              viewport_y,
              visible_width,
              viewport_width,
              visible_height,
              viewport_height,
              duration_frames,
              alt,
              control,
              shift,
              ruler_zoom| {
            let pixels_per_frame = scroll_window
                .upgrade()
                .map_or(1.0, |window| window.get_pixels_per_frame());
            let plan = plan_timeline_scroll(
                TimelineScrollInput {
                    delta_x,
                    delta_y,
                    content_x,
                    viewport_x,
                    viewport_y,
                    visible_width,
                    viewport_width,
                    visible_height,
                    viewport_height,
                    duration_frames,
                    alt,
                    control,
                    shift,
                    ruler_zoom,
                    pixels_per_frame,
                },
                &scroll_settings.borrow(),
            );
            TimelineViewportData {
                pixels_per_frame: plan.pixels_per_frame,
                viewport_x: plan.viewport_x,
                viewport_y: plan.viewport_y,
            }
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn handle_keyboard_shortcut(
    input: ShortcutInput,
    editor_window: bool,
    use_skimmer: bool,
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
    settings: &Rc<RefCell<SettingsStore>>,
    audio_playback: &Rc<RefCell<AudioPlaybackRuntime>>,
) -> bool {
    let action = {
        let settings = settings.borrow();
        configured_shortcut_action(&settings, &input, editor_window)
    };
    let Some(action) = action else {
        return false;
    };
    dispatch_shortcut(
        action,
        use_skimmer,
        main,
        timeline,
        model,
        settings,
        audio_playback,
    );
    true
}

fn configured_shortcut_action(
    settings: &SettingsStore,
    input: &ShortcutInput,
    editor_window: bool,
) -> Option<ShortcutAction> {
    const BINDINGS: [(ShortcutAction, &str, &str, bool); 34] = [
        (ShortcutAction::NewProject, "project.new", "Ctrl+N", false),
        (ShortcutAction::OpenProject, "project.open", "Ctrl+O", false),
        (
            ShortcutAction::SystemSettings,
            "app.settings",
            "Ctrl+P",
            false,
        ),
        (ShortcutAction::Quit, "app.quit", "Ctrl+Q", false),
        (ShortcutAction::SaveProject, "project.save", "Ctrl+S", false),
        (
            ShortcutAction::SaveProjectAs,
            "project.saveAs",
            "Ctrl+Shift+S",
            false,
        ),
        (
            ShortcutAction::ProjectSettings,
            "project.settings",
            "Alt+Enter",
            false,
        ),
        (
            ShortcutAction::ExportMedia,
            "project.export",
            "Ctrl+E",
            false,
        ),
        (ShortcutAction::ShowTimeline, "view.timeline", "F3", false),
        (
            ShortcutAction::ShowObjectSettings,
            "view.objectSettings",
            "F4",
            false,
        ),
        (ShortcutAction::ZoomIn, "view.zoomIn", "Ctrl++", false),
        (ShortcutAction::ZoomOut, "view.zoomOut", "Ctrl+-", false),
        (ShortcutAction::Undo, "edit.undo", "Ctrl+Z", false),
        (ShortcutAction::Redo, "edit.redo", "Ctrl+Shift+Z", false),
        (ShortcutAction::Copy, "edit.copy", "Ctrl+C", true),
        (ShortcutAction::Cut, "edit.cut", "Ctrl+X", true),
        (ShortcutAction::Paste, "edit.paste", "Ctrl+V", true),
        (ShortcutAction::Duplicate, "edit.duplicate", "Ctrl+D", true),
        (ShortcutAction::Delete, "edit.delete", "Delete", true),
        (ShortcutAction::Split, "timeline.split", "S", true),
        (ShortcutAction::MoveUp, "timeline.moveUp", "Alt+Up", true),
        (
            ShortcutAction::MoveDown,
            "timeline.moveDown",
            "Alt+Down",
            true,
        ),
        (
            ShortcutAction::NudgeLeft,
            "timeline.nudgeLeft",
            "Alt+Left",
            true,
        ),
        (
            ShortcutAction::NudgeRight,
            "timeline.nudgeRight",
            "Alt+Right",
            true,
        ),
        (
            ShortcutAction::ToggleLayerLock,
            "timeline.layerLock",
            "Ctrl+L",
            true,
        ),
        (
            ShortcutAction::ToggleLayerVisibility,
            "timeline.layerHide",
            "Ctrl+H",
            true,
        ),
        (
            ShortcutAction::AddScene,
            "timeline.addScene",
            "Ctrl+T",
            false,
        ),
        (
            ShortcutAction::SceneSettings,
            "timeline.sceneSettings",
            "Alt+S",
            true,
        ),
        (
            ShortcutAction::RemoveScene,
            "timeline.removeScene",
            "Ctrl+Shift+Delete",
            true,
        ),
        (
            ShortcutAction::PlayPause,
            "transport.playPause",
            "Space",
            false,
        ),
        (
            ShortcutAction::PreviousFrame,
            "transport.prevFrame",
            "Left",
            false,
        ),
        (
            ShortcutAction::NextFrame,
            "transport.nextFrame",
            "Right",
            false,
        ),
        (
            ShortcutAction::JumpStart,
            "transport.jumpStart",
            "Home",
            false,
        ),
        (ShortcutAction::JumpEnd, "transport.jumpEnd", "End", false),
    ];

    BINDINGS
        .iter()
        .find_map(|(action, key, fallback, editor_only)| {
            if *editor_only && !editor_window {
                return None;
            }
            let configured = shortcut_setting(settings, key, fallback);
            shortcut_matches(&configured, input).then_some(*action)
        })
}

pub(super) fn shortcut_setting(settings: &SettingsStore, key: &str, fallback: &str) -> String {
    configured_shortcut(settings.value("shortcuts"), key, fallback).to_owned()
}

pub(super) fn configured_shortcut<'a>(
    shortcuts: Option<&'a serde_json::Value>,
    key: &str,
    fallback: &'a str,
) -> &'a str {
    // An explicit empty string disables the binding; only missing/invalid values use defaults.
    shortcuts
        .and_then(|values| values.get(key))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
}

pub(super) fn shortcut_matches(value: &str, input: &ShortcutInput) -> bool {
    let Some(pattern) = parse_shortcut(value) else {
        return false;
    };
    pattern.text == input.text.to_lowercase()
        && pattern.alt == input.alt
        && pattern.control == input.control
        && pattern.meta == input.meta
        && (pattern.ignore_shift || pattern.shift == input.shift)
}

pub(super) fn native_shortcut(value: &str) -> slint::Keys {
    let Some(pattern) = parse_shortcut(value) else {
        return slint::Keys::default();
    };
    let mut parts = Vec::new();
    if pattern.control {
        parts.push("Control");
    }
    if pattern.meta {
        parts.push("Meta");
    }
    if pattern.alt {
        parts.push("Alt");
    }
    if pattern.ignore_shift {
        parts.push("Shift?");
    } else if pattern.shift {
        parts.push("Shift");
    }
    parts.push(&pattern.text);
    slint::Keys::from_parts(parts).unwrap_or_default()
}

pub(super) fn record_shortcut(input: ShortcutInput) -> Option<String> {
    let named = [
        (Key::LeftArrow, "Left"),
        (Key::RightArrow, "Right"),
        (Key::UpArrow, "Up"),
        (Key::DownArrow, "Down"),
        (Key::Tab, "Tab"),
        (Key::Return, "Enter"),
        (Key::Space, "Space"),
        (Key::Backspace, "Backspace"),
        (Key::Delete, "Delete"),
        (Key::Home, "Home"),
        (Key::End, "End"),
        (Key::PageUp, "PageUp"),
        (Key::PageDown, "PageDown"),
        (Key::F1, "F1"),
        (Key::F2, "F2"),
        (Key::F3, "F3"),
        (Key::F4, "F4"),
        (Key::F5, "F5"),
        (Key::F6, "F6"),
        (Key::F7, "F7"),
        (Key::F8, "F8"),
        (Key::F9, "F9"),
        (Key::F10, "F10"),
        (Key::F11, "F11"),
        (Key::F12, "F12"),
    ];
    let key = named
        .iter()
        .find(|(key, _)| slint::SharedString::from(*key).as_str() == input.text)
        .map(|(_, name)| (*name).to_owned())
        .or_else(|| {
            let mut chars = input.text.chars();
            let ch = chars.next()?;
            (chars.next().is_none()
                && !ch.is_control()
                && !(('\u{e000}'..='\u{f8ff}').contains(&ch)))
            .then(|| input.text.to_uppercase())
        })?;
    let mut parts = Vec::new();
    if input.control {
        parts.push("Ctrl".to_owned());
    }
    if input.meta {
        parts.push("Meta".to_owned());
    }
    if input.alt {
        parts.push("Alt".to_owned());
    }
    if input.shift && key != "+" {
        parts.push("Shift".to_owned());
    }
    parts.push(key);
    let value = parts.join("+");
    parse_shortcut(&value).map(|_| value)
}

pub(super) fn validate_shortcuts(values: &[String]) -> Result<(), &'static str> {
    let mut patterns: Vec<ShortcutPattern> = Vec::new();
    for value in values.iter().filter(|value| !value.trim().is_empty()) {
        let pattern = parse_shortcut(value).ok_or(crate::localization::localized(
            "Invalid shortcut. Record a key combination or clear the binding.",
            "快捷键无效。请录入组合键或清除绑定。",
            "無効なショートカットです。キーを記録するか、割り当てを解除してください。",
        ))?;
        if patterns.iter().any(|other| {
            other.text == pattern.text
                && other.alt == pattern.alt
                && other.control == pattern.control
                && other.meta == pattern.meta
                && (other.ignore_shift || pattern.ignore_shift || other.shift == pattern.shift)
        }) {
            return Err(crate::localization::localized(
                "A shortcut is assigned to more than one command.",
                "同一快捷键被分配给了多个命令。",
                "同じショートカットが複数のコマンドに割り当てられています。",
            ));
        }
        patterns.push(pattern);
    }
    Ok(())
}

pub(super) fn parse_shortcut(value: &str) -> Option<ShortcutPattern> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let plus_key = value.ends_with('+');
    let mut parts = value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let key_name = if plus_key { "+" } else { parts.pop()? };
    let mut pattern = ShortcutPattern {
        text: String::new(),
        alt: false,
        control: false,
        shift: false,
        meta: false,
        ignore_shift: key_name == "+",
    };
    for modifier in parts {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" => pattern.control = true,
            "cmd" | "command" => {
                if cfg!(target_os = "macos") {
                    pattern.control = true;
                } else {
                    pattern.meta = true;
                }
            }
            "control" => {
                if cfg!(target_os = "macos") {
                    pattern.meta = true;
                } else {
                    pattern.control = true;
                }
            }
            "meta" | "super" => pattern.meta = true,
            "alt" | "option" => pattern.alt = true,
            "shift" => pattern.shift = true,
            _ => return None,
        }
    }
    pattern.text = shortcut_key_text(key_name)?;
    Some(pattern)
}

fn shortcut_key_text(value: &str) -> Option<String> {
    let named = match value.trim().to_ascii_lowercase().as_str() {
        "left" | "arrowleft" => Some(Key::LeftArrow),
        "right" | "arrowright" => Some(Key::RightArrow),
        "up" | "arrowup" => Some(Key::UpArrow),
        "down" | "arrowdown" => Some(Key::DownArrow),
        "escape" | "esc" => Some(Key::Escape),
        "tab" => Some(Key::Tab),
        "backspace" => Some(Key::Backspace),
        "enter" | "return" => Some(Key::Return),
        "space" => Some(Key::Space),
        "delete" | "del" => Some(Key::Delete),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        _ => None,
    };
    if let Some(key) = named {
        return Some(char::from(key).to_string());
    }
    let text = value.trim().to_lowercase();
    (text.chars().count() == 1).then_some(text)
}

#[allow(clippy::too_many_arguments)]
fn dispatch_shortcut(
    action: ShortcutAction,
    use_skimmer: bool,
    main: &slint::Weak<MainWindow>,
    timeline: &slint::Weak<TimelineWindow>,
    model: &Rc<RefCell<ApplicationModel>>,
    settings: &Rc<RefCell<SettingsStore>>,
    audio_playback: &Rc<RefCell<AudioPlaybackRuntime>>,
) {
    let Some(main_window) = main.upgrade() else {
        return;
    };
    let Some(timeline_window) = timeline.upgrade() else {
        return;
    };
    match action {
        ShortcutAction::NewProject => main_window.invoke_new_project(),
        ShortcutAction::OpenProject => main_window.invoke_open_project(),
        ShortcutAction::SaveProject => main_window.invoke_save_project(),
        ShortcutAction::SaveProjectAs => main_window.invoke_save_project_as(),
        ShortcutAction::ExportMedia => main_window.invoke_export_media(),
        ShortcutAction::Quit => main_window.invoke_quit_requested(),
        ShortcutAction::SystemSettings => main_window.invoke_show_system_settings(),
        ShortcutAction::Undo => main_window.invoke_undo(),
        ShortcutAction::Redo => main_window.invoke_redo(),
        ShortcutAction::PlayPause => main_window.invoke_toggle_playback(),
        ShortcutAction::PreviousFrame => main_window.invoke_previous_frame(),
        ShortcutAction::NextFrame => main_window.invoke_next_frame(),
        ShortcutAction::ShowTimeline => main_window.invoke_show_timeline(),
        ShortcutAction::ShowObjectSettings => main_window.invoke_show_object_settings(),
        ShortcutAction::ProjectSettings => main_window.invoke_show_project_settings(),
        ShortcutAction::AddScene => timeline_window.invoke_show_scene_settings(),
        ShortcutAction::SceneSettings => {
            if let Some(scene_id) = model
                .borrow()
                .current_workspace()
                .map(WorkspaceModel::selected_scene)
            {
                timeline_window.invoke_scene_settings(scene_id);
            }
        }
        ShortcutAction::ZoomIn | ShortcutAction::ZoomOut => {
            let direction = if action == ShortcutAction::ZoomIn {
                1
            } else {
                -1
            };
            step_timeline_zoom(&timeline_window, &settings.borrow(), direction);
        }
        ShortcutAction::Copy
        | ShortcutAction::Cut
        | ShortcutAction::Paste
        | ShortcutAction::Delete
        | ShortcutAction::Duplicate
        | ShortcutAction::JumpStart
        | ShortcutAction::JumpEnd
        | ShortcutAction::Split
        | ShortcutAction::MoveUp
        | ShortcutAction::MoveDown
        | ShortcutAction::NudgeLeft
        | ShortcutAction::NudgeRight
        | ShortcutAction::RemoveScene
        | ShortcutAction::ToggleLayerLock
        | ShortcutAction::ToggleLayerVisibility => {
            if let Some(workspace) = model.borrow_mut().current_workspace_mut() {
                let skimmer_targets = use_skimmer && timeline_window.get_skimmer_visible();
                let maximum_layers = timeline_window.get_maximum_layers();
                let selected_layer = workspace.selected_layer();
                let frame = if skimmer_targets {
                    timeline_window.get_skimmer_frame()
                } else {
                    workspace.playhead()
                };
                let layer = if skimmer_targets {
                    timeline_window.get_skimmer_layer()
                } else {
                    selected_layer
                };
                match action {
                    ShortcutAction::Copy => {
                        workspace.copy_selected_clips();
                    }
                    ShortcutAction::Cut => {
                        workspace.cut_selected_clips();
                    }
                    ShortcutAction::Paste => {
                        if let Some((next_frame, next_layer)) =
                            workspace.paste_clips_at(frame, layer, maximum_layers)
                        {
                            advance_shortcut_target(
                                workspace,
                                &timeline_window,
                                skimmer_targets,
                                next_frame,
                                next_layer,
                            );
                        }
                    }
                    ShortcutAction::Delete => {
                        workspace.remove_selected_clips();
                    }
                    ShortcutAction::Duplicate => {
                        if let Some((next_frame, next_layer)) =
                            workspace.duplicate_selected_clips_at(frame, layer, maximum_layers)
                        {
                            advance_shortcut_target(
                                workspace,
                                &timeline_window,
                                skimmer_targets,
                                next_frame,
                                next_layer,
                            );
                        }
                    }
                    ShortcutAction::JumpStart => {
                        workspace.seek(0);
                        audio_playback.borrow_mut().reset_queue();
                    }
                    ShortcutAction::JumpEnd => {
                        let end_frame = workspace.timeline_duration();
                        workspace.seek(end_frame);
                        audio_playback.borrow_mut().reset_queue();
                    }
                    ShortcutAction::Split => {
                        workspace.split_selected_clips_at(frame);
                    }
                    ShortcutAction::MoveUp => {
                        workspace.move_selected_clips(-1, 0, maximum_layers);
                    }
                    ShortcutAction::MoveDown => {
                        workspace.move_selected_clips(1, 0, maximum_layers);
                    }
                    ShortcutAction::NudgeLeft => {
                        workspace.move_selected_clips(0, -1, maximum_layers);
                    }
                    ShortcutAction::NudgeRight => {
                        workspace.move_selected_clips(0, 1, maximum_layers);
                    }
                    ShortcutAction::RemoveScene => {
                        workspace.remove_scene(workspace.selected_scene());
                    }
                    ShortcutAction::ToggleLayerLock => {
                        workspace.toggle_layer_lock(selected_layer);
                    }
                    ShortcutAction::ToggleLayerVisibility => {
                        workspace.toggle_layer_visibility(selected_layer);
                    }
                    _ => unreachable!("direct timeline shortcut is exhaustively matched"),
                }
            }
            sync_weak_windows(main, timeline, model);
        }
    }
}

fn advance_shortcut_target(
    workspace: &mut WorkspaceModel,
    timeline: &TimelineWindow,
    skimmer_targets: bool,
    frame: i32,
    layer: i32,
) {
    if skimmer_targets {
        timeline.set_skimmer_frame(frame.max(0));
        timeline.set_skimmer_layer(layer.clamp(0, timeline_last_layer(timeline)));
    } else {
        workspace.set_edit_target(frame, layer);
    }
}

fn step_timeline_zoom(window: &TimelineWindow, settings: &SettingsStore, direction: i32) {
    window.set_pixels_per_frame(stepped_timeline_scale(
        window.get_pixels_per_frame(),
        settings,
        direction,
    ));
}

fn stepped_timeline_scale(current_scale: f32, settings: &SettingsStore, direction: i32) -> f32 {
    stepped_timeline_scale_with(current_scale, direction, timeline_zoom_settings(settings))
}

fn stepped_timeline_scale_with(
    current_scale: f32,
    direction: i32,
    (minimum, maximum, step): (f32, f32, f32),
) -> f32 {
    let current = scale_to_zoom_percent(current_scale);
    let next = (current + direction.signum() as f32 * step).clamp(minimum, maximum);
    zoom_percent_to_scale(next)
}

fn plan_timeline_scroll(
    input: TimelineScrollInput,
    settings: &SettingsStore,
) -> TimelineScrollPlan {
    plan_timeline_scroll_with(input, timeline_zoom_settings(settings))
}

pub(super) fn plan_timeline_scroll_with(
    input: TimelineScrollInput,
    zoom_settings: (f32, f32, f32),
) -> TimelineScrollPlan {
    let mut plan = TimelineScrollPlan {
        pixels_per_frame: input.pixels_per_frame,
        viewport_x: input.viewport_x,
        viewport_y: input.viewport_y,
    };
    let dominant_delta = if input.delta_x.abs() > input.delta_y.abs() {
        input.delta_x
    } else {
        input.delta_y
    };
    if dominant_delta.abs() <= f32::EPSILON {
        return plan;
    }

    if input.ruler_zoom || input.alt || input.control {
        let direction = if dominant_delta > 0.0 { 1 } else { -1 };
        let new_scale = if input.ruler_zoom {
            let (minimum, maximum, _) = zoom_settings;
            (input.pixels_per_frame * if direction > 0 { 1.1 } else { 0.9 }).clamp(
                zoom_percent_to_scale(minimum),
                zoom_percent_to_scale(maximum),
            )
        } else {
            stepped_timeline_scale_with(input.pixels_per_frame, direction, zoom_settings)
        };
        let mouse_x = input.content_x + input.viewport_x;
        let anchor_frame = input.content_x / input.pixels_per_frame.max(f32::EPSILON);
        let new_content_x = anchor_frame * new_scale - mouse_x;
        plan.pixels_per_frame = new_scale;
        plan.viewport_x = clamp_viewport(
            -new_content_x,
            input.visible_width,
            (input.duration_frames * new_scale).max(input.visible_width),
        );
    } else if input.shift {
        plan.viewport_y = clamp_viewport(
            input.viewport_y + input.delta_y,
            input.visible_height,
            input.viewport_height,
        );
    } else {
        plan.viewport_x = clamp_viewport(
            input.viewport_x + dominant_delta,
            input.visible_width,
            input.viewport_width,
        );
    }
    plan
}

pub(super) fn timeline_zoom_settings(settings: &SettingsStore) -> (f32, f32, f32) {
    let minimum = settings.i32_value("timelineZoomMin", 10).clamp(1, 400) as f32;
    let maximum = settings
        .i32_value("timelineZoomMax", 400)
        .clamp(minimum as i32, 1_000) as f32;
    let step = settings.i32_value("timelineZoomStep", 10).clamp(1, 100) as f32;
    (minimum, maximum, step)
}

pub(super) fn sync_timeline_zoom_settings(window: &TimelineWindow, settings: &SettingsStore) {
    let (minimum, maximum, _) = timeline_zoom_settings(settings);
    window.set_zoom_min(minimum.round() as i32);
    window.set_zoom_max(maximum.round() as i32);
    let current = scale_to_zoom_percent(window.get_pixels_per_frame()).clamp(minimum, maximum);
    window.set_pixels_per_frame(zoom_percent_to_scale(current));
}

fn clamp_viewport(value: f32, visible: f32, viewport: f32) -> f32 {
    value.clamp((visible - viewport).min(0.0), 0.0)
}

pub(super) fn zoom_percent_to_scale(percent: f32) -> f32 {
    let percent = percent.max(1.0);
    if percent <= 100.0 {
        percent / 100.0
    } else {
        1.0 + (percent - 100.0) * 9.0 / 300.0
    }
}

pub(super) fn fit_timeline_range(start: i32, end: i32, width: f32) -> TimelineViewportData {
    let width = if width.is_finite() {
        width.max(1.0)
    } else {
        1.0
    };
    let span = (i64::from(end) - i64::from(start)).max(1) as f64;
    let scale = ((f64::from(width) - 32.0).max(1.0) / span).clamp(0.0001, 10.0) as f32;
    TimelineViewportData {
        pixels_per_frame: scale,
        viewport_x: -(start.max(0) as f32 * scale - 16.0).max(0.0),
        viewport_y: 0.0,
    }
}

pub(super) fn scale_to_zoom_percent(scale: f32) -> f32 {
    if scale <= 1.0 {
        scale * 100.0
    } else {
        100.0 + (scale - 1.0) * 300.0 / 9.0
    }
}
