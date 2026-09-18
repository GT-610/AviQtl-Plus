//! Native dialogs, value pickers, and persisted window placement.

use crate::ObjectSettingsWindow;
use crate::localization::localized;
use crate::projection::update_vec_model;
use aviqtl_app::settings::SettingsStore;
use slint::winit_030::WinitWindowAccessor;
use slint::{CloseRequestResponse, Color, ComponentHandle, SharedString, Timer};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowGeometry {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) maximized: bool,
}

impl WindowGeometry {
    pub(super) const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    pub(super) fn load(settings: &SettingsStore, id: &str, fallback: Self) -> Self {
        let key = format!("windowGeometry_{id}");
        Self::from_value(settings.value(&key), fallback)
    }

    pub(super) fn from_value(value: Option<&serde_json::Value>, fallback: Self) -> Self {
        let Some(value) = value.and_then(serde_json::Value::as_object) else {
            return fallback;
        };
        Self {
            x: json_i32(value.get("x"), fallback.x),
            y: json_i32(value.get("y"), fallback.y),
            width: json_i32(value.get("width"), fallback.width).max(1),
            height: json_i32(value.get("height"), fallback.height).max(1),
            maximized: value
                .get("maximized")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(fallback.maximized),
        }
    }

    pub(super) fn capture(window: &slint::Window) -> Self {
        let scale_factor = window.scale_factor().max(f32::EPSILON);
        let position = window.position().to_logical(scale_factor);
        let size = window.size().to_logical(scale_factor);
        Self {
            x: position.x.round() as i32,
            y: position.y.round() as i32,
            width: size.width.round().max(1.0) as i32,
            height: size.height.round().max(1.0) as i32,
            maximized: window.is_maximized(),
        }
    }

    pub(super) fn json(self) -> serde_json::Value {
        serde_json::json!({
            "x": self.x,
            "y": self.y,
            "width": self.width,
            "height": self.height,
            "maximized": self.maximized,
        })
    }
}

