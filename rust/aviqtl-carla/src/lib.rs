//! Safe ownership wrapper around the optional Carla Native Rack ABI.
//!
//! All Carla and dynamic-library unsafety lives in this crate so the editor's
//! audio planner and processing chain can continue to forbid unsafe code.

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

const PLUGIN_OPTIONS_NULL: u32 = 0x1_0000;
const PLUGIN_LADSPA: c_int = 2;
const PLUGIN_DSSI: c_int = 3;
const PLUGIN_LV2: c_int = 4;
const PLUGIN_VST2: c_int = 5;
const PLUGIN_VST3: c_int = 6;

#[cfg(target_os = "windows")]
const BINARY_NATIVE_64: c_int = 4;
#[cfg(not(target_os = "windows"))]
const BINARY_NATIVE_64: c_int = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarlaLibraryPaths {
    pub native_plugin: PathBuf,
    pub host_plugin: PathBuf,
    pub resource_dir: PathBuf,
}

impl CarlaLibraryPaths {
    /// Finds a matching Native Rack and host-API pair beside Carla discovery,
    /// beside the application, or in the platform's conventional locations.
    pub fn discover(discovery_tool: Option<&Path>) -> Option<Self> {
        let mut directories = Vec::new();
        if let Some(directory) = discovery_tool.and_then(Path::parent) {
            directories.push(directory.to_path_buf());
        }
        if let Ok(executable) = std::env::current_exe()
            && let Some(directory) = executable.parent()
        {
            directories.push(directory.to_path_buf());
            directories.push(directory.join("../Resources"));
        }
        for directory in conventional_library_directories() {
            directories.push(PathBuf::from(directory));
        }
        directories.into_iter().find_map(Self::in_directory)
    }

