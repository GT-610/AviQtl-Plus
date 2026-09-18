//! Shared GPU initialization and opt-in rendering validation.

use slint::ComponentHandle;
use slint::wgpu_29::wgpu;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub(super) const VALIDATION_PROJECT: &[u8] = br#"{
    "version": 3,
    "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
    "scenes": [
        {"id": 1, "name": "Root", "duration": 1200, "gridMode": "Frame", "gridInterval": 10},
        {"id": 2, "name": "Scene 2", "duration": 600}
    ],
    "clips": [
        {"id": 1, "sceneId": 1, "type": "video", "start": 30, "duration": 180, "layer": 0},
        {"id": 2, "sceneId": 1, "type": "text", "start": 120, "duration": 100, "layer": 1},
        {"id": 3, "sceneId": 1, "type": "audio", "start": 60, "duration": 240, "layer": 3}
    ]
}"#;

pub(super) fn parse_validation_frames() -> Result<Option<u64>, String> {
    let mut args = std::env::args().skip(1);
    let mut frames = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--validate-gpu" => frames = Some(120),
            "--frames" => {
                let value = args.next().ok_or("--frames requires a value")?;
                frames = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| "invalid --frames value")?
                        .max(1),
                );
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(frames)
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