pub(super) fn pick_parameter_file(current_path: &str, filter: &str, label: &str) -> Option<String> {
    let mut dialog = rfd::FileDialog::new().set_title(if label.is_empty() {
        localized("Choose a file", "选择文件", "ファイルを選択")
    } else {
        label
    });
    for (name, extensions) in qt_file_filters(filter) {
        dialog = dialog.add_filter(name, &extensions);
    }
    dialog = dialog.add_filter("All Files", &["*"]);

    let current_path = PathBuf::from(current_path.trim());
    if let Some(parent) = current_path.parent().filter(|path| path.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    if let Some(name) = current_path.file_name().filter(|name| !name.is_empty()) {
        dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
    }
    dialog
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

pub(super) fn qt_file_filters(value: &str) -> Vec<(String, Vec<String>)> {
    value
        .split(";;")
        .filter_map(|filter| {
            let filter = filter.trim();
            let (name, patterns) = filter.split_once('(')?;
            let patterns = patterns.strip_suffix(')')?;
            let extensions = patterns
                .split_whitespace()
                .filter_map(|pattern| {
                    let extension = pattern
                        .trim()
                        .strip_prefix("*.")
                        .or_else(|| (pattern.trim() == "*").then_some("*"))?;
                    (!extension.is_empty()).then(|| extension.to_owned())
                })
                .collect::<Vec<_>>();
            (!extensions.is_empty()).then(|| (name.trim().to_owned(), extensions))
        })
        .collect()
}

pub(super) fn parse_qt_color(value: &str) -> [u8; 4] {
    let hex = value.trim().strip_prefix('#').unwrap_or_default();
    let nibble = |value: u8| value.saturating_mul(17);
    match hex.len() {
        3 => {
            let bytes = hex.as_bytes();
            let Some(red) = hex_nibble(bytes[0]) else {
                return [255; 4];
            };
            let Some(green) = hex_nibble(bytes[1]) else {
                return [255; 4];
            };
            let Some(blue) = hex_nibble(bytes[2]) else {
                return [255; 4];
            };
            [255, nibble(red), nibble(green), nibble(blue)]
        }
        4 => {
            let bytes = hex.as_bytes();
            let Some(alpha) = hex_nibble(bytes[0]) else {
                return [255; 4];
            };
            let Some(red) = hex_nibble(bytes[1]) else {
                return [255; 4];
            };
            let Some(green) = hex_nibble(bytes[2]) else {
                return [255; 4];
            };
            let Some(blue) = hex_nibble(bytes[3]) else {
                return [255; 4];
            };
            [nibble(alpha), nibble(red), nibble(green), nibble(blue)]
        }
        6 => parse_hex_bytes(hex).map_or([255; 4], |bytes| [255, bytes[0], bytes[1], bytes[2]]),
        8 => {
            parse_hex_bytes(hex).map_or([255; 4], |bytes| [bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        _ => [255; 4],
    }
}

fn hex_nibble(value: u8) -> Option<u8> {
    (value as char).to_digit(16).map(|value| value as u8)
}

fn parse_hex_bytes(value: &str) -> Option<Vec<u8>> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|digits| {
            let digits = std::str::from_utf8(digits).ok()?;
            u8::from_str_radix(digits, 16).ok()
        })
        .collect()
}

pub(super) fn format_qt_color(red: i32, green: i32, blue: i32, alpha: i32) -> String {
    let [red, green, blue, alpha] = [red, green, blue, alpha].map(|value| value.clamp(0, 255));
    if alpha == 255 {
        format!("#{red:02x}{green:02x}{blue:02x}")
    } else {
        format!("#{alpha:02x}{red:02x}{green:02x}{blue:02x}")
    }
}

pub(super) fn slint_color(value: &str) -> Color {
    let [alpha, red, green, blue] = parse_qt_color(value);
    Color::from_argb_u8(alpha, red, green, blue)
}

pub(super) fn system_font_families() -> Vec<String> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    database
        .faces()
        .flat_map(|face| face.families.iter().map(|(family, _)| family.trim()))
        .filter(|family| !family.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn filtered_font_families(families: &[String], query: &str) -> Vec<SharedString> {
    let query = query.trim().to_lowercase();
    families
        .iter()
        .filter(|family| query.is_empty() || family.to_lowercase().contains(&query))
        .map(|family| SharedString::from(family.clone()))
        .collect()
}

pub(super) fn sync_font_families(window: &ObjectSettingsWindow, families: &[String], query: &str) {
    update_vec_model(
        &window.get_font_families(),
        filtered_font_families(families, query),
    );
}

pub(super) fn choose_project_to_open() -> Option<PathBuf> {
    project_file_dialog().pick_file()
}

pub(super) fn choose_project_save_path(suggested_path: &Path) -> Option<PathBuf> {
    let mut dialog = project_file_dialog();
    if let Some(file_name) = suggested_path.file_name() {
        dialog = dialog.set_file_name(file_name.to_string_lossy().into_owned());
    }
    if let Some(parent) = suggested_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        dialog = dialog.set_directory(parent);
    }
    let mut path = dialog.save_file()?;
    if path.extension().is_none() {
        path.set_extension("aviqtl");
    }
    Some(path)
}

fn project_file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("AviQtl Plus Project files", &["aviqtl"])
        .add_filter("JSON files", &["json"])
}

pub(super) fn json_i32(value: Option<&serde_json::Value>, fallback: i32) -> i32 {
    value
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .or_else(|| {
            value
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .map(|value| value.round() as i32)
        })
        .unwrap_or(fallback)
}

pub(super) fn restore_window_geometry(
    window: &slint::Window,
    settings: &SettingsStore,
    id: &str,
    fallback: WindowGeometry,
) {
    let geometry = WindowGeometry::load(settings, id, fallback);
    window.set_size(slint::LogicalSize::new(
        geometry.width as f32,
        geometry.height as f32,
    ));
    window.set_position(slint::LogicalPosition::new(
        geometry.x as f32,
        geometry.y as f32,
    ));
    window.set_maximized(geometry.maximized);
}

pub(super) fn persist_window_geometry(
    settings: &Rc<RefCell<SettingsStore>>,
    id: &str,
    window: &slint::Window,
) {
    let mut replacement = settings.borrow().snapshot();
    replacement.insert(
        format!("windowGeometry_{id}"),
        WindowGeometry::capture(window).json(),
    );
    if let Err(error) = settings.borrow_mut().apply(replacement) {
        eprintln!("Failed to save {id} window geometry: {error}");
    }
}

pub(super) fn insert_visible_window_geometry<T: ComponentHandle + 'static>(
    replacement: &mut serde_json::Map<String, serde_json::Value>,
    id: &str,
    window: &slint::Weak<T>,
) -> bool {
    let Some(window) = window.upgrade() else {
        return false;
    };
    if !window.window().is_visible() {
        return false;
    }
    replacement.insert(
        format!("windowGeometry_{id}"),
        WindowGeometry::capture(window.window()).json(),
    );
    true
}

pub(super) fn install_window_geometry_close_handler<T: ComponentHandle + 'static>(
    window: slint::Weak<T>,
    settings: Rc<RefCell<SettingsStore>>,
    id: &'static str,
) {
    let Some(component) = window.upgrade() else {
        return;
    };
    component.window().on_close_requested(move || {
        if let Some(component) = window.upgrade() {
            persist_window_geometry(&settings, id, component.window());
        }
        CloseRequestResponse::HideWindow
    });
}