    fn in_directory(directory: PathBuf) -> Option<Self> {
        let native_plugin = directory.join(native_plugin_library_name());
        let host_plugin = directory.join(host_plugin_library_name());
        if !native_plugin.is_file() || !host_plugin.is_file() {
            return None;
        }
        let resource_dir = carla_prefix(&directory)
            .map(|prefix| prefix.join("share/carla/resources"))
            .filter(|path| path.is_dir())
            .or_else(|| {
                let path = directory.join("resources");
                path.is_dir().then_some(path)
            })
            .unwrap_or_else(|| directory.clone());
        Some(Self {
            native_plugin,
            host_plugin,
            resource_dir,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarlaPluginInfo {
    pub id: String,
    pub name: String,
    pub format: String,
    pub category: String,
    pub path: PathBuf,
    pub label: String,
    pub maker: String,
    pub unique_id: i64,
    pub index: i32,
    pub libraries: CarlaLibraryPaths,
}

impl CarlaPluginInfo {
    pub fn supports_format(format: &str) -> bool {
        carla_plugin_type(format).is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CarlaParameterInfo {
    pub index: usize,
    pub name: String,
    pub unit: String,
    pub minimum: f64,
    pub maximum: f64,
    pub default_value: f64,
    pub current_value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CarlaPluginDescription {
    pub info: CarlaPluginInfo,
    pub parameters: Vec<CarlaParameterInfo>,
}

pub fn inspect_plugin(
    info: &CarlaPluginInfo,
    sample_rate: u32,
    max_block_size: usize,
) -> Result<CarlaPluginDescription, String> {
    let processor = CarlaPluginProcessor::load(info.clone(), sample_rate, max_block_size)?;
    let parameters = (0..processor.parameter_count())
        .filter_map(|index| processor.parameter_info(index).ok())
        .collect();
    Ok(CarlaPluginDescription {
        info: info.clone(),
        parameters,
    })
}

pub struct CarlaPluginProcessor {
    api: CarlaApi,
    descriptor: *const ffi::NativePluginDescriptor,
    native_handle: *mut c_void,
    host_handle: *mut c_void,
    host_state: Box<HostState>,
    _host_descriptor: Box<ffi::NativeHostDescriptor>,
    _ui_name: CString,
    _resource_dir: CString,
    parameter_count: usize,
    input_left: Vec<f32>,
    input_right: Vec<f32>,
    active: bool,
}

// Carla instances are independent, accessed only through `&mut self` while
// processing, and destroyed on the same owning audio worker that uses them.
unsafe impl Send for CarlaPluginProcessor {}

impl CarlaPluginProcessor {
    pub fn load(
        info: CarlaPluginInfo,
        sample_rate: u32,
        max_block_size: usize,
    ) -> Result<Self, String> {
        if sample_rate == 0 || max_block_size == 0 {
            return Err("Carla requires a non-zero sample rate and block size".to_owned());
        }
        let plugin_type = carla_plugin_type(&info.format)
            .ok_or_else(|| format!("Carla does not host {} plugins", info.format))?;
        let api = CarlaApi::load(&info.libraries)?;
        let descriptor = unsafe { (api.get_native_rack_plugin)() };
        if descriptor.is_null() {
            return Err("Carla Native Rack descriptor is unavailable".to_owned());
        }
        let descriptor_ref = unsafe { &*descriptor };
        let instantiate = descriptor_ref
            .instantiate
            .ok_or_else(|| "Carla Native Rack has no instantiate callback".to_owned())?;
        let cleanup = descriptor_ref
            .cleanup
            .ok_or_else(|| "Carla Native Rack has no cleanup callback".to_owned())?;
        if descriptor_ref.activate.is_none()
            || descriptor_ref.deactivate.is_none()
            || descriptor_ref.process.is_none()
        {
            return Err("Carla Native Rack is missing processing callbacks".to_owned());
        }

        let ui_name = c_string(&info.name);
        let resource_dir = c_string(&info.libraries.resource_dir.to_string_lossy());
        let mut host_state = Box::new(HostState::new(sample_rate, max_block_size));
        let host_descriptor = Box::new(ffi::NativeHostDescriptor {
            handle: (&mut *host_state as *mut HostState).cast(),
            resource_dir: resource_dir.as_ptr(),
            ui_name: ui_name.as_ptr(),
            ui_parent_id: 0,
            get_buffer_size: Some(host_get_buffer_size),
            get_sample_rate: Some(host_get_sample_rate),
            is_offline: Some(host_is_offline),
            get_time_info: Some(host_get_time_info),
            write_midi_event: Some(host_write_midi_event),
            ui_parameter_changed: Some(host_ui_parameter_changed),
            ui_midi_program_changed: Some(host_ui_midi_program_changed),
            ui_custom_data_changed: Some(host_ui_custom_data_changed),
            ui_closed: Some(host_ui_closed),
            ui_open_file: Some(host_ui_file),
            ui_save_file: Some(host_ui_file),
            dispatcher: Some(host_dispatcher),
        });
        let native_handle = unsafe { instantiate(&raw const *host_descriptor) };
        if native_handle.is_null() {
            return Err(format!(
                "failed to instantiate Carla Native Rack for {}",
                info.name
            ));
        }
        let host_handle =
            unsafe { (api.create_native_plugin_host_handle)(descriptor, native_handle) };
        if host_handle.is_null() {
            unsafe { cleanup(native_handle) };
            return Err(format!("failed to create a Carla host for {}", info.name));
        }

        let filename = c_string(&info.path.to_string_lossy());
        let plugin_name = c_string(&info.name);
        let label = c_string(&carla_label(&info.format, &info.label));
        let loaded = unsafe {
            (api.add_plugin)(
                host_handle,
                BINARY_NATIVE_64,
                plugin_type,
                nullable_c_string(&filename),
                nullable_c_string(&plugin_name),
                nullable_c_string(&label),
                info.unique_id,
                std::ptr::null(),
                PLUGIN_OPTIONS_NULL,
            )
        };
        if !loaded {
            unsafe {
                (api.host_handle_free)(host_handle);
                cleanup(native_handle);
            }
            return Err(format!(
                "Carla failed to load {} from {}",
                info.name,
                info.path.display()
            ));
        }

        unsafe {
            (api.set_active)(host_handle, 0, true);
            descriptor_ref
                .activate
                .expect("processing callbacks were validated")(native_handle);
        }
        let raw_count = unsafe { (api.get_parameter_count)(host_handle, 0) };
        // The count comes from untrusted plugin code: reject values that
        // would turn the inspect loop below into an allocation bomb.
        const MAX_CARLA_PARAMETERS: usize = 10_000;
        let parameter_count = match usize::try_from(raw_count) {
            Ok(count) if count <= MAX_CARLA_PARAMETERS => count,
            Ok(_) => {
                release_handles(&api, descriptor, native_handle, host_handle);
                return Err(format!(
                    "Carla reported an implausible parameter count: {raw_count}"
                ));
            }
            Err(_) => {
                release_handles(&api, descriptor, native_handle, host_handle);
                return Err(format!(
                    "Carla reported an invalid parameter count: {raw_count}"
                ));
            }
        };
        Ok(Self {
            api,
            descriptor,
            native_handle,
            host_handle,
            host_state,
            _host_descriptor: host_descriptor,
            _ui_name: ui_name,
            _resource_dir: resource_dir,
            parameter_count,
            input_left: vec![0.0; max_block_size],
            input_right: vec![0.0; max_block_size],
            active: true,
        })
    }

    pub fn parameter_count(&self) -> usize {
        self.parameter_count
    }

    pub fn parameter_info(&self, index: usize) -> Result<CarlaParameterInfo, String> {
        let index_u32 = u32::try_from(index)
            .map_err(|_| format!("Carla parameter index is too large: {index}"))?;
        if index >= self.parameter_count {
            return Err(format!("Carla parameter index is out of range: {index}"));
        }
        let info = unsafe { (self.api.get_parameter_info)(self.host_handle, 0, index_u32) };
        let ranges = unsafe { (self.api.get_parameter_ranges)(self.host_handle, 0, index_u32) };
        let name = if info.is_null() {
            index.to_string()
        } else {
            c_text(unsafe { (*info).name }).unwrap_or_else(|| index.to_string())
        };
        let unit = if info.is_null() {
            String::new()
        } else {
            c_text(unsafe { (*info).unit }).unwrap_or_default()
        };
        let (mut minimum, mut maximum, mut default_value) = if ranges.is_null() {
            let default_value = f64::from(unsafe {
                (self.api.get_default_parameter_value)(self.host_handle, 0, index_u32)
            });
            (0.0, 1.0, default_value)
        } else {
            let ranges = unsafe { &*ranges };
            (
                f64::from(ranges.min),
                f64::from(ranges.max),
                f64::from(ranges.default_value),
            )
        };
        if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
            minimum = 0.0;
            maximum = 1.0;
        }
        if !default_value.is_finite() {
            default_value = minimum;
        }
        default_value = default_value.clamp(minimum, maximum);
        let mut current_value = f64::from(unsafe {
            (self.api.get_current_parameter_value)(self.host_handle, 0, index_u32)
        });
        if !current_value.is_finite() {
            current_value = default_value;
        }
        Ok(CarlaParameterInfo {
            index,
            name,
            unit,
            minimum,
            maximum,
            default_value,
            current_value,
        })
    }

    pub fn set_parameter(&mut self, index: usize, value: f64) -> Result<(), String> {
        if !value.is_finite() {
            return Err(format!("Carla parameter {index} is not finite"));
        }
        let info = self.parameter_info(index)?;
        let value = value.clamp(info.minimum, info.maximum) as f32;
        unsafe {
            (self.api.set_parameter_value)(
                self.host_handle,
                0,
                u32::try_from(index).map_err(|_| "Carla parameter index overflow".to_owned())?,
                value,
            );
        }
        Ok(())
    }

    pub fn process(
        &mut self,
        input_left: &[f32],
        input_right: &[f32],
        output_left: &mut [f32],
        output_right: &mut [f32],
        _start_sample: i64,
    ) -> Result<(), String> {
        let frames = input_left.len();
        if input_right.len() != frames
            || output_left.len() != frames
            || output_right.len() != frames
        {
            return Err("Carla stereo buffers have different lengths".to_owned());
        }
        if frames > self.host_state.max_block_size {
            return Err(format!(
                "Carla block has {frames} frames but the configured maximum is {}",
                self.host_state.max_block_size
            ));
        }
        if frames == 0 {
            return Ok(());
        }
        let frame_count = u32::try_from(frames)
            .map_err(|_| format!("Carla block is too large: {frames} frames"))?;
        output_left.fill(0.0);
        output_right.fill(0.0);
        self.input_left[..frames].copy_from_slice(input_left);
        self.input_right[..frames].copy_from_slice(input_right);
        let mut inputs = [self.input_left.as_mut_ptr(), self.input_right.as_mut_ptr()];
        let mut outputs = [output_left.as_mut_ptr(), output_right.as_mut_ptr()];
        unsafe {
            self.descriptor
                .as_ref()
                .and_then(|descriptor| descriptor.process)
                .ok_or_else(|| "Carla Native Rack process callback disappeared".to_owned())?(
                self.native_handle,
                inputs.as_mut_ptr(),
                outputs.as_mut_ptr(),
                frame_count,
                std::ptr::null(),
                0,
            );
        }
        Ok(())
    }
}

/// Releases a half-constructed processor the same way `Drop` does.
/// Used by `load` failure paths that run before `Self` exists.
fn release_handles(
    api: &CarlaApi,
    descriptor: *const ffi::NativePluginDescriptor,
    native_handle: *mut c_void,
    host_handle: *mut c_void,
) {
    unsafe {
        if let Some(deactivate) = (*descriptor).deactivate {
            deactivate(native_handle);
        }
        if !host_handle.is_null() {
            (api.host_handle_free)(host_handle);
        }
        if let Some(cleanup) = (*descriptor).cleanup {
            cleanup(native_handle);
        }
    }
}

impl Drop for CarlaPluginProcessor {
    fn drop(&mut self) {
        if self.native_handle.is_null() {
            return;
        }
        unsafe {
            if self.active
                && let Some(deactivate) = (*self.descriptor).deactivate
            {
                deactivate(self.native_handle);
            }
            if !self.host_handle.is_null() {
                (self.api.host_handle_free)(self.host_handle);
                self.host_handle = std::ptr::null_mut();
            }
            if let Some(cleanup) = (*self.descriptor).cleanup {
                cleanup(self.native_handle);
            }
        }
        self.native_handle = std::ptr::null_mut();
        self.active = false;
    }
}

struct HostState {
    sample_rate: u32,
    max_block_size: usize,
    time_info: ffi::NativeTimeInfo,
}

impl HostState {
    fn new(sample_rate: u32, max_block_size: usize) -> Self {
        Self {
            sample_rate,
            max_block_size,
            time_info: ffi::NativeTimeInfo::default(),
        }
    }
}

struct CarlaApi {
    _native_library: Library,
    _host_library: Library,
    get_native_rack_plugin: unsafe extern "C" fn() -> *const ffi::NativePluginDescriptor,
    create_native_plugin_host_handle:
        unsafe extern "C" fn(*const ffi::NativePluginDescriptor, *mut c_void) -> *mut c_void,
    host_handle_free: unsafe extern "C" fn(*mut c_void),
    add_plugin: unsafe extern "C" fn(
        *mut c_void,
        c_int,
        c_int,
        *const c_char,
        *const c_char,
        *const c_char,
        i64,
        *const c_void,
        u32,
    ) -> bool,
    get_parameter_count: unsafe extern "C" fn(*mut c_void, u32) -> u32,
    get_parameter_info:
        unsafe extern "C" fn(*mut c_void, u32, u32) -> *const ffi::CarlaParameterInfo,
    get_parameter_ranges:
        unsafe extern "C" fn(*mut c_void, u32, u32) -> *const ffi::ParameterRanges,
    get_default_parameter_value: unsafe extern "C" fn(*mut c_void, u32, u32) -> f32,
    get_current_parameter_value: unsafe extern "C" fn(*mut c_void, u32, u32) -> f32,
    set_active: unsafe extern "C" fn(*mut c_void, u32, bool),
    set_parameter_value: unsafe extern "C" fn(*mut c_void, u32, u32, f32),
}

impl CarlaApi {
    fn load(paths: &CarlaLibraryPaths) -> Result<Self, String> {
        let native_library = open_library(&paths.native_plugin)
            .map_err(|error| format!("{}: {error}", paths.native_plugin.display()))?;
        let host_library = open_library(&paths.host_plugin)
            .map_err(|error| format!("{}: {error}", paths.host_plugin.display()))?;
        unsafe {
            Ok(Self {
                get_native_rack_plugin: symbol(&native_library, b"carla_get_native_rack_plugin\0")?,
                create_native_plugin_host_handle: symbol(
                    &native_library,
                    b"carla_create_native_plugin_host_handle\0",
                )?,
                host_handle_free: symbol(&native_library, b"carla_host_handle_free\0")?,
                add_plugin: symbol(&host_library, b"carla_add_plugin\0")?,
                get_parameter_count: symbol(&host_library, b"carla_get_parameter_count\0")?,
                get_parameter_info: symbol(&host_library, b"carla_get_parameter_info\0")?,
                get_parameter_ranges: symbol(&host_library, b"carla_get_parameter_ranges\0")?,
                get_default_parameter_value: symbol(
                    &host_library,
                    b"carla_get_default_parameter_value\0",
                )?,
                get_current_parameter_value: symbol(
                    &host_library,
                    b"carla_get_current_parameter_value\0",
                )?,
                set_active: symbol(&host_library, b"carla_set_active\0")?,
                set_parameter_value: symbol(&host_library, b"carla_set_parameter_value\0")?,
                _native_library: native_library,
                _host_library: host_library,
            })
        }
    }
}

#[cfg(unix)]
fn open_library(path: &Path) -> Result<Library, libloading::Error> {
    use libloading::os::unix::{Library as UnixLibrary, RTLD_GLOBAL, RTLD_NOW};

    unsafe { UnixLibrary::open(Some(path), RTLD_NOW | RTLD_GLOBAL) }.map(Into::into)
}

#[cfg(not(unix))]
fn open_library(path: &Path) -> Result<Library, libloading::Error> {
    unsafe { Library::new(path) }
}

unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    unsafe { library.get::<T>(name) }
        .map(|symbol| *symbol)
        .map_err(|error| {
            format!(
                "Carla symbol {}: {error}",
                String::from_utf8_lossy(name).trim_end_matches('\0')
            )
        })
}

fn carla_plugin_type(format: &str) -> Option<c_int> {
    match format.trim().to_ascii_uppercase().as_str() {
        "LADSPA" => Some(PLUGIN_LADSPA),
        "DSSI" => Some(PLUGIN_DSSI),
        "LV2" => Some(PLUGIN_LV2),
        "VST2" => Some(PLUGIN_VST2),
        "VST3" => Some(PLUGIN_VST3),
        _ => None,
    }
}

fn carla_label(format: &str, label: &str) -> String {
    if format.eq_ignore_ascii_case("LV2")
        && let Some(index) = label.find(".lv2/")
    {
        return label[index + 5..].to_owned();
    }
    label.to_owned()
}

fn carla_prefix(directory: &Path) -> Option<&Path> {
    let parent = directory.parent()?;
    (parent.file_name().and_then(|name| name.to_str()) == Some("lib"))
        .then(|| parent.parent())
        .flatten()
}

fn c_string(value: &str) -> CString {
    CString::new(value.replace('\0', "")).expect("interior nul bytes were removed")
}

fn nullable_c_string(value: &CString) -> *const c_char {
    if value.as_bytes().is_empty() {
        std::ptr::null()
    } else {
        value.as_ptr()
    }
}

fn c_text(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(target_os = "macos")]
fn native_plugin_library_name() -> &'static str {
    "libcarla_native-plugin.dylib"
}

#[cfg(all(unix, not(target_os = "macos")))]
fn native_plugin_library_name() -> &'static str {
    "libcarla_native-plugin.so"
}

#[cfg(target_os = "windows")]
fn native_plugin_library_name() -> &'static str {
    "libcarla_native-plugin.dll"
}

#[cfg(target_os = "macos")]
fn host_plugin_library_name() -> &'static str {
    "libcarla_host-plugin.dylib"
}

#[cfg(all(unix, not(target_os = "macos")))]
fn host_plugin_library_name() -> &'static str {
    "libcarla_host-plugin.so"
}

