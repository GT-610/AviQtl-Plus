//! Shared GPU initialization and opt-in rendering validation.

use slint::ComponentHandle;
use slint::wgpu_29::wgpu;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// A project with one video, one text, and one audio clip, used by
/// `--validate-gpu` / `--frames`.
///
/// Each clip carries the effects a saved project has: `transform` plus the
/// object effect itself. Clips without effects load fine but project an
/// empty object-settings panel, which looks like a UI bug rather than a
/// fixture problem.
pub(super) const VALIDATION_PROJECT: &str = r##"{
    "version": 3,
    "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
    "scenes": [
        {"id": 1, "name": "Root", "duration": 1200, "gridMode": "Frame", "gridInterval": 10},
        {"id": 2, "name": "Scene 2", "duration": 600}
    ],
    "clips": [
        {"id": 1, "sceneId": 1, "type": "video", "start": 30, "duration": 180, "layer": 0,
         "effects": [
                {"enabled": true, "id": "transform", "params": {"backfaceVisible": true, "blendMode": "通常", "cx": 0, "cy": 0, "cz": 0, "opacity": 1.0, "rotationX": 0, "rotationY": 0, "rotationZ": 0, "scale": 100, "x": 0, "y": 0, "z": 0}},
                {"enabled": true, "id": "video", "params": {"directFrame": 0, "opacity": 1.0, "path": "", "playMode": "normal", "speed": 100.0, "startFrame": 0}}
         ]},
        {"id": 2, "sceneId": 1, "type": "text", "start": 120, "duration": 100, "layer": 1,
         "effects": [
                {"enabled": true, "id": "transform", "params": {"backfaceVisible": true, "blendMode": "通常", "cx": 0, "cy": 0, "cz": 0, "opacity": 1.0, "rotationX": 0, "rotationY": 0, "rotationZ": 0, "scale": 100, "x": 0, "y": 0, "z": 0}},
                {"enabled": true, "id": "text", "params": {"alignment": 4, "backgroundColor": "#80000000", "backgroundPaddingX": 20, "backgroundPaddingY": 10, "backgroundRadius": 10, "bgEnabled": false, "color": "#ffffff", "fontBold": false, "fontFamily": "sans-serif", "fontItalic": false, "fontSize": 48, "letterSpacing": 0, "lineSpacing": 0, "outlineColor": "#000000", "outlineEnabled": false, "outlineWidth": 2, "shadowColor": "#80000000", "shadowEnabled": false, "shadowOffsetX": 5, "shadowOffsetY": 5, "text": "テキスト"}}
         ]},
        {"id": 3, "sceneId": 1, "type": "audio", "start": 60, "duration": 240, "layer": 3,
         "effects": [
                {"enabled": true, "id": "audio", "params": {"directTime": 0, "fadeIn": 0.0, "fadeOut": 0.0, "limiter": true, "linkedVideo": false, "masterVolume": 1.0, "mute": false, "pan": 0.0, "playMode": "normal", "solo": false, "source": "", "speed": 100.0, "startTime": 0, "volume": 1.0}}
         ]}
    ]
}"##;

/// Reports whether the host exposes a wgpu adapter backed by real GPU hardware.
///
/// A CPU adapter is not merely slow: the compositor's 41-case effect shader
/// cannot be compiled by the FXC path WARP uses, so the pipeline creation
/// crashes with an access violation instead of returning an error. A host that
/// only offers `Microsoft Basic Render Driver` therefore cannot run the
/// production preview at all, and must be told so instead of crashing.
pub(super) fn has_gpu_backed_adapter() -> bool {
    let instance = wgpu::Instance::default();
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .any(|adapter| adapter.get_info().device_type != wgpu::DeviceType::Cpu)
}

/// Lists every wgpu adapter the host exposes, so a GPU-less launch can say what
/// it actually found instead of only what it needs.
pub(super) fn describe_adapters() -> Vec<String> {
    let instance = wgpu::Instance::default();
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .map(|adapter| {
            let info = adapter.get_info();
            format!(
                "- {} (backend {:?}, device type {:?})",
                info.name, info.backend, info.device_type
            )
        })
        .collect()
}

/// The human-readable launch failure used when no GPU-backed wgpu adapter exists.
///
/// Adobe documents GPU memory requirements for Premiere rather than shipping a
/// software renderer, and removed the software-rendering option entirely
/// because it degraded image quality. AviQtl takes the same stance: the preview
/// compositor is GPU-only, and a GPU-less host gets an actionable message
/// instead of an access violation.
pub(super) fn missing_gpu_message(adapters: &[String]) -> String {
    let detected = if adapters.is_empty() {
        "  (none)".to_owned()
    } else {
        adapters.join("\n")
    };
    format!(
        "AviQtl requires a GPU-backed wgpu adapter and could not find one.\n\
         \n\
         Adapters detected on this host:\n\
         {detected}\n\
         \n\
         Why the detected adapters cannot be used:\n\
         - No adapter at all: install a graphics driver, then try again.\n\
         - \"Microsoft Basic Render Driver\" (WARP) or another CPU adapter: it exposes a\n\
           `DeviceType::Cpu` adapter that appears usable, but its shader compiler\n\
           cannot build the compositor's effect pipelines. The application would otherwise\n\
           die with an access violation (0xC0000005) instead of reporting an error.\n\
         \n\
         A discrete or integrated GPU from the last decade with current drivers is\n\
         required. Hardware encoding and decoding also depend on the same GPU."
    )
}

