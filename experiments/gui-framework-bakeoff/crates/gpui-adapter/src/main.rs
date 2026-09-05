#![forbid(unsafe_code)]

mod text_input;

use std::{
    env, process,
    time::{Duration, Instant},
};

use aviqtl_gui_lab_core::{
    BenchmarkConfig, FrameMetrics, MetricsCollector, RunMetadata, ScriptedWorkload,
    TimelineDataset, VisibleClip, rustc_version, write_report,
};
#[cfg(target_os = "macos")]
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::CFString,
};
#[cfg(target_os = "macos")]
use core_video::pixel_buffer::{
    CVPixelBuffer, CVPixelBufferKeys, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use gpui::{
    AnyElement, App, Bounds, Context, Entity, FontWeight, Render, SharedString, Task,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, rgba, size,
};

use crate::text_input::{TextInput, TextInputEditor, install_key_bindings};

const WINDOW_WIDTH: f32 = 1_440.0;
const WINDOW_HEIGHT: f32 = 900.0;
const TOOLBAR_HEIGHT: f32 = 48.0;
const TIMELINE_HEIGHT: f32 = 390.0;
const TIMELINE_HEADER_HEIGHT: f32 = 30.0;
const PREVIEW_WIDTH: usize = 640;
const PREVIEW_HEIGHT: usize = 360;
const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);