#[cfg(target_os = "windows")]
fn host_plugin_library_name() -> &'static str {
    "libcarla_host-plugin.dll"
}

#[cfg(target_os = "macos")]
fn conventional_library_directories() -> &'static [&'static str] {
    &[
        "/opt/homebrew/opt/carla/lib/carla",
        "/usr/local/opt/carla/lib/carla",
        "/Applications/Carla.app/Contents/MacOS",
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn conventional_library_directories() -> &'static [&'static str] {
    &["/usr/lib/carla", "/usr/local/lib/carla", "/usr/lib64/carla"]
}

#[cfg(target_os = "windows")]
fn conventional_library_directories() -> &'static [&'static str] {
    &[]
}

unsafe extern "C" fn host_get_buffer_size(handle: *mut c_void) -> u32 {
    let state = unsafe { &*(handle.cast::<HostState>()) };
    state.max_block_size.min(u32::MAX as usize) as u32
}

unsafe extern "C" fn host_get_sample_rate(handle: *mut c_void) -> f64 {
    f64::from(unsafe { &*(handle.cast::<HostState>()) }.sample_rate)
}

unsafe extern "C" fn host_is_offline(_handle: *mut c_void) -> bool {
    false
}

unsafe extern "C" fn host_get_time_info(handle: *mut c_void) -> *const ffi::NativeTimeInfo {
    &raw const unsafe { &*(handle.cast::<HostState>()) }.time_info
}

