fn main() {
    println!("STAGE instance");
    let instance = wgpu::Instance::default();
    println!("STAGE enumerate adapters");
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    for adapter in &adapters {
        let info = adapter.get_info();
        println!("ADAPTER {} ({:?}, {:?})", info.name, info.backend, info.device_type);
    }
    println!("STAGE request cpu adapter");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("cpu adapter");
    println!("STAGE adapter info {:?}", adapter.get_info());
    println!("STAGE request device");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");
    println!("STAGE device ok; limits max_texture_dimension_2d={} max_bind_groups={}",
        device.limits().max_texture_dimension_2d, device.limits().max_bind_groups);

    println!("STAGE create texture");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("probe-preview"),
        size: wgpu::Extent3d { width: 640, height: 360, depth_or_array_layers: 1 },
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
    let _ = queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[0u8; 640 * 360 * 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(640 * 4),
            rows_per_image: Some(360),
        },
        wgpu::Extent3d { width: 640, height: 360, depth_or_array_layers: 1 },
    );
    println!("STAGE texture written");

    // Replicate the compositor's bind group layout: filtered float texture +
    // filtering sampler + uniform buffer, the exact BindingTypes the app uses.
    println!("STAGE bind group layout");
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    println!("STAGE bind group layout ok");

    println!("STAGE shader module");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe-shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
            r#"
            struct Layer {
                transform: mat4x4<f32>,
                color: vec4<f32>,
            };
            @group(0) @binding(0) var layer_texture: texture_2d<f32>;
            @group(0) @binding(1) var layer_sampler: sampler;
            @group(0) @binding(2) var<uniform> layer: Layer;
            struct VertexOutput {
                @builtin(position) position: vec4<f32>,
                @location(0) uv: vec2<f32>,
            };
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
                let sampled = textureSample(layer_texture, layer_sampler, input.uv);
                return sampled * layer.color;
            }
            "#,
        )),
    });
    println!("STAGE shader module ok");

    println!("STAGE render pipeline");
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
    println!("STAGE render pipeline ok");

    // Readback, exactly as the app's preview does.
    println!("STAGE readback");
    let bytes_per_row = 640u32 * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe-readback"),
        size: u64::from(bytes_per_row) * 360,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("probe-encoder") });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(360),
            },
        },
        wgpu::Extent3d { width: 640, height: 360, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result.map_err(|error| error.to_string()));
    });
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    receiver.recv().expect("recv").expect("map");
    let mapped = buffer.slice(..).get_mapped_range();
    println!("READBACK first 8 bytes: {:?}", &mapped[..8]);
    drop(mapped);

    // Silence unused import warning while keeping DeviceExt available if needed.
    let _ = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe-uniform"),
        size: 128,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    println!("RESULT compositor cpu path fully succeeded");
}

