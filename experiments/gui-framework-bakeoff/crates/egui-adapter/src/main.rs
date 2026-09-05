#![forbid(unsafe_code)]

use std::{env, fs, path::Path, process, sync::Arc, time::Instant};

use aviqtl_gui_lab_core::{
    BenchmarkConfig, FrameMetrics, MetricsCollector, RunMetadata, ScriptedWorkload,
    TimelineDataset, VisibleClip, rustc_version, write_report,
};
use eframe::{egui, wgpu};

const PREVIEW_WIDTH: u32 = 640;
const PREVIEW_HEIGHT: u32 = 360;

fn main() {
    let (config, interactive) = parse_configuration();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("aviqtl-gui-bakeoff-egui")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        centered: true,
        persist_window: false,
        ..Default::default()
    };

    if let Err(error) = eframe::run_native(
        "AviQtl GUI bake-off — egui",
        options,
        Box::new(move |context| Ok(Box::new(EguiLab::new(context, config, interactive)))),
    ) {
        eprintln!("failed to run egui adapter: {error}");
        process::exit(1);
    }
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

struct EguiLab {
    config: BenchmarkConfig,
    interactive: bool,
    dataset: TimelineDataset,
    workload: ScriptedWorkload,
    visible: Vec<VisibleClip>,
    metrics: Option<MetricsCollector>,
    previous_frame: Option<Instant>,
    preview_texture_id: egui::TextureId,
    _preview_texture: wgpu::Texture,
    selected_clip: Option<u32>,
    project_filter: String,
    effect_name: String,
    opacity: f32,
    position: [f32; 2],
    auxiliary_window: bool,
}

impl EguiLab {
    fn new(
        context: &eframe::CreationContext<'_>,
        config: BenchmarkConfig,
        interactive: bool,
    ) -> Self {
        install_cjk_font(&context.egui_ctx);
        context.egui_ctx.set_visuals(egui::Visuals::dark());

        let render_state = context
            .wgpu_render_state
            .as_ref()
            .expect("the egui experiment requires the wgpu renderer");
        let preview_texture = create_preview_texture(&render_state.device, &render_state.queue);
        let preview_view = preview_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let preview_texture_id = render_state.renderer.write().register_native_texture(
            &render_state.device,
            &preview_view,
            wgpu::FilterMode::Linear,
        );

        let dataset = TimelineDataset::synthetic(config.dataset);
        let workload = ScriptedWorkload::new(dataset.total_frames(), config.dataset.layer_count);
        let metrics = MetricsCollector::new(
            RunMetadata {
                framework: "egui".to_owned(),
                framework_version: "0.36.1".to_owned(),
                renderer: "wgpu 30 / caller-owned texture".to_owned(),
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
            preview_texture_id,
            _preview_texture: preview_texture,
            selected_clip: None,
            project_filter: String::new(),
            effect_name: "色調補正 / Color correction".to_owned(),
            opacity: 0.82,
            position: [0.0, 0.0],
            auxiliary_window: false,
        }
    }

    fn draw_toolbar(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(root, |ui| {
            ui.horizontal(|ui| {
                ui.heading("AviQtl 2 UI laboratory");
                ui.separator();
                let _ = ui.button("＋ メディア追加");
                let _ = ui.button("分割");
                let _ = ui.button("元に戻す");
                ui.separator();
                ui.label(format!("Frame {:.0}", self.workload.playhead_frame()));
                if ui.button("補助プレビュー").clicked() {
                    self.auxiliary_window = !self.auxiliary_window;
                }
                if self.interactive {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, "interactive");
                } else {
                    ui.colored_label(egui::Color32::LIGHT_BLUE, "benchmark");
                }
            });
        });
    }

