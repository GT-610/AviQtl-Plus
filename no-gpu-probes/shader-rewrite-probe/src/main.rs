//! Does replacing `textureSample` with `textureSampleLevel` inside the
//! dynamic-bound loops make the application's compositor shader compile on a
//! CPU/DX12-WARP device?
//!
//! The full shader fails with FXC's X3511 ("unable to unroll loop ... or
//! unrolled loop is too large") because a gradient instruction is used inside a
//! loop whose iteration count is not a compile-time constant. Loading with an
//! explicit level does not imply derivatives, so the loop no longer needs
//! unrolling. This probe rewrites the shader and recompiles it to confirm the
//! full shader then succeeds.

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
    let source = compositor_shader();
    // Some `textureSample(` calls span multiple lines. Work on the whole source
    // with a character iterator so parentheses are matched across newlines,
    // then splice the level argument in at each call's closing parenthesis.
    let marker = "textureSample(";
    let text: Vec<char> = source.chars().collect();
    let mut rewritten = String::with_capacity(source.len() + 512);
    let mut i = 0usize;
    let mut rewrote = 0usize;
    while i < text.len() {
        if text[i..].starts_with(&marker.chars().collect::<Vec<_>>()[..]) {
            // Find the matching close parenthesis.
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
            // Collapse newlines so a multi-line call's arguments stay on one
            // line, then trim any trailing comma the original left behind.
            let collapsed = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
            let trimmed = collapsed.trim_end().trim_end_matches(',').to_string();
            rewritten.push_str(&format!("textureSampleLevel({trimmed}, 0.0)"));
            rewrote += 1;
            i = end + 1;
        } else {
            rewritten.push(text[i]);
            i += 1;
        }
    }
    println!("STAGE rewrote {rewrote} textureSample calls");

    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("cpu adapter");
    println!("ADAPTER {:?}", adapter.get_info());
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

    println!("STAGE create shader module (rewritten)");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe-shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(rewritten.clone())),
    });
    println!("STAGE shader module created");

    for (vertex, fragment) in [
        ("vertex", "fragment"),
        ("effect_vertex", "effect_fragment"),
    ] {
        println!("STAGE pipeline {vertex}/{fragment}");
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
        println!("OK pipeline {vertex}/{fragment}");
    }
    std::fs::write("../../.build_tmp/app_shader_rewritten.wgsl", &rewritten)
        .expect("write rewritten shader");
    println!("RESULT textureSampleLevel rewrite compiles the compositor shader on WARP");
}
