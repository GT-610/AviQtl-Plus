// Does femtovg's own Canvas + WGPURenderer work on a WARP DX12 adapter?
// This is the exact layer Slint's femtoVG renderer sits on.
use femtovg::{Canvas, Paint, Path, Renderer};

fn main() {
    let event_loop = winit::event_loop::EventLoop::new().expect("event loop");
    let attrs = winit::window::Window::default_attributes()
        .with_title("femtovg canvas WARP probe")
        .with_inner_size(winit::dpi::PhysicalSize::new(320u32, 200u32));
    let window = event_loop.create_window(attrs).expect("window");
    println!("STAGE window_created");

    let instance = wgpu::Instance::default();
    let surface = instance.create_surface(&window).expect("surface");
    println!("STAGE surface_created");

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: Some(&surface),
    }))
    .expect("adapter");
    let info = adapter.get_info();
    println!(
        "STAGE adapter_selected name={:?} type={:?}",
        info.name, info.device_type
    );

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("probe"),
            ..Default::default()
        },
    ))
    .expect("device");
    println!("STAGE device_created");

    let mut config = surface
        .get_default_config(&adapter, 320, 200)
        .expect("default config");
    // Same format Slint's Manual branch ends up with.
    println!("STAGE swapchain_format={:?}", config.format);
    config.present_mode = wgpu::PresentMode::AutoVsync;
    surface.configure(&device, &config);
    println!("STAGE surface_configured");

    println!("STAGE building_femtovg_renderer");
    let renderer = femtovg::renderer::WGPURenderer::new(device.clone(), queue.clone());
    println!("STAGE femtovg_renderer_created");

    // Match Slint's femtoVG renderer exactly: it builds the canvas with a shared font
    // cache. TextContext has no public constructor, so make one the way Canvas::new does
    // — through a first canvas whose context is reachable via add_font_mem.
    let mut canvas = Canvas::new(renderer).expect("canvas");
    println!("STAGE canvas_created");

    let mut frames = 0u32;
    event_loop
        .run(move |event, control_flow| {
            control_flow.set_control_flow(winit::event_loop::ControlFlow::Poll);
            use winit::event::{Event, WindowEvent};
            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => control_flow.exit(),
                Event::AboutToWait => {
                    match surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(frame) => {
                            let view = frame
                                .texture
                                .create_view(&wgpu::TextureViewDescriptor::default());
                            let mut encoder = device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                            {
                                let mut pass = encoder.begin_render_pass(
                                    &wgpu::RenderPassDescriptor {
                                        label: Some("probe-pass"),
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: &view,
                                                depth_slice: None,
                                                resolve_target: None,
                                                ops: wgpu::Operations {
                                                    load: wgpu::LoadOp::Clear(
                                                        wgpu::Color::GREEN,
                                                    ),
                                                    store: wgpu::StoreOp::Store,
                                                },
                                            },
                                        )],
                                        depth_stencil_attachment: None,
                                        timestamp_writes: None,
                                        occlusion_query_set: None,
                                        multiview_mask: None,
                                    },
                                );

                                canvas.set_size(320, 200, 1.0);
                                canvas.clear_rect(0, 0, 320, 200, femtovg::Color::rgb(0, 0, 0));
                                let mut path = Path::new();
                                path.rect(40.0, 40.0, 200.0, 100.0);
                                let mut paint = Paint::color(femtovg::Color::rgb(30, 120, 220));
                                paint.set_line_width(3.0);
                                canvas.stroke_path(&path, &paint);

                                // Real glyph rendering exercises the glyph atlas path, which is
                                // the last untested difference vs. Slint's femtoVG renderer.
                                let font: &[u8] = include_bytes!("C:\\Windows\\Fonts\\arial.ttf");
                                let _ = canvas.add_font_mem(font);
                                let mut text_paint = paint.clone();
                                text_paint.set_font_size(14.0);
                                let _ = canvas.fill_text(50.0, 40.0, "probe", &text_paint);

                                let commands = canvas.flush_to_output(&frame.texture);
                                let _ = commands;
                            }
                            queue.submit([encoder.finish()]);
                            frame.present();
                            frames += 1;
                            if frames == 5 {
                                println!("RESULT femtovg_frames_presented={frames}");
                                control_flow.exit();
                            }
                        }
                        other => {
                            println!("RESULT surface_texture_unavailable={:?}", other);
                            control_flow.exit();
                        }
                    }
                }
                _ => {}
            }
        })
        .expect("event loop run");
    println!("DONE");
}