unsafe extern "C" fn host_write_midi_event(
    _handle: *mut c_void,
    _event: *const ffi::NativeMidiEvent,
) -> bool {
    false
}

unsafe extern "C" fn host_ui_parameter_changed(_handle: *mut c_void, _index: u32, _value: f32) {}

unsafe extern "C" fn host_ui_midi_program_changed(
    _handle: *mut c_void,
    _channel: u8,
    _bank: u32,
    _program: u32,
) {
}

unsafe extern "C" fn host_ui_custom_data_changed(
    _handle: *mut c_void,
    _key: *const c_char,
    _value: *const c_char,
) {
}

unsafe extern "C" fn host_ui_closed(_handle: *mut c_void) {}

unsafe extern "C" fn host_ui_file(
    _handle: *mut c_void,
    _is_dir: bool,
    _title: *const c_char,
    _filter: *const c_char,
) -> *const c_char {
    std::ptr::null()
}

unsafe extern "C" fn host_dispatcher(
    _handle: *mut c_void,
    _opcode: c_int,
    _index: i32,
    _value: isize,
    _pointer: *mut c_void,
    _option: f32,
) -> isize {
    0
}

mod ffi {
    #![allow(dead_code)]

    use super::{c_char, c_int, c_void};

    #[repr(C)]
    #[derive(Default)]
    pub struct NativeTimeInfoBbt {
        pub valid: bool,
        pub bar: i32,
        pub beat: i32,
        pub tick: f64,
        pub bar_start_tick: f64,
        pub beats_per_bar: f32,
        pub beat_type: f32,
        pub ticks_per_beat: f64,
        pub beats_per_minute: f64,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct NativeTimeInfo {
        pub playing: bool,
        pub frame: u64,
        pub usecs: u64,
        pub bbt: NativeTimeInfoBbt,
    }

