//! Bisect the application's compositor WGSL against a CPU/DX12-WARP device.
//!
//! The full shader hard-crashes (0xC0000005) inside `create_render_pipeline`.
//! This probe compiles progressively larger prefixes of the `apply_visual_effects`
//! switch so the exact effect case that first triggers the compiler failure is
//! identified by number.
//!
//! The prefix shader is assembled from the parts that surround the switch: the
//! real header/bindings, a stub `apply_visual_effects` that closes after the
//! given number of cases, and the real vertex/fragment entry points.

use std::borrow::Cow;

fn build_shader(cases_to_keep: usize) -> String {
    let source = std::fs::read_to_string(
        "../../.build_tmp/app_shader_rewritten.wgsl",
    )
    .expect("read extracted shader");
    let lines: Vec<&str> = source.lines().collect();
    // Find the region of the switch inside apply_visual_effects.
    let fn_start = lines
        .iter()
        .position(|line| line.contains("fn apply_visual_effects"))
        .expect("apply_visual_effects");
    let switch_start = lines
        .iter()
        .enumerate()
        .skip(fn_start)
        .find(|(_, line)| line.contains("switch"))
        .map(|(index, _)| index)
        .expect("switch");
    let mut depth = 0i32;
    let mut switch_end = switch_start;
    for (offset, line) in lines.iter().enumerate().skip(switch_start) {
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if offset > switch_start && depth <= 0 {
            switch_end = offset;
            break;
        }
    }
    // Count the `case N:` lines inside the switch so they can be truncated.
    let mut case_positions: Vec<usize> = Vec::new();
    for (index, line) in lines.iter().enumerate().take(switch_end).skip(switch_start) {
        if line.trim_start().starts_with("case ") && line.trim_end().ends_with(": {") {
            case_positions.push(index);
        }
    }
    case_positions.push(switch_end);

    let mut output = String::new();
    for line in &lines[..switch_start] {
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("    switch kind {\n");
    for (index, start) in case_positions.iter().enumerate() {
        if index >= cases_to_keep {
            break;
        }
        let end = case_positions[index + 1];
        for line in &lines[*start..end] {
            output.push_str(line);
            output.push('\n');
        }
    }
    output.push_str("        default: {}\n    }\n");
    for line in &lines[switch_end + 1..] {
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn main() {
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

    let total_cases = {
        let source =
            std::fs::read_to_string("../../.build_tmp/app_shader_rewritten.wgsl").expect("read");
        source
            .lines()
            .filter(|line| {
                line.trim_start().starts_with("case ") && line.trim_end().ends_with(": {")
            })
            .count()
    };
    println!("TOTAL_CASES {total_cases}");

    let mut last_good = 0usize;
    for cases in 1..=total_cases {
        let shader_source = build_shader(cases);
        println!("TRY cases={cases} shader_bytes={}", shader_source.len());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("probe-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(shader_source)),
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let pipeline =
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("probe-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment"),
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
            let _ = pipeline;
        }));
        if result.is_err() {
            eprintln!("PANIC at cases={cases}");
            break;
        }
        last_good = cases;
        println!("OK cases={cases}");
        if cases == total_cases {
            break;
        }
    }
    println!("RESULT last_good_cases={last_good}");
}
