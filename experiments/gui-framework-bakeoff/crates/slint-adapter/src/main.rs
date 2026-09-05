#![deny(unsafe_code)]

use std::{
    cell::RefCell,
    env, process,
    rc::Rc,
    time::{Duration, Instant},
};

use aviqtl_gui_lab_core::{
    BenchmarkConfig, FrameMetrics, MetricsCollector, RunMetadata, ScriptedWorkload,
    TimelineDataset, VisibleClip, rustc_version, write_report,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

const PREVIEW_WIDTH: u32 = 640;
const PREVIEW_HEIGHT: u32 = 360;

slint::slint! {
    import { Button, LineEdit, Slider } from "std-widgets.slint";

    export struct ClipView {
        id: int,
        x: float,
        y: float,
        width: float,
        height: float,
        color: color,
    }

    export component AuxiliaryWindow inherits Window {
        title: "AviQtl auxiliary preview";
        preferred-width: 720px;
        preferred-height: 450px;
        in property <image> preview-image;

        Rectangle {
            background: #181b22;
            VerticalLayout {
                padding: 12px;
                spacing: 8px;
                Text { text: "Independent preview window"; color: #e8eaf0; font-size: 18px; }
                Image {
                    source: root.preview-image;
                    image-fit: contain;
                    vertical-stretch: 1;
                }
            }
        }
    }

    export component MainWindow inherits Window {
        title: "AviQtl GUI bake-off — Slint";
        preferred-width: 1440px;
        preferred-height: 900px;
        min-width: 960px;
        min-height: 640px;
        background: #181b22;

        in property <[ClipView]> visible-clips;
        in property <image> preview-image;
        in property <float> playhead-x;
        in property <string> frame-label: "Frame 0";
        in property <string> visible-label: "visible 0";
        in property <string> zoom-label: "zoom 1.00px/f";
        in property <string> effect-label: "Exposure 0.00";
        in-out property <string> selected-label: "Selected clip: none";
        in property <bool> interactive;
        out property <float> timeline-width: timeline.width / 1px;
        out property <float> timeline-height: timeline.height / 1px;
        callback toggle-auxiliary();
        callback clip-selected(int);

        VerticalLayout {
            spacing: 0px;

            Rectangle {
                height: 52px;
                background: #20242d;
                HorizontalLayout {
                    padding-left: 12px;
                    padding-right: 12px;
                    spacing: 8px;
                    Text {
                        text: "AviQtl 2 UI laboratory";
                        color: #f2f3f7;
                        font-size: 19px;
                        vertical-alignment: center;
                    }
                    Button { text: "＋ メディア追加"; }
                    Button { text: "分割"; }
                    Button { text: "元に戻す"; }
                    Rectangle { horizontal-stretch: 1; }
                    Text { text: root.frame-label; color: #c7cad3; vertical-alignment: center; }
                    Button { text: "補助プレビュー"; clicked => { root.toggle-auxiliary(); } }
                    Text {
                        text: root.interactive ? "interactive" : "benchmark";
                        color: root.interactive ? #8fd694 : #8dc8ff;
                        vertical-alignment: center;
                    }
                }
            }

            HorizontalLayout {
                spacing: 0px;
                vertical-stretch: 1;

                Rectangle {
                    width: 220px;
                    background: #20242d;
                    VerticalLayout {
                        padding: 12px;
                        spacing: 8px;
                        Text { text: "プロジェクト / 项目"; color: #f2f3f7; font-size: 18px; }
                        LineEdit { placeholder-text: "素材を検索 / 搜索素材"; }
                        Text { text: "▣  scene_001.mp4"; color: #e0e2e8; }
                        Text { text: "     3840×2160 · 60fps"; color: #9298a7; font-size: 11px; }
                        Text { text: "♫  voice_main.wav"; color: #e0e2e8; }
                        Text { text: "     48kHz · stereo"; color: #9298a7; font-size: 11px; }
                        Text { text: "◆  タイトル"; color: #e0e2e8; }
                        Text { text: "▧  背景.png"; color: #e0e2e8; }
                        Rectangle { vertical-stretch: 1; }
                        Text { text: "10,000 clips · 300 layers"; color: #9298a7; font-size: 11px; }
                    }
                }

                Rectangle {
                    horizontal-stretch: 1;
                    background: #111319;
                    VerticalLayout {
                        padding: 12px;
                        spacing: 8px;
                        Text {
                            text: "Preview — caller-owned wgpu texture";
                            color: #e8eaf0;
                            horizontal-alignment: center;
                            font-size: 17px;
                        }
                        Image {
                            source: root.preview-image;
                            image-fit: contain;
                            vertical-stretch: 1;
                        }
                    }
                }

                Rectangle {
                    width: 280px;
                    background: #20242d;
                    VerticalLayout {
                        padding: 12px;
                        spacing: 8px;
                        Text { text: "エフェクト / 效果"; color: #f2f3f7; font-size: 18px; }
                        Text { text: root.selected-label; color: #8dc8ff; }
                        Text { text: "Effect name"; color: #aeb3c0; }
                        LineEdit { text: "色調補正 / Color correction"; }
                        Text { text: "Opacity"; color: #aeb3c0; }
                        Slider { value: 82; minimum: 0; maximum: 100; }
                        Text { text: "Position X"; color: #aeb3c0; }
                        Slider { value: 50; minimum: 0; maximum: 100; }
                        Text { text: "Position Y"; color: #aeb3c0; }
                        Slider { value: 50; minimum: 0; maximum: 100; }
                        Text { text: root.effect-label; color: #8dc8ff; }
                        Text { text: "Transform · Scale 100% · Rotation 0°"; color: #c7cad3; wrap: word-wrap; }
                        Rectangle { vertical-stretch: 1; }
                        Text { text: "IME probe"; color: #aeb3c0; }
                        LineEdit { placeholder-text: "日本語 / 中文输入"; }
                    }
                }
            }

            timeline := Rectangle {
                preferred-height: 390px;
                min-height: 220px;
                background: #191c23;

                Rectangle {
                    height: 28px;
                    background: #242934;
                    HorizontalLayout {
                        padding-left: 10px;
                        spacing: 14px;
                        Text { text: "Timeline"; color: #f2f3f7; font-weight: 700; vertical-alignment: center; }
                        Text { text: root.visible-label; color: #aeb3c0; vertical-alignment: center; }
                        Text { text: root.zoom-label; color: #aeb3c0; vertical-alignment: center; }
                    }
                }

                for clip in root.visible-clips: Rectangle {
                    x: clip.x * 1px;
                    y: 28px + clip.y * 1px;
                    width: max(1px, clip.width * 1px);
                    height: max(1px, clip.height * 1px);
                    background: clip.color;
                    border-radius: 3px;
                    Text {
                        text: clip.width > 44 ? "C" + clip.id : "";
                        color: #101218;
                        font-size: 10px;
                        x: 5px;
                        vertical-alignment: center;
                    }
                    TouchArea { clicked => { root.clip-selected(clip.id); } }
                }

                Rectangle {
                    x: root.playhead-x * 1px;
                    y: 0px;
                    width: 2px;
                    height: parent.height;
                    background: #ff4c60;
                }
            }
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("failed to run Slint adapter: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (config, interactive) = parse_configuration();
    slint::BackendSelector::new()
        .require_wgpu_29(slint::wgpu_29::WGPUConfiguration::default())
        .select()?;

    let app = MainWindow::new()?;
    app.set_interactive(interactive);
    let clip_model = Rc::new(VecModel::from(Vec::<ClipView>::new()));
    app.set_visible_clips(ModelRc::from(clip_model.clone()));
    install_auxiliary_window_handler(&app);
    let selection_weak = app.as_weak();
    app.on_clip_selected(move |id| {
        if let Some(app) = selection_weak.upgrade() {
            app.set_selected_label(SharedString::from(format!("Selected clip: {id}")));
        }
    });

    let state = Rc::new(RefCell::new(SlintState::new(config, interactive)));
    let frame_state = state.clone();
    let rendering_clip_model = clip_model.clone();
    let app_weak = app.as_weak();
    app.window()
        .set_rendering_notifier(move |rendering_phase, graphics_api| {
            let Some(app) = app_weak.upgrade() else {
                return;
            };
            match rendering_phase {
                slint::RenderingState::RenderingSetup => {
                    let slint::GraphicsAPI::WGPU29 { device, queue, .. } = graphics_api else {
                        eprintln!("Slint did not provide the required WGPU29 renderer");
                        process::exit(1);
                    };
                    app.set_preview_image(create_preview_image(device, queue));
                    app.window().request_redraw();
                }
                slint::RenderingState::AfterRendering => {
                    let complete = frame_state.borrow_mut().presented();
                    if complete && !interactive {
                        let close_weak = app.as_weak();
                        slint::invoke_from_event_loop(move || {
                            if let Some(app) = close_weak.upgrade() {
                                let _ = app.hide();
                            }
                        })
                        .unwrap_or_else(|error| {
                            eprintln!("failed to schedule Slint close: {error}");
                            process::exit(1);
                        });
                    } else {
                        let next_weak = app.as_weak();
                        let next_state = frame_state.clone();
                        let next_clip_model = rendering_clip_model.clone();
                        slint::Timer::single_shot(Duration::ZERO, move || {
                            if let Some(app) = next_weak.upgrade() {
                                next_state.borrow_mut().prepare(&app, &next_clip_model);
                                app.window().request_redraw();
                            }
                        });
                    }
                }
                _ => {}
            }
        })?;

    app.show()?;
    #[cfg(target_os = "macos")]
    slint::private_unstable_api::re_exports::macos_bring_all_windows_to_front();
    let initial_weak = app.as_weak();
    slint::Timer::single_shot(Duration::ZERO, move || {
        if let Some(app) = initial_weak.upgrade() {
            state.borrow_mut().prepare(&app, &clip_model);
            app.window().request_redraw();
        }
    });
    slint::run_event_loop()?;
    Ok(())
}

fn parse_configuration() -> (BenchmarkConfig, bool) {
    let mut interactive = false;
    let arguments = env::args().skip(1).filter(|argument| {
        if argument == "--interactive" {
            interactive = true;
            false
        } else {
            true
        }
    });
    let config = BenchmarkConfig::parse(arguments).unwrap_or_else(|error| {
        eprintln!("{error}\nGUI option: --interactive");
        process::exit(2);
    });
    (config, interactive)
}

struct SlintState {
    config: BenchmarkConfig,
    interactive: bool,
    dataset: TimelineDataset,
    workload: ScriptedWorkload,
    visible: Vec<VisibleClip>,
    metrics: Option<MetricsCollector>,
    previous_frame: Option<Instant>,
    pending_frame: Option<PendingFrame>,
}

struct PendingFrame {
    frame_cpu: Duration,
    query_cpu: Duration,
    visible_clips: usize,
}

impl SlintState {
    fn new(config: BenchmarkConfig, interactive: bool) -> Self {
        let dataset = TimelineDataset::synthetic(config.dataset);
        let workload = ScriptedWorkload::new(dataset.total_frames(), config.dataset.layer_count);
        let metrics = MetricsCollector::new(
            RunMetadata {
                framework: "Slint".to_owned(),
                framework_version: "1.17.1".to_owned(),
                renderer: "FemtoVG / wgpu 29 / imported texture".to_owned(),
                platform: format!("{}-{}", env::consts::OS, env::consts::ARCH),
                rustc: rustc_version(),
                profile: if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
                .to_owned(),
            },
            config.dataset,
            config.frames,
            config.warmup_frames,
        );
        Self {
            config,
            interactive,
            dataset,
            workload,
            visible: Vec::with_capacity(512),
            metrics: Some(metrics),
            previous_frame: None,
            pending_frame: None,
        }
    }

    fn prepare(&mut self, app: &MainWindow, clip_model: &VecModel<ClipView>) {
        if self.pending_frame.is_some() {
            return;
        }
        let frame_started = Instant::now();
        self.workload.advance();
        self.workload.resize(
            app.get_timeline_width().max(1.0),
            (app.get_timeline_height() - 28.0).max(1.0),
        );
        let query_started = Instant::now();
        self.dataset
            .query_visible(self.workload.viewport(), &mut self.visible);
        let query_cpu = query_started.elapsed();

        clip_model.set_vec(
            self.visible
                .iter()
                .map(|clip| ClipView {
                    id: i32::try_from(clip.id).unwrap_or(i32::MAX),
                    x: clip.x_px,
                    y: clip.y_px,
                    width: clip.width_px,
                    height: clip.height_px,
                    color: slint::Color::from_argb_u8(
                        clip.color[3],
                        clip.color[0],
                        clip.color[1],
                        clip.color[2],
                    ),
                })
                .collect::<Vec<_>>(),
        );
        let viewport = self.workload.viewport();
        app.set_playhead_x(
            ((self.workload.playhead_frame() - viewport.first_frame)
                * f64::from(viewport.pixels_per_frame)) as f32,
        );
        app.set_frame_label(SharedString::from(format!(
            "Frame {:.0}",
            self.workload.playhead_frame()
        )));
        app.set_visible_label(SharedString::from(format!(
            "visible {}",
            self.visible.len()
        )));
        app.set_zoom_label(SharedString::from(format!(
            "zoom {:.2}px/f",
            viewport.pixels_per_frame
        )));
        app.set_effect_label(SharedString::from(format!(
            "Exposure {:.2} · Temperature {:.0} K",
            (self.workload.frame_index() as f32 * 0.01).sin(),
            6500.0 + (self.workload.frame_index() as f32).sin() * 400.0
        )));

        self.pending_frame = Some(PendingFrame {
            frame_cpu: frame_started.elapsed(),
            query_cpu,
            visible_clips: self.visible.len(),
        });
    }

    fn presented(&mut self) -> bool {
        let Some(pending) = self.pending_frame.take() else {
            return false;
        };
        let presented = Instant::now();
        let frame_interval = self
            .previous_frame
            .replace(presented)
            .map(|previous| presented.saturating_duration_since(previous));

        let mut complete = false;
        if let Some(metrics) = &mut self.metrics {
            metrics.record(FrameMetrics {
                frame_cpu: pending.frame_cpu,
                query_cpu: pending.query_cpu,
                frame_interval,
                visible_clips: pending.visible_clips,
            });
            complete = metrics.is_complete();
        }
        if complete {
            let report = self.metrics.take().expect("metrics exist").finish();
            if let Some(path) = self.config.report_path.as_deref() {
                write_report(path, &report).unwrap_or_else(|error| {
                    eprintln!("failed to write {}: {error}", path.display());
                    process::exit(1);
                });
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("serialize Slint report")
            );
        }
        complete && !self.interactive
    }
}

fn install_auxiliary_window_handler(app: &MainWindow) {
    let auxiliary = Rc::new(RefCell::new(None::<AuxiliaryWindow>));
    let auxiliary_ref = auxiliary.clone();
    let app_weak = app.as_weak();
    app.on_toggle_auxiliary(move || {
        if let Some(window) = auxiliary_ref.borrow_mut().take() {
            let _ = window.hide();
            return;
        }
        let Ok(window) = AuxiliaryWindow::new() else {
            return;
        };
        if let Some(app) = app_weak.upgrade() {
            window.set_preview_image(app.get_preview_image());
        }
        if window.show().is_ok() {
            *auxiliary_ref.borrow_mut() = Some(window);
        }
    });
}

fn create_preview_image(
    device: &slint::wgpu_29::wgpu::Device,
    queue: &slint::wgpu_29::wgpu::Queue,
) -> slint::Image {
    use slint::wgpu_29::wgpu;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aviqtl-slint-preview-probe"),
        size: wgpu::Extent3d {
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut pixels = vec![0_u8; PREVIEW_WIDTH as usize * PREVIEW_HEIGHT as usize * 4];
    for y in 0..PREVIEW_HEIGHT {
        for x in 0..PREVIEW_WIDTH {
            let offset = ((y * PREVIEW_WIDTH + x) * 4) as usize;
            let grid = u8::from(x % 80 < 2 || y % 80 < 2) * 28;
            pixels[offset] = 18_u8.saturating_add((x * 90 / PREVIEW_WIDTH) as u8 + grid);
            pixels[offset + 1] = 28_u8.saturating_add((y * 75 / PREVIEW_HEIGHT) as u8 + grid);
            pixels[offset + 2] = 52_u8.saturating_add(((x + y) * 80 / 1000) as u8 + grid);
            pixels[offset + 3] = 255;
        }
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(PREVIEW_WIDTH * 4),
            rows_per_image: Some(PREVIEW_HEIGHT),
        },
        wgpu::Extent3d {
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    slint::Image::try_from(texture).unwrap_or_else(|error| {
        eprintln!("failed to import Slint wgpu preview texture: {error}");
        process::exit(1);
    })
}