    #[repr(C)]
    pub struct NativeMidiEvent {
        pub time: u32,
        pub port: u8,
        pub size: u8,
        pub data: [u8; 4],
    }

    #[repr(C)]
    pub struct NativeHostDescriptor {
        pub handle: *mut c_void,
        pub resource_dir: *const c_char,
        pub ui_name: *const c_char,
        pub ui_parent_id: usize,
        pub get_buffer_size: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
        pub get_sample_rate: Option<unsafe extern "C" fn(*mut c_void) -> f64>,
        pub is_offline: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
        pub get_time_info: Option<unsafe extern "C" fn(*mut c_void) -> *const NativeTimeInfo>,
        pub write_midi_event:
            Option<unsafe extern "C" fn(*mut c_void, *const NativeMidiEvent) -> bool>,
        pub ui_parameter_changed: Option<unsafe extern "C" fn(*mut c_void, u32, f32)>,
        pub ui_midi_program_changed: Option<unsafe extern "C" fn(*mut c_void, u8, u32, u32)>,
        pub ui_custom_data_changed:
            Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
        pub ui_closed: Option<unsafe extern "C" fn(*mut c_void)>,
        pub ui_open_file: Option<
            unsafe extern "C" fn(*mut c_void, bool, *const c_char, *const c_char) -> *const c_char,
        >,
        pub ui_save_file: Option<
            unsafe extern "C" fn(*mut c_void, bool, *const c_char, *const c_char) -> *const c_char,
        >,
        pub dispatcher:
            Option<unsafe extern "C" fn(*mut c_void, c_int, i32, isize, *mut c_void, f32) -> isize>,
    }

