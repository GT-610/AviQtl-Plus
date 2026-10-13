//! Does the Slint software backend plus a wgpu device coexist on a GPU-less host?
//! Stage A selects the Slint software backend and creates a window, then
//! builds the wgpu compositor resources on a CPU adapter. Stage B does the
//! same wgpu work *before* selecting the backend. Each stage prints a stage
//! line before and after every operation so the exact failing call is visible.

slint::slint! {
    export component ProbeWindow inherits Window {
        width: 320px;
        height: 200px;
        title: "backend order probe";
        in-out property <bool> done;
        VerticalLayout {
            alignment: center;
            padding: 16px;
            Text { text: "backend order probe"; horizontal-alignment: center; }
        }
    }
}

fn build_compositor_resources(tag: &str) {
    println!("STAGE {tag}: wgpu instance");
    let instance = wgpu::Instance::default();
    println!("STAGE {tag}: request adapter");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("cpu adapter");
    println!("STAGE {tag}: request device");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");
    println!("STAGE {tag}: create texture");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("probe-preview"),
        size: wgpu::Extent3d {
            width: 640,
            height: 360,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    println!("STAGE {tag}: bind group layout");
    let bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("probe-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
    println!("STAGE {tag}: shader module");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe-shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
            r#"
            struct VertexOutput {
                @builtin(position) position: vec4<f32>,
                @location(0) uv: vec2<f32>,
            };
            @group(0) @binding(0) var layer_texture: texture_2d<f32>;
            @group(0) @binding(1) var layer_sampler: sampler;
            @vertex
            fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
                var positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
                var out: VertexOutput;
                out.position = vec4<f32>(positions[index], 0.0, 1.0);
                out.uv = positions[index] * 0.5 + 0.5;
                return out;
            }
            @fragment
            fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
                return textureSample(layer_texture, layer_sampler, input.uv);
            }
            "#,
        )),
    });
    println!("STAGE {tag}: render pipeline");
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("probe-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("probe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    println!("STAGE {tag}: composer ok, texture width={}", texture.width());
    // keep the queue alive across the boundary
    drop(queue);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let order = std::env::args().nth(1).unwrap_or_else(|| "backend-first".to_owned());
    println!("ORDER {order}");

    if order == "gpu-first" {
        println!("STAGE pre: build wgpu resources before backend selection");
        build_compositor_resources("pre");
        println!("STAGE pre: select software backend");
        slint::BackendSelector::new().select()?;
    } else {
        println!("STAGE pre: select software backend");
        slint::BackendSelector::new().select()?;
    }

    println!("STAGE create window");
    let app = ProbeWindow::new()?;
    println!("STAGE window created");

    if order == "backend-first" {
        println!("STAGE post: build wgpu resources after backend selection and window creation");
        build_compositor_resources("post");
    }

    println!("STAGE hide window to end the run");
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_secs(6),
        move || {
            if let Some(app) = weak.upgrade() {
                let _ = app.hide();
            }
        },
    );

    println!("STAGE run event loop");
    app.run()?;
    println!("RESULT order={order} completed");
    Ok(())
}