pub(super) fn choose_missing_media_replacement(
    clip_type: &str,
    suggested_path: &Path,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(localized(
        "Replace missing media",
        "替换缺失媒体",
        "不足しているメディアを置換",
    ));
    dialog = match clip_type {
        "audio" => dialog.add_filter("Audio files", &["wav", "mp3", "aac", "m4a", "flac", "ogg"]),
        "image" => dialog.add_filter(
            "Image files",
            &["png", "jpg", "jpeg", "bmp", "gif", "webp", "svg"],
        ),
        "video" => dialog.add_filter("Video files", &["mp4", "mov", "avi", "mkv", "webm", "wmv"]),
        _ => return None,
    };
    if let Some(parent) = suggested_path.parent().filter(|parent| parent.exists()) {
        dialog = dialog.set_directory(parent);
    }
    if let Some(name) = suggested_path.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy().into_owned());
    }
    dialog.pick_file()
}

pub(super) fn show_and_redraw<T: ComponentHandle + 'static>(
    window: &T,
) -> Result<(), slint::PlatformError> {
    window.show()?;
    let window = window.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        if let Some(window) = window.upgrade() {
            window.window().request_redraw();
            let _ = window
                .window()
                .with_winit_window(|winit_window| winit_window.focus_window());
        }
    });
    Ok(())
}

pub(super) fn show_centered_and_redraw<T, P>(
    window: &T,
    parent: &P,
) -> Result<(), slint::PlatformError>
where
    T: ComponentHandle + 'static,
    P: ComponentHandle + 'static,
{
    window.show()?;
    let window = window.as_weak();
    let parent = parent.as_weak();
    Timer::single_shot(Duration::ZERO, move || {
        let (Some(window), Some(parent)) = (window.upgrade(), parent.upgrade()) else {
            return;
        };
        let parent_position = parent.window().position();
        let parent_size = parent.window().size();
        let window_size = window.window().size();
        let centered_coordinate = |origin: i32, parent: u32, child: u32| {
            (i64::from(origin) + (i64::from(parent) - i64::from(child)) / 2)
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        };
        window.window().set_position(slint::PhysicalPosition::new(
            centered_coordinate(parent_position.x, parent_size.width, window_size.width),
            centered_coordinate(parent_position.y, parent_size.height, window_size.height),
        ));
        window.window().request_redraw();
    });
    Ok(())
}

pub(super) fn show_error_dialog(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("AviQtl Plus")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