    #[repr(C)]
    pub struct NativePluginDescriptor {
        pub category: c_int,
        pub hints: c_int,
        pub supports: c_int,
        pub audio_ins: u32,
        pub audio_outs: u32,
        pub midi_ins: u32,
        pub midi_outs: u32,
        pub param_ins: u32,
        pub param_outs: u32,
        pub name: *const c_char,
        pub label: *const c_char,
        pub maker: *const c_char,
        pub copyright: *const c_char,
        pub instantiate: Option<unsafe extern "C" fn(*const NativeHostDescriptor) -> *mut c_void>,
        pub cleanup: Option<unsafe extern "C" fn(*mut c_void)>,
        pub get_parameter_count: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
        pub get_parameter_info:
            Option<unsafe extern "C" fn(*mut c_void, u32) -> *const NativeParameter>,
        pub get_parameter_value: Option<unsafe extern "C" fn(*mut c_void, u32) -> f32>,
        pub get_midi_program_count: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
        pub get_midi_program_info:
            Option<unsafe extern "C" fn(*mut c_void, u32) -> *const NativeMidiProgram>,
        pub set_parameter_value: Option<unsafe extern "C" fn(*mut c_void, u32, f32)>,
        pub set_midi_program: Option<unsafe extern "C" fn(*mut c_void, u8, u32, u32)>,
        pub set_custom_data:
            Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
        pub ui_show: Option<unsafe extern "C" fn(*mut c_void, bool)>,
        pub ui_idle: Option<unsafe extern "C" fn(*mut c_void)>,
        pub ui_set_parameter_value: Option<unsafe extern "C" fn(*mut c_void, u32, f32)>,
        pub ui_set_midi_program: Option<unsafe extern "C" fn(*mut c_void, u8, u32, u32)>,
        pub ui_set_custom_data:
            Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
        pub activate: Option<unsafe extern "C" fn(*mut c_void)>,
        pub deactivate: Option<unsafe extern "C" fn(*mut c_void)>,
        pub process: Option<
            unsafe extern "C" fn(
                *mut c_void,
                *mut *mut f32,
                *mut *mut f32,
                u32,
                *const NativeMidiEvent,
                u32,
            ),
        >,
        pub get_state: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_char>,
        pub set_state: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
        pub dispatcher:
            Option<unsafe extern "C" fn(*mut c_void, c_int, i32, isize, *mut c_void, f32) -> isize>,
        pub render_inline_display:
            Option<unsafe extern "C" fn(*mut c_void, u32, u32) -> *const c_void>,
        pub cv_ins: u32,
        pub cv_outs: u32,
        pub get_buffer_port_name:
            Option<unsafe extern "C" fn(*mut c_void, u32, bool) -> *const c_char>,
        pub get_buffer_port_range:
            Option<unsafe extern "C" fn(*mut c_void, u32, bool) -> *const NativePortRange>,
        pub ui_width: u16,
        pub ui_height: u16,
    }

