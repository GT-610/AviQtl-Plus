// Reproduce the femtoVG-on-WARP crash with a real winit window, no Slint involved.
// Stages are printed so the crash location is unambiguous.
fn main() {
    let event_loop = winit::event_loop::EventLoop::new().expect("event loop");
    let window_attributes = winit::window::Window::default_attributes()
        .with_title("femtovg/wgpu WARP probe")
        .with_inner_size(winit::dpi::PhysicalSize::new(320u32, 200u32));
    let window = event_loop
        .create_window(window_attributes)
        .expect("window");
    println!("STAGE window_created");

    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(&window)
        .expect("surface from window");
    println!("STAGE surface_created");

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: Some(&surface),
    }))
    .expect("adapter");
    let info = adapter.get_info();
    println!(
        "STAGE adapter_selected name={:?} backend={:?} type={:?}",
        info.name, info.backend, info.device_type
    );

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("probe"),
            ..Default::default()
        },
    ))
    .expect("device");
    println!("STAGE device_created");

    let caps = surface.get_capabilities(&adapter);
    println!(
        "STAGE surface_capabilities formats={:?} alpha_modes={:?} usages={:?}",
        caps.formats, caps.alpha_modes, caps.usages
    );

    let mut config = surface
        .get_default_config(&adapter, 320, 200)
        .expect("default config");
    println!("STAGE default_config format={:?} usage={:?}", config.format, config.usage);
    // Mirror Slint's swapchain format preference.
    if let Some(format) = caps
        .formats
        .iter()
        .find(|f| matches!(f, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm))
        .copied()
    {
        config.format = format;
    }
    surface.configure(&device, &config);
    let capsules = surface.get_capabilities(&adapter);
    println!(
        "STAGE surface_capabilities formats={:?} alpha_modes={:?}",
        capsules.formats, capsules.alpha_modes
    );

    // Slint's Manual branch calls get_default_config and never rewrites the format.
    // Probe whether a pipeline survives for each advertised format.
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
            r#"
            struct VOut { @builtin(position) pos: vec4<f32>, @location(0) c: vec3<f32> };
            @vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
                var p = array<vec2<f32>, 3>(vec2<f32>(0.0,0.5), vec2<f32>(-0.5,-0.5), vec2<f32>(0.5,-0.5));
                var o: VOut;
                o.pos = vec4<f32>(p[i], 0.0, 1.0);
                o.c = vec3<f32>(1.0, 0.2, 0.1);
                return o;
            }
            @fragment fn fs(in: VOut) -> @location(0) vec4<f32> { return vec4<f32>(in.c, 1.0); }
            "#,
        )),
    });
    for format in &capsules.formats {
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("probe-pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: *format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        println!("STAGE pipeline_created format={:?}", format);
        let _ = &pipeline;
    }

    let mut config = surface
        .get_default_config(&adapter, 320, 200)
        .expect("default config");
    println!("STAGE default_config format={:?} usage={:?}", config.format, config.usage);
    surface.configure(&device, &config);
    println!("STAGE surface_configured");

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
                            let view = frame.texture.create_view(&Default::default());
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
                                                    load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
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
                                pass.draw(0..3, 0..1);
                            }
                            queue.submit([encoder.finish()]);
                            frame.present();
                            frames += 1;
                            if frames == 5 {
                                println!("RESULT frames_presented={frames}");
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
