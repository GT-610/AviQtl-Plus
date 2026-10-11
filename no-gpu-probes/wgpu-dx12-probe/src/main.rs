// Isolate the wgpu 29 + DX12 WARP failure on this host.
fn main() {
    println!("--- enumerate adapters (all backends) ---");
    let instance = wgpu::Instance::default();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    for adapter in &adapters {
        let info = adapter.get_info();
        println!(
            "  {:?} backend={:?} type={:?} vendor={:?}",
            info.name, info.backend, info.device_type, info.vendor
        );
    }

    println!("--- request fallback (CPU) adapter and exercise the texture copy path ---");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: None,
    }))
    .expect("cpu adapter");
    let info = adapter.get_info();
    println!(
        "  selected {:?} backend={:?} type={:?}",
        info.name, info.backend, info.device_type
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("probe"),
        ..Default::default()
    }))
    .expect("device");
    println!("  device created");

    let width = 64u32;
    let height = 32u32;
    let bytes_per_row = width * 4;
    let padded_bytes_per_row = bytes_per_row
        .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("probe-texture"),
        size: wgpu::Extent3d {
            width,
            height,
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
    let pixels: Vec<u8> = (0..(width * height * 4) as usize)
        .map(|i| (i % 251) as u8)
        .collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe-readback"),
        size: (padded_bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("probe-enc") });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("map callback")
        .expect("map readback");
    println!("  readback mapped");

    let data = slice.get_mapped_range();
    let mut mismatch = 0usize;
    for row in 0..height as usize {
        let start = row * padded_bytes_per_row as usize;
        let end = start + bytes_per_row as usize;
        for (index, actual) in data[start..end].iter().enumerate() {
            let expected = ((row * bytes_per_row as usize + index) % 251) as u8;
            if *actual != expected {
                mismatch += 1;
            }
        }
    }
    drop(data);
    println!("RESULT copy_roundtrip_mismatches={mismatch}");
}
