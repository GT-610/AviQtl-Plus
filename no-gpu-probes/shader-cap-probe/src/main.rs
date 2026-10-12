//! Narrow the CPU/DX12-WARP pipeline-compilation crash in the application's
//! compositor shader to a single construct.
//!
//! Two knobs are applied independently so their effect is separable:
//! * `rewrite_samples` turns every `textureSample` into
//!   `textureSampleLevel(..., 0.0)`.
//! * `cap` bounds the effect-strip loop with a compile-time constant.
//!
//! Each variant is compiled in a freshly created child process, because a hard
//! access violation (0xC0000005) kills the whole process and cannot be caught.

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

const REPLACEMENT: &str =
    "let count = min(u32(effect_chain.header.x), arrayLength(&effect_chain.values) / 4u);";

fn build_shader(cap: Option<u32>, rewrite_samples: bool) -> String {
    let source = compositor_shader();
    let source = match cap {
        Some(cap) => source.replace(
            REPLACEMENT,
            &format!(
                "let count = min(
                min(u32(effect_chain.header.x), arrayLength(&effect_chain.values) / 4u),
                {cap}u
            );"
            ),
        ),
        None => source,
    };
    if !rewrite_samples {
        return source;
    }
    let marker = "textureSample(";
    let text: Vec<char> = source.chars().collect();
    let mut rewritten = String::with_capacity(source.len() + 512);
    let mut i = 0usize;
    while i < text.len() {
        if text[i..].starts_with(&marker.chars().collect::<Vec<_>>()[..]) {
            let body = i + marker.chars().count();
            let mut depth = 1usize;
            let mut end = None;
            let mut j = body;
            while j < text.len() {
                match text[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let end = end.expect("unterminated textureSample call");
            let call: String = text[i..=end].iter().collect();
            let arguments = &call[marker.len()..call.len() - 1];
            let collapsed = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
            let trimmed = collapsed.trim_end().trim_end_matches(',').to_string();
            rewritten.push_str(&format!("textureSampleLevel({trimmed}, 0.0)"));
            i = end + 1;
        } else {
            rewritten.push(text[i]);
            i += 1;
        }
    }
    rewritten
}

fn compile(shader_source: &str, entry: (&str, &str)) {
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
    let storage_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("probe-layout"),
        entries: &[
            texture_entry(0),
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
            texture_entry(3),
            storage_entry(4),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("probe-pipeline-layout"),
        bind_group_layouts: &[Some(&layer_layout)],
        immediate_size: 0,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(shader_source.to_owned())),
    });
    let _ = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("probe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(entry.0),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(entry.1),
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
}

fn main() {
    let kind = std::env::args().nth(1).unwrap_or_default();
    let (cap, rewrite) = match kind.as_str() {
        "baseline" => (None, false),
        "samples" => (None, true),
        "cap" => (Some(1), false),
        "both" => (Some(1), true),
        _ => {
            eprintln!("usage: shader-cap-probe <baseline|samples|cap|both>");
            std::process::exit(2);
        }
    };
    let source = build_shader(cap, rewrite);
    compile(&source, ("vertex", "fragment"));
    println!("OK kind={kind}");
}