    fn draw_project_panel(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("project")
            .default_size(220.0)
            .resizable(true)
            .show(root, |ui| {
                ui.heading("プロジェクト / 项目");
                ui.add(
                    egui::TextEdit::singleline(&mut self.project_filter)
                        .hint_text("素材を検索 / 搜索素材"),
                );
                ui.separator();
                for (icon, name, detail) in [
                    ("▣", "scene_001.mp4", "3840×2160 · 60fps"),
                    ("♫", "voice_main.wav", "48kHz · stereo"),
                    ("◆", "タイトル", "テキストオブジェクト"),
                    ("▧", "背景.png", "4096×2160"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(icon);
                        ui.vertical(|ui| {
                            ui.label(name);
                            ui.small(detail);
                        });
                    });
                    ui.add_space(6.0);
                }
                ui.separator();
                ui.small(format!(
                    "{} clips · {} layers",
                    self.config.dataset.clip_count, self.config.dataset.layer_count
                ));
            });
    }

    fn draw_inspector(&mut self, root: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .default_size(280.0)
            .resizable(true)
            .show(root, |ui| {
                ui.heading("エフェクト / 效果");
                ui.label(format!(
                    "Selected clip: {}",
                    self.selected_clip
                        .map_or_else(|| "none".to_owned(), |id| id.to_string())
                ));
                ui.separator();
                ui.label("Effect name");
                ui.text_edit_singleline(&mut self.effect_name);
                ui.add(egui::Slider::new(&mut self.opacity, 0.0..=1.0).text("Opacity"));
                ui.add(
                    egui::Slider::new(&mut self.position[0], -1920.0..=1920.0).text("Position X"),
                );
                ui.add(
                    egui::Slider::new(&mut self.position[1], -1080.0..=1080.0).text("Position Y"),
                );
                ui.collapsing("Color correction", |ui| {
                    let frame = self.workload.frame_index() as f32;
                    ui.label(format!("Exposure {:.2}", (frame * 0.01).sin()));
                    ui.label(format!("Temperature {:.0} K", 6500.0 + frame.sin() * 400.0));
                });
                ui.collapsing("Transform", |ui| {
                    ui.label("Scale 100%");
                    ui.label("Rotation 0°");
                    ui.label("Blend: Normal");
                });
                ui.separator();
                ui.label("IME probe");
                ui.text_edit_multiline(&mut self.project_filter);
            });
    }

    fn draw_preview(&self, root: &mut egui::Ui) {
        egui::CentralPanel::default().show(root, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Preview — caller-owned wgpu texture");
            });
            let available = ui.available_size();
            let aspect = PREVIEW_WIDTH as f32 / PREVIEW_HEIGHT as f32;
            let mut size = egui::vec2(available.x, available.x / aspect);
            if size.y > available.y {
                size = egui::vec2(available.y * aspect, available.y);
            }
            ui.centered_and_justified(|ui| {
                ui.add(
                    egui::Image::new((
                        self.preview_texture_id,
                        egui::vec2(PREVIEW_WIDTH as f32, PREVIEW_HEIGHT as f32),
                    ))
                    .fit_to_exact_size(size)
                    .alt_text("AviQtl compositor preview probe"),
                );
            });
        });
    }

    fn draw_timeline(&mut self, root: &mut egui::Ui) -> std::time::Duration {
        let mut query_cpu = std::time::Duration::ZERO;
        egui::Panel::bottom("timeline")
            .default_size(390.0)
            .min_size(220.0)
            .resizable(true)
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Timeline");
                    ui.label(format!("visible {}", self.visible.len()));
                    ui.label(format!(
                        "zoom {:.2}px/f",
                        self.workload.viewport().pixels_per_frame
                    ));
                });

                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), egui::Sense::click());
                let ruler_height = 24.0;
                self.workload.resize(
                    response.rect.width(),
                    (response.rect.height() - ruler_height).max(1.0),
                );
                let query_started = Instant::now();
                self.dataset
                    .query_visible(self.workload.viewport(), &mut self.visible);
                query_cpu = query_started.elapsed();

                painter.rect_filled(response.rect, 0.0, egui::Color32::from_rgb(25, 28, 35));
                let content_top = response.rect.top() + ruler_height;
                let viewport = self.workload.viewport();
                let tick_step = if viewport.pixels_per_frame < 0.8 {
                    120
                } else {
                    30
                };
                let first_tick = (viewport.first_frame as i64 / tick_step) * tick_step;
                let last_frame = viewport.last_frame().ceil() as i64;
                for frame in (first_tick..=last_frame).step_by(tick_step as usize) {
                    let x = response.rect.left()
                        + ((frame as f64 - viewport.first_frame)
                            * f64::from(viewport.pixels_per_frame))
                            as f32;
                    painter.line_segment(
                        [
                            egui::pos2(x, response.rect.top()),
                            egui::pos2(x, response.rect.bottom()),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_gray(48)),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, response.rect.top() + 3.0),
                        egui::Align2::LEFT_TOP,
                        frame,
                        egui::FontId::monospace(10.0),
                        egui::Color32::LIGHT_GRAY,
                    );
                }
                for row in 0..viewport.visible_layer_count() {
                    let y = content_top + row as f32 * viewport.layer_height_px;
                    painter.line_segment(
                        [
                            egui::pos2(response.rect.left(), y),
                            egui::pos2(response.rect.right(), y),
                        ],
                        egui::Stroke::new(1.0, egui::Color32::from_gray(44)),
                    );
                }

                let clicked = response
                    .clicked()
                    .then(|| response.interact_pointer_pos())
                    .flatten();
                for clip in &self.visible {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(response.rect.left() + clip.x_px, content_top + clip.y_px),
                        egui::vec2(clip.width_px, clip.height_px),
                    )
                    .intersect(response.rect);
                    let color = egui::Color32::from_rgba_unmultiplied(
                        clip.color[0],
                        clip.color[1],
                        clip.color[2],
                        clip.color[3],
                    );
                    painter.rect_filled(rect, 3.0, color);
                    if clip.width_px > 44.0 {
                        painter.text(
                            rect.left_center() + egui::vec2(5.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("C{}", clip.id),
                            egui::FontId::monospace(10.0),
                            egui::Color32::BLACK,
                        );
                    }
                    if clicked.is_some_and(|position| rect.contains(position)) {
                        self.selected_clip = Some(clip.id);
                    }
                }

                let playhead_x = response.rect.left()
                    + ((self.workload.playhead_frame() - viewport.first_frame)
                        * f64::from(viewport.pixels_per_frame)) as f32;
                if response.rect.left() <= playhead_x && playhead_x <= response.rect.right() {
                    painter.line_segment(
                        [
                            egui::pos2(playhead_x, response.rect.top()),
                            egui::pos2(playhead_x, response.rect.bottom()),
                        ],
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 76, 96)),
                    );
                }
            });
        query_cpu
    }

    fn draw_auxiliary_window(&mut self, context: &egui::Context) {
        if !self.auxiliary_window {
            return;
        }
        let texture = self.preview_texture_id;
        let close_requested = context.show_viewport_immediate(
            egui::ViewportId::from_hash_of("auxiliary-preview"),
            egui::ViewportBuilder::default()
                .with_title("AviQtl auxiliary preview")
                .with_inner_size([720.0, 450.0]),
            move |ui, _class| {
                ui.heading("Independent preview window");
                ui.add(egui::Image::new((texture, egui::vec2(640.0, 360.0))));
                ui.input(|input| input.viewport().close_requested())
            },
        );
        if close_requested {
            self.auxiliary_window = false;
        }
    }

    fn finish_if_ready(&mut self, context: &egui::Context) {
        if !self
            .metrics
            .as_ref()
            .is_some_and(MetricsCollector::is_complete)
        {
            return;
        }
        let report = self.metrics.take().expect("metrics exist").finish();
        if let Some(path) = self.config.report_path.as_deref() {
            write_report(path, &report).unwrap_or_else(|error| {
                eprintln!("failed to write {}: {error}", path.display());
                process::exit(1);
            });
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("serialize egui report")
        );
        if !self.interactive {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for EguiLab {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_started = Instant::now();
        let frame_interval = self
            .previous_frame
            .replace(frame_started)
            .map(|previous| frame_started.saturating_duration_since(previous));
        self.workload.advance();
        let context = root.ctx().clone();

        self.draw_toolbar(root);
        let query_cpu = self.draw_timeline(root);
        self.draw_project_panel(root);
        self.draw_inspector(root);
        self.draw_preview(root);
        self.draw_auxiliary_window(&context);

        if let Some(metrics) = &mut self.metrics {
            metrics.record(FrameMetrics {
                frame_cpu: frame_started.elapsed(),
                query_cpu,
                frame_interval,
                visible_clips: self.visible.len(),
            });
        }
        self.finish_if_ready(&context);
        context.request_repaint();
    }
}

fn install_cjk_font(context: &egui::Context) {
    let font_path = Path::new("/System/Library/Fonts/Hiragino Sans GB.ttc");
    let Ok(bytes) = fs::read(font_path) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let name = "system-cjk".to_owned();
    fonts
        .font_data
        .insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(fallbacks) = fonts.families.get_mut(&family) {
            fallbacks.push(name.clone());
        }
    }
    context.set_fonts(fonts);
}

fn create_preview_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aviqtl-egui-preview-probe"),
        size: wgpu::Extent3d {
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
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
    texture
}