fn main() {
    let (config, interactive) = parse_configuration();
    gpui_ce_platform::application().run(move |cx: &mut App| {
        install_key_bindings(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("AviQtl GUI bake-off — GPUI-CE".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| GpuiLab::new(config, interactive, window, cx)),
        ) {
            eprintln!("failed to open GPUI-CE experiment window: {error}");
            process::exit(1);
        }
        cx.activate(true);
    });
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

struct GpuiLab {
    config: BenchmarkConfig,
    interactive: bool,
    dataset: TimelineDataset,
    workload: ScriptedWorkload,
    visible: Vec<VisibleClip>,
    metrics: Option<MetricsCollector>,
    previous_frame: Option<Instant>,
    selected_clip: Option<u32>,
    effect_name: Entity<TextInputEditor>,
    project_filter: Entity<TextInputEditor>,
    opacity: f32,
    position: [f32; 2],
    _frame_task: Task<()>,
    #[cfg(target_os = "macos")]
    preview_buffer: CVPixelBuffer,
}

impl GpuiLab {
    fn new(
        config: BenchmarkConfig,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let dataset = TimelineDataset::synthetic(config.dataset);
        let workload = ScriptedWorkload::new(dataset.total_frames(), config.dataset.layer_count);
        let metrics = MetricsCollector::new(
            RunMetadata {
                framework: "GPUI-CE".to_owned(),
                framework_version: "0.2.2".to_owned(),
                renderer: gpui_renderer_description().to_owned(),
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
        let frame_task = cx.spawn(async move |this, cx| {
            let mut next_tick = Instant::now();
            loop {
                next_tick += FRAME_INTERVAL;
                let wait = next_tick.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    cx.background_executor().timer(wait).await;
                }
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
                if Instant::now().saturating_duration_since(next_tick) >= FRAME_INTERVAL {
                    next_tick = Instant::now();
                }
            }
        });

        Self {
            config,
            interactive,
            dataset,
            workload,
            visible: Vec::with_capacity(512),
            metrics: Some(metrics),
            previous_frame: None,
            selected_clip: None,
            effect_name: cx
                .new(|cx| TextInputEditor::new("色調補正 / Color correction", window, cx)),
            project_filter: cx.new(|cx| TextInputEditor::new("素材を検索 / 搜索素材", window, cx)),
            opacity: 0.82,
            position: [0.0, 0.0],
            _frame_task: frame_task,
            #[cfg(target_os = "macos")]
            preview_buffer: create_preview_buffer(),
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = if self.interactive {
            "interactive"
        } else {
            "benchmark"
        };
        div()
            .h(px(TOOLBAR_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(rgb(0x20242d))
            .border_b_1()
            .border_color(rgb(0x363b47))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("AviQtl 2 UI laboratory"),
            )
            .child(toolbar_button("＋ メディア追加"))
            .child(toolbar_button("分割"))
            .child(toolbar_button("元に戻す"))
            .child(
                div()
                    .ml_auto()
                    .text_sm()
                    .text_color(rgb(0xaeb6c8))
                    .child(format!("Frame {:.0}", self.workload.playhead_frame())),
            )
            .child(
                toolbar_button("補助プレビュー")
                    .on_click(cx.listener(|this, _, _window, cx| this.open_auxiliary_window(cx))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if self.interactive {
                        rgb(0x78dba9)
                    } else {
                        rgb(0x72b7ff)
                    })
                    .child(mode),
            )
    }

    fn project_panel(&self) -> impl IntoElement {
        let assets = [
            ("▣", "scene_001.mp4", "3840×2160 · 60fps"),
            ("♫", "voice_main.wav", "48kHz · stereo"),
            ("◆", "タイトル", "テキストオブジェクト"),
            ("▧", "背景.png", "4096×2160"),
        ];
        div()
            .w(px(220.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(rgb(0x1d2028))
            .border_r_1()
            .border_color(rgb(0x343945))
            .child(section_heading("プロジェクト / 项目"))
            .child(TextInput::new(self.project_filter.clone()))
            .children(assets.into_iter().map(|(icon, name, detail)| {
                div()
                    .flex()
                    .gap_2()
                    .child(div().text_lg().child(icon))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(name)
                            .child(div().text_xs().text_color(rgb(0x8f98aa)).child(detail)),
                    )
            }))
            .child(
                div()
                    .mt_auto()
                    .text_xs()
                    .text_color(rgb(0x8f98aa))
                    .child(format!(
                        "{} clips · {} layers",
                        self.config.dataset.clip_count, self.config.dataset.layer_count
                    )),
            )
    }

    fn preview_panel(&self) -> impl IntoElement {
        div()
            .flex_1()
            .h_full()
            .min_w(px(240.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .bg(rgb(0x151820))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xaeb6c8))
                    .child(gpui_preview_label()),
            )
            .child(self.preview_element())
    }

    fn inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self
            .selected_clip
            .map_or_else(|| "none".to_owned(), |id| id.to_string());
        let frame = self.workload.frame_index() as f32;
        div()
            .w(px(280.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(rgb(0x1d2028))
            .border_l_1()
            .border_color(rgb(0x343945))
            .child(section_heading("エフェクト / 效果"))
            .child(format!("Selected clip: {selected}"))
            .child(property_label("Effect name"))
            .child(TextInput::new(self.effect_name.clone()))
            .child(self.adjuster("Opacity", self.opacity, Property::Opacity, cx))
            .child(self.adjuster("Position X", self.position[0], Property::PositionX, cx))
            .child(self.adjuster("Position Y", self.position[1], Property::PositionY, cx))
            .child(
                div()
                    .mt_2()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(0x242934))
                    .text_sm()
                    .child(format!(
                        "Exposure {:.2}\nTemperature {:.0} K\nScale 100% · Rotation 0°",
                        (frame * 0.01).sin(),
                        6_500.0 + frame.sin() * 400.0
                    )),
            )
            .child(
                div()
                    .mt_auto()
                    .text_xs()
                    .text_color(rgb(0x8f98aa))
                    .child("IME probe: focus either text field and enter 日本語 / 中文"),
            )
    }

    fn adjuster(
        &self,
        label: &'static str,
        value: f32,
        property: Property,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(82.0)).text_sm().child(label))
            .child(
                small_button("−").on_click(cx.listener(move |this, _, _, cx| {
                    this.adjust_property(property, -0.05);
                    cx.notify();
                })),
            )
            .child(
                div()
                    .w(px(78.0))
                    .text_sm()
                    .text_color(rgb(0xcbd3e4))
                    .child(format!("{value:.2}")),
            )
            .child(
                small_button("+").on_click(cx.listener(move |this, _, _, cx| {
                    this.adjust_property(property, 0.05);
                    cx.notify();
                })),
            )
    }

    fn adjust_property(&mut self, property: Property, delta: f32) {
        match property {
            Property::Opacity => self.opacity = (self.opacity + delta).clamp(0.0, 1.0),
            Property::PositionX => self.position[0] += delta * 100.0,
            Property::PositionY => self.position[1] += delta * 100.0,
        }
    }

    fn timeline(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = self.workload.viewport();
        let mut children = Vec::<AnyElement>::with_capacity(self.visible.len() + 80);
        let tick_step = if viewport.pixels_per_frame < 0.8 {
            120_i64
        } else {
            30_i64
        };
        let first_tick = viewport.first_frame.floor() as i64 / tick_step * tick_step;
        let last_frame = viewport.last_frame().ceil() as i64;
        for frame in (first_tick..=last_frame).step_by(tick_step as usize) {
            let x = ((frame as f64 - viewport.first_frame) * f64::from(viewport.pixels_per_frame))
                as f32;
            children.push(
                div()
                    .absolute()
                    .left(px(x))
                    .top_0()
                    .bottom_0()
                    .w(px(1.0))
                    .bg(rgb(0x343945))
                    .child(
                        div()
                            .absolute()
                            .left(px(4.0))
                            .top(px(3.0))
                            .text_xs()
                            .text_color(rgb(0xaeb6c8))
                            .child(frame.to_string()),
                    )
                    .into_any(),
            );
        }
        for row in 0..viewport.visible_layer_count() {
            children.push(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(
                        TIMELINE_HEADER_HEIGHT + row as f32 * viewport.layer_height_px
                    ))
                    .h(px(1.0))
                    .bg(rgb(0x303540))
                    .into_any(),
            );
        }
        for clip in &self.visible {
            let id = clip.id;
            let color = u32::from(clip.color[0]) << 24
                | u32::from(clip.color[1]) << 16
                | u32::from(clip.color[2]) << 8
                | u32::from(clip.color[3]);
            children.push(
                div()
                    .id(("clip", id as usize))
                    .absolute()
                    .left(px(clip.x_px))
                    .top(px(TIMELINE_HEADER_HEIGHT + clip.y_px))
                    .w(px(clip.width_px))
                    .h(px(clip.height_px))
                    .px_1()
                    .rounded_sm()
                    .overflow_hidden()
                    .bg(rgba(color))
                    .text_xs()
                    .text_color(rgb(0x0c0d10))
                    .cursor_pointer()
                    .when(clip.width_px > 44.0, |element| {
                        element.child(format!("C{}", clip.id))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_clip = Some(id);
                        cx.notify();
                    }))
                    .into_any(),
            );
        }
        let playhead_x = ((self.workload.playhead_frame() - viewport.first_frame)
            * f64::from(viewport.pixels_per_frame)) as f32;
        if (0.0..=viewport.width_px).contains(&playhead_x) {
            children.push(
                div()
                    .absolute()
                    .left(px(playhead_x))
                    .top_0()
                    .bottom_0()
                    .w(px(2.0))
                    .bg(rgb(0xff4c60))
                    .into_any(),
            );
        }

        div()
            .h(px(TIMELINE_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(0x191c23))
            .border_t_1()
            .border_color(rgb(0x363b47))
            .child(
                div()
                    .h(px(TIMELINE_HEADER_HEIGHT))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .text_sm()
                    .child("Timeline")
                    .child(format!("visible {}", self.visible.len()))
                    .child(format!("zoom {:.2}px/f", viewport.pixels_per_frame)),
            )
            .child(
                div()
                    .id("timeline-canvas")
                    .relative()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .bg(rgb(0x191c23))
                    .children(children),
            )
    }

    fn open_auxiliary_window(&mut self, cx: &mut Context<Self>) {
        if !self.interactive {
            return;
        }
        #[cfg(target_os = "macos")]
        let preview = self.preview_buffer.clone();
        let bounds = Bounds::centered(None, size(px(720.0), px(450.0)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("AviQtl auxiliary preview".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|_| AuxiliaryPreview {
                    #[cfg(target_os = "macos")]
                    preview,
                })
            },
        );
        if let Err(error) = result {
            eprintln!("failed to open auxiliary preview: {error}");
        }
    }

    #[cfg(target_os = "macos")]
    fn preview_element(&self) -> AnyElement {
        gpui::surface(self.preview_buffer.clone())
            .w_full()
            .h_full()
            .max_w(px(PREVIEW_WIDTH as f32))
            .max_h(px(PREVIEW_HEIGHT as f32))
            .into_any()
    }

    #[cfg(not(target_os = "macos"))]
    fn preview_element(&self) -> AnyElement {
        div()
            .w(px(PREVIEW_WIDTH as f32))
            .h(px(PREVIEW_HEIGHT as f32))
            .bg(rgb(0x23304a))
            .flex()
            .items_center()
            .justify_center()
            .child("Preview surface probe is macOS-only in this experiment")
            .into_any()
    }

    fn finish_if_ready(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
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
            serde_json::to_string_pretty(&report).expect("serialize GPUI-CE report")
        );
        if !self.interactive {
            window.on_next_frame(|window, cx| {
                window.remove_window();
                cx.quit();
            });
        }
    }
}

