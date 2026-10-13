//! Which of the application's compositor pipelines can a CPU/DX12-WARP device
//! compile?
//!
//! The compositor creates one effect pipeline plus four blend pipelines, all
//! from the same 41-case effect shader. This probe compiles one pipeline per
//! child process (a hard access violation kills the process) so each result is
//! attributable to a single pipeline kind.
//!
//! Usage: shader-pipeline-probe <vertex|effect|normal|screen|multiply|add|complex|vertex-only>

use std::borrow::Cow;

/// Reads the compositor's WGSL straight out of `aviqtl-render/src/lib.rs`, so the probe
/// needs no intermediate copy and runs from a clean checkout.
fn compositor_shader() -> String {
    fn repo_root() -> std::path::PathBuf {
        let mut current = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            if current.join("rust").join("Cargo.toml").is_file() {
                return current;
            }
            current = current
                .parent()
                .expect("the probe lives inside the repository")
                .to_path_buf();
        }
    }
    let path = repo_root().join("rust").join("aviqtl-render").join("src").join("lib.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    const OPEN: &str = "const SHADER: &str = r#\"";
    const CLOSE: &str = "\"#;";
    let start = source
        .find(OPEN)
        .unwrap_or_else(|| panic!("{} does not declare const SHADER", path.display()))
        + OPEN.len();
    let end = source[start..]
        .find(CLOSE)
        .unwrap_or_else(|| panic!("unterminated SHADER constant in {}", path.display()))
        + start;
    source[start..end].to_owned()
}

fn main() {
    let kind = std::env::args().nth(1).expect("usage: <pipeline kind>");
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("cpu adapter");
    let (device, _queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");
    println!("DEVICE_OK");

    let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let sampler_entry = wgpu::BindGroupLayoutEntry {
        binding: 1,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    };
    let uniform_entry = wgpu::BindGroupLayoutEntry {
        binding: 2,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let storage_entry = wgpu::BindGroupLayoutEntry {
        binding: 4,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("probe-layer-layout"),
        entries: &[
            texture_entry(0),
            sampler_entry,
            uniform_entry,
            texture_entry(3),
            storage_entry,
        ],
    });
    println!("LAYOUT_OK");

    let source = compositor_shader();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
    });
    println!("SHADER_OK");

    // The blend pipelines all use `vertex`/`fragment` with their own blend
    // state; the complex pipeline uses both bind group layouts.
    let (blend, bg_layout): (Option<wgpu::BlendState>, Option<&wgpu::BindGroupLayout>) = match kind.as_str() {
        "vertex" | "vertex-only" => (Some(wgpu::BlendState::ALPHA_BLENDING), None),
        "effect" => (None, None),
        "normal" => (
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            None,
        ),
        "multiply" => (
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            None,
        ),
        "screen" => (
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            None,
        ),
        "add" => (
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            None,
        ),
        "complex" => (Some(wgpu::BlendState::ALPHA_BLENDING), None),
        other => panic!("unknown kind {other}"),
    };

    let (vertex, fragment) = match kind.as_str() {
        "effect" => ("effect_vertex", "effect_fragment"),
        _ => ("vertex", "fragment"),
    };

    let mut layouts: Vec<Option<&wgpu::BindGroupLayout>> = vec![Some(&layer_layout)];
    if kind == "complex" {
        let background_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("probe-background-layout"),
                entries: &[texture_entry(0)],
            });
        println!("BG_LAYOUT_OK");
        // Leak the layout into the process lifetime so the reference stays valid.
        layouts.push(Some(Box::leak(Box::new(background_layout))));
    }
    let _ = bg_layout;

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("probe-pipeline-layout"),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    println!("PIPELINE_LAYOUT_OK");

    let _ = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("probe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(vertex),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend,
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
    println!("RESULT kind={kind} compiled");
}