    #[repr(C)]
    pub struct NativeParameter {
        pub hints: c_int,
        pub name: *const c_char,
        pub unit: *const c_char,
        pub ranges: NativeParameterRanges,
        pub scale_point_count: u32,
        pub scale_points: *const c_void,
        pub comment: *const c_char,
        pub group_name: *const c_char,
        pub designation: u32,
    }

    #[repr(C)]
    pub struct NativeParameterRanges {
        pub default_value: f32,
        pub min: f32,
        pub max: f32,
        pub step: f32,
        pub step_small: f32,
        pub step_large: f32,
    }

    #[repr(C)]
    pub struct NativeMidiProgram {
        pub bank: u32,
        pub program: u32,
        pub name: *const c_char,
    }

    #[repr(C)]
    pub struct NativePortRange {
        pub minimum: f32,
        pub maximum: f32,
    }

    #[repr(C)]
    pub struct CarlaParameterInfo {
        pub name: *const c_char,
        pub symbol: *const c_char,
        pub unit: *const c_char,
        pub comment: *const c_char,
        pub group_name: *const c_char,
        pub scale_point_count: u32,
    }

    #[repr(C)]
    pub struct ParameterRanges {
        pub default_value: f32,
        pub min: f32,
        pub max: f32,
        pub step: f32,
        pub step_small: f32,
        pub step_large: f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_lv2_label_rules_match_the_qt_carla_adapter() {
        for format in ["LADSPA", "DSSI", "LV2", "VST2", "VST3"] {
            assert!(CarlaPluginInfo::supports_format(format));
        }
        for format in ["CLAP", "SF2", "SFZ", "AU"] {
            assert!(!CarlaPluginInfo::supports_format(format));
        }
        assert_eq!(
            carla_label("LV2", "/plugins/gain.lv2/http://example.org/gain"),
            "http://example.org/gain"
        );
        assert_eq!(carla_label("VST2", "gain"), "gain");
    }