impl Render for GpuiLab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame_started = Instant::now();
        let frame_interval = self
            .previous_frame
            .replace(frame_started)
            .map(|previous| frame_started.saturating_duration_since(previous));
        self.workload.advance();

        let window_size = window.viewport_size();
        self.workload.resize(
            f32::from(window_size.width).max(1.0),
            (TIMELINE_HEIGHT - TIMELINE_HEADER_HEIGHT).max(1.0),
        );
        let query_started = Instant::now();
        self.dataset
            .query_visible(self.workload.viewport(), &mut self.visible);
        let query_cpu = query_started.elapsed();

        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x151820))
            .text_color(rgb(0xe4e8f0))
            .text_sm()
            .child(self.toolbar(cx))
            .child(
                div()
                    .w_full()
                    .h(px((f32::from(window_size.height)
                        - TOOLBAR_HEIGHT
                        - TIMELINE_HEIGHT)
                        .max(120.0)))
                    .flex_none()
                    .flex()
                    .child(self.project_panel())
                    .child(self.preview_panel())
                    .child(self.inspector(cx)),
            )
            .child(self.timeline(cx));

        if let Some(metrics) = &mut self.metrics {
            metrics.record(FrameMetrics {
                frame_cpu: frame_started.elapsed(),
                query_cpu,
                frame_interval,
                visible_clips: self.visible.len(),
            });
        }
        self.finish_if_ready(window, cx);
        root
    }
}

