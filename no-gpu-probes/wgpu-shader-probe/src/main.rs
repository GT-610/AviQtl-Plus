// Does wgpu 29 + DX12 WARP survive a WGSL pipeline (which invokes d3dcompiler_47)?
fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: None,
    }))
    .expect("cpu adapter");
    let (device, _queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("probe"),
            ..Default::default()
        },
    ))
    .expect("device");
    println!("RESULT device_created");

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe-shader"),
        source: wgpu::ShaderSource::Wgsl(
            std::borrow::Cow::Borrowed(
                r#"
                struct VertexOut {
                    @builtin(position) position: vec4<f32>,
                    @location(0) uv: vec2<f32>,
                };

                @vertex
                fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
                    var positions = array<vec2<f32>, 3>(
                        vec2<f32>(-1.0, -3.0),
                        vec2<f32>(-1.0,  1.0),
                        vec2<f32>( 3.0,  1.0),
                    );
                    var out: VertexOut;
                    out.position = vec4<f32>(positions[index], 0.0, 1.0);
                    out.uv = positions[index];
                    return out;
                }

                struct Uniforms {
                    mip_level: f32,
                    _pad: vec3<f32>,
                };
                @group(0) @binding(0) var<uniform> uniforms: Uniforms;
                @group(0) @binding(1) var source: texture_2d<f32>;
                @group(0) @binding(2) var source_sampler: sampler;

                @fragment
                fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
                    let dims = textureDimensions(source);
                    let level = min(uniforms.mip_level, f32(dims.x) * 0.5);
                    return textureSampleLevel(source, source_sampler, in.uv * 0.5 + 0.5, level);
                }
                "#,
            ),
        ),
    });
    println!("RESULT shader_module_created");

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("probe-source"),
        size: wgpu::Extent3d {
            width: 64,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("probe-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("probe-pl"),
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
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    let _ = &texture;
    println!("RESULT pipeline_created");
}