    #[test]
    #[ignore = "requires installed Carla libraries"]
    fn installed_carla_pair_exposes_the_native_rack_descriptor() {
        let Some(paths) = CarlaLibraryPaths::discover(None) else {
            panic!("installed Carla libraries are required; run with -- --ignored where available");
        };
        let api = CarlaApi::load(&paths).expect("installed Carla libraries load");
        assert!(!unsafe { (api.get_native_rack_plugin)() }.is_null());
    }

    #[test]
    #[ignore = "requires installed Carla libraries and AVIQTL_CARLA_TEST_LADSPA"]
    fn external_ladspa_fixture_processes_through_carla() {
        let Some(path) = std::env::var_os("AVIQTL_CARLA_TEST_LADSPA").map(PathBuf::from) else {
            panic!(
                "AVIQTL_CARLA_TEST_LADSPA must point at a LADSPA fixture; run with -- --ignored where available"
            );
        };
        let paths = CarlaLibraryPaths::discover(None).expect("Carla libraries are installed");
        let info = CarlaPluginInfo {
            id: "LADSPA:aviqtl_gain:41001".to_owned(),
            name: "AviQtl Test Gain".to_owned(),
            format: "LADSPA".to_owned(),
            category: "Utility".to_owned(),
            path,
            label: "aviqtl_gain".to_owned(),
            maker: "AviQtl".to_owned(),
            unique_id: 41_001,
            index: 0,
            libraries: paths,
        };
        let mut processor =
            CarlaPluginProcessor::load(info, 48_000, 64).expect("LADSPA fixture loads");
        assert_eq!(processor.parameter_count(), 1);
        processor.set_parameter(0, 2.0).expect("gain changes");
        let left = [0.25_f32; 32];
        let right = [-0.5_f32; 32];
        let mut output_left = [0.0_f32; 32];
        let mut output_right = [0.0_f32; 32];
        processor
            .process(&left, &right, &mut output_left, &mut output_right, 256)
            .expect("LADSPA fixture processes");
        assert!(
            output_left
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-6)
        );
        assert!(
            output_right
                .iter()
                .all(|sample| (*sample + 1.0).abs() < 1e-6)
        );
    }
}