struct AuxiliaryPreview {
    #[cfg(target_os = "macos")]
    preview: CVPixelBuffer,
}

impl Render for AuxiliaryPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .bg(rgb(0x151820))
            .text_color(rgb(0xe4e8f0))
            .child("Independent preview window");
        #[cfg(target_os = "macos")]
        let root = root.child(
            gpui::surface(self.preview.clone())
                .w_full()
                .h_full()
                .max_w(px(PREVIEW_WIDTH as f32))
                .max_h(px(PREVIEW_HEIGHT as f32)),
        );
        #[cfg(not(target_os = "macos"))]
        let root = root.child("CVPixelBuffer preview path is available on macOS");
        root
    }
}

#[derive(Clone, Copy)]
enum Property {
    Opacity,
    PositionX,
    PositionY,
}

fn toolbar_button(label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(label)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0x303642))
        .hover(|style| style.bg(rgb(0x3a4251)))
        .cursor_pointer()
        .child(label)
}

fn small_button(label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(label)
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(rgb(0x303642))
        .hover(|style| style.bg(rgb(0x3a4251)))
        .cursor_pointer()
        .child(label)
}

fn section_heading(label: &'static str) -> gpui::Div {
    div()
        .text_lg()
        .font_weight(FontWeight::SEMIBOLD)
        .child(label)
}

fn property_label(label: &'static str) -> gpui::Div {
    div().text_xs().text_color(rgb(0x8f98aa)).child(label)
}

#[cfg(target_os = "macos")]
fn create_preview_buffer() -> CVPixelBuffer {
    let empty_properties: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[]);
    let options: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[
        (
            CFString::from(CVPixelBufferKeys::IOSurfaceProperties),
            empty_properties.as_CFType(),
        ),
        (
            CFString::from(CVPixelBufferKeys::MetalCompatibility),
            CFBoolean::true_value().as_CFType(),
        ),
    ]);
    CVPixelBuffer::new(
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        PREVIEW_WIDTH,
        PREVIEW_HEIGHT,
        Some(&options),
    )
    .unwrap_or_else(|status| {
        eprintln!("failed to create the CVPixelBuffer preview probe: {status}");
        process::exit(1);
    })
}

#[cfg(target_os = "macos")]
fn gpui_renderer_description() -> &'static str {
    "native Metal / CVPixelBuffer via CVMetalTextureCache"
}

#[cfg(not(target_os = "macos"))]
fn gpui_renderer_description() -> &'static str {
    "platform renderer / preview interoperability not exercised"
}

#[cfg(target_os = "macos")]
fn gpui_preview_label() -> SharedString {
    "Preview — caller-provided CVPixelBuffer, no CPU readback".into()
}

#[cfg(not(target_os = "macos"))]
fn gpui_preview_label() -> SharedString {
    "Preview — platform-specific surface probe".into()
}