/// Launch options parsed from the command line.
pub(super) struct LaunchOptions {
    /// Frame budget for the opt-in `--validate-gpu` / `--frames` gate.
    pub(super) validation_frames: Option<u64>,
}

pub(super) fn parse_launch_options() -> Result<LaunchOptions, String> {
    let mut args = std::env::args().skip(1);
    let mut options = LaunchOptions {
        validation_frames: None,
    };
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--validate-gpu" => options.validation_frames = Some(120),
            "--frames" => {
                let value = args.next().ok_or("--frames requires a value")?;
                options.validation_frames = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| "invalid --frames value")?
                        .max(1),
                );
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(options)
}

pub(super) struct AppGpu {
    pub(super) instance: wgpu::Instance,
    pub(super) adapter: wgpu::Adapter,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) errors: Arc<Mutex<Vec<String>>>,
}

impl AppGpu {
    pub(super) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("aviqtl-slint-device"),
                ..Default::default()
            }))?;
        let errors = Arc::new(Mutex::new(Vec::new()));
        let error_log = errors.clone();
        device.on_uncaptured_error(Arc::new(move |error| {
            if let Ok(mut errors) = error_log.lock() {
                errors.push(error.to_string());
            }
        }));
        let info = adapter.get_info();
        eprintln!("AviQtl Slint GPU: {} ({:?})", info.name, info.backend);
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            errors,
        })
    }
}
pub(super) enum WindowKind {
    Main,
    Timeline,
}

pub(super) struct GpuValidation {
    pub(super) expected_device: wgpu::Device,
    pub(super) main_same_device: Cell<bool>,
    pub(super) timeline_same_device: Cell<bool>,
    pub(super) main_frames: Cell<u64>,
    pub(super) timeline_frames: Cell<u64>,
    pub(super) preview_updates: Cell<u64>,
    pub(super) texture_imported: bool,
    pub(super) errors: Arc<Mutex<Vec<String>>>,
}

impl GpuValidation {
    pub(super) fn new(
        expected_device: wgpu::Device,
        errors: Arc<Mutex<Vec<String>>>,
        texture_imported: bool,
    ) -> Self {
        Self {
            expected_device,
            main_same_device: Cell::new(false),
            timeline_same_device: Cell::new(false),
            main_frames: Cell::new(0),
            timeline_frames: Cell::new(0),
            preview_updates: Cell::new(0),
            texture_imported,
            errors,
        }
    }

    pub(super) fn print_and_validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        let errors = self
            .errors
            .lock()
            .map_err(|_| "wgpu error log was poisoned")?
            .clone();
        let summary = serde_json::json!({
            "main_same_device": self.main_same_device.get(),
            "timeline_same_device": self.timeline_same_device.get(),
            "texture_imported": self.texture_imported,
            "main_frames": self.main_frames.get(),
            "timeline_frames": self.timeline_frames.get(),
            "preview_updates": self.preview_updates.get(),
            "wgpu_errors": errors,
        });
        println!(
            "SLINT_WGPU_COMPATIBILITY {}",
            serde_json::to_string_pretty(&summary)?
        );
        if !self.main_same_device.get()
            || !self.timeline_same_device.get()
            || !self.texture_imported
            || self.main_frames.get() == 0
            || self.timeline_frames.get() == 0
            || self.preview_updates.get() == 0
            || !summary["wgpu_errors"].as_array().is_some_and(Vec::is_empty)
        {
            return Err("Slint/wgpu compatibility validation failed".into());
        }
        Ok(())
    }
}

pub(super) fn install_render_probe<T: ComponentHandle + 'static>(
    component: &T,
    stats: Rc<GpuValidation>,
    kind: WindowKind,
) -> Result<(), slint::SetRenderingNotifierError> {
    component.window().set_rendering_notifier(move |phase, api| match phase {
        slint::RenderingState::RenderingSetup => {
            let same = matches!(api, slint::GraphicsAPI::WGPU29 { device, .. } if device == &stats.expected_device);
            match kind {
                WindowKind::Main => stats.main_same_device.set(same),
                WindowKind::Timeline => stats.timeline_same_device.set(same),
            }
        }
        slint::RenderingState::AfterRendering => match kind {
            WindowKind::Main => stats.main_frames.set(stats.main_frames.get() + 1),
            WindowKind::Timeline => stats.timeline_frames.set(stats.timeline_frames.get() + 1),
        },
        _ => {}
    })
}
