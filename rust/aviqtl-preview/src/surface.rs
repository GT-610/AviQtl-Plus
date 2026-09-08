use crate::decode::{DecodedContent, DecodedScene};
use aviqtl_media::VideoFrame;
use aviqtl_render::{
    CompositionLayer, CompositionMask, CompositionSize, CompositionSource, Compositor,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc;
use wgpu;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

pub struct PreviewSurface {
    device: wgpu::Device,
    queue: wgpu::Queue,
    compositor: Compositor,
    texture: wgpu::Texture,
    readback_buffer: Option<wgpu::Buffer>,
    readback_capacity: u64,
    nested_surfaces: HashMap<u64, SceneSurface>,
    size: (u32, u32),
    render_scale: f32,
    msaa_samples: u32,
}

impl PreviewSurface {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let pixels = placeholder_pixels();
        let texture = create_texture(&device, &queue, WIDTH, HEIGHT, &pixels);
        Self {
            compositor: Compositor::new(&device, wgpu::TextureFormat::Rgba8Unorm),
            device,
            queue,
            texture,
            readback_buffer: None,
            readback_capacity: 0,
            nested_surfaces: HashMap::new(),
            size: (WIDTH, HEIGHT),
            render_scale: 1.0,
            msaa_samples: 1,
        }
    }

    pub fn set_render_scale(&mut self, render_scale: f32) -> bool {
        let render_scale = if render_scale.is_finite() {
            render_scale.clamp(0.25, 1.0)
        } else {
            1.0
        };
        if (self.render_scale - render_scale).abs() <= f32::EPSILON {
            return false;
        }
        self.render_scale = render_scale;
        true
    }

    pub fn set_msaa_samples(&mut self, msaa_samples: u32) -> bool {
        let msaa_samples = match msaa_samples {
            2 | 4 | 8 => msaa_samples,
            _ => 1,
        };
        if self.msaa_samples == msaa_samples {
            return false;
        }
        self.msaa_samples = msaa_samples;
        self.compositor = Compositor::new_with_sample_count(
            &self.device,
            wgpu::TextureFormat::Rgba8Unorm,
            msaa_samples,
        );
        self.nested_surfaces.clear();
        true
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn compose(&mut self, scene: &DecodedScene, generation: u64) -> bool {
        let logical_size = (scene.width.max(1), scene.height.max(1));
        let (width, height) = scaled_preview_size(logical_size, self.render_scale);
        let target_replaced = self.texture.width() != width || self.texture.height() != height;
        if target_replaced {
            self.replace_target(width, height);
        }
        let mut active_nested = HashSet::new();
        let prepared = self.prepare_layers(scene, generation, &mut active_nested);
        let layers = composition_layers(&prepared, generation);
        self.compositor.render_scaled(
            &self.device,
            &self.queue,
            &self.texture,
            CompositionSize {
                physical: (width, height),
                logical: logical_size,
            },
            &layers,
            scene.camera.as_ref(),
        );
        self.nested_surfaces
            .retain(|key, _| active_nested.contains(key));
        target_replaced
    }

    pub fn read_rgba(&mut self) -> Result<Vec<u8>, String> {
        let width = self.texture.width();
        let height = self.texture.height();
        let unpadded_bytes_per_row = width.saturating_mul(4);
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(alignment) * alignment;
        let required_capacity = u64::from(padded_bytes_per_row) * u64::from(height);
        if self.readback_capacity < required_capacity {
            self.readback_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aviqtl-preview-readback"),
                size: required_capacity,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.readback_capacity = required_capacity;
        }
        let buffer = self
            .readback_buffer
            .as_ref()
            .ok_or_else(|| "GPU readback buffer was not created".to_owned())?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aviqtl-preview-readback-encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer,
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
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result.map_err(|error| error.to_string()));
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| error.to_string())?;
        receiver
            .recv()
            .map_err(|_| "GPU readback callback was dropped".to_owned())??;
        let mapped = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
            rgba.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(rgba)
    }

    fn prepare_layers<'a>(
        &mut self,
        scene: &'a DecodedScene,
        generation: u64,
        active_nested: &mut HashSet<u64>,
    ) -> Vec<PreparedLayer<'a>> {
        scene
            .layers
            .iter()
            .map(|layer| {
                let source = match &layer.content {
                    DecodedContent::Frame(frame) => PreparedSource::Frame(frame),
                    DecodedContent::Scene(scene) => {
                        active_nested.insert(scene.instance_key);
                        PreparedSource::Texture {
                            texture: self.render_nested_scene(scene, generation, active_nested),
                            size: (scene.width.max(1), scene.height.max(1)),
                        }
                    }
                };
                PreparedLayer {
                    cache_key: layer.cache_key,
                    source,
                    mask: layer.mask.as_deref().map(|scene| {
                        active_nested.insert(scene.instance_key);
                        PreparedMask {
                            cache_key: scene.instance_key,
                            texture: self.render_nested_scene(scene, generation, active_nested),
                            size: (scene.width.max(1), scene.height.max(1)),
                        }
                    }),
                    timeline_layer: layer.timeline_layer,
                    transform: layer.transform,
                    blend_mode: layer.blend_mode,
                    crop: layer.crop,
                    effects: &layer.effects,
                }
            })
            .collect()
    }

    fn render_nested_scene(
        &mut self,
        scene: &DecodedScene,
        generation: u64,
        active_nested: &mut HashSet<u64>,
    ) -> Arc<wgpu::Texture> {
        let size = (scene.width.max(1), scene.height.max(1));
        let prepared = self.prepare_layers(scene, generation, active_nested);
        let layers = composition_layers(&prepared, generation);
        let msaa_samples = self.msaa_samples;
        let surface = self
            .nested_surfaces
            .entry(scene.instance_key)
            .or_insert_with(|| SceneSurface::new(&self.device, &self.queue, size, msaa_samples));
        surface.ensure_size(&self.device, &self.queue, size);
        if scene.opaque_background {
            surface.compositor.render_opaque_black(
                &self.device,
                &self.queue,
                &surface.texture,
                size,
                &layers,
                scene.camera.as_ref(),
            );
        } else {
            surface.compositor.render_transparent(
                &self.device,
                &self.queue,
                &surface.texture,
                size,
                &layers,
                scene.camera.as_ref(),
            );
        }
        Arc::clone(&surface.texture)
    }

    fn replace_target(&mut self, width: u32, height: u32) {
        let pixels = vec![0_u8; width as usize * height as usize * 4];
        self.texture = create_texture(&self.device, &self.queue, width, height, &pixels);
        self.size = (width, height);
    }
}

fn scaled_preview_size(logical_size: (u32, u32), render_scale: f32) -> (u32, u32) {
    let render_scale = if render_scale.is_finite() {
        render_scale.clamp(0.25, 1.0)
    } else {
        1.0
    };
    (
        ((logical_size.0.max(1) as f32) * render_scale)
            .round()
            .max(1.0) as u32,
        ((logical_size.1.max(1) as f32) * render_scale)
            .round()
            .max(1.0) as u32,
    )
}

enum PreparedSource<'a> {
    Frame(&'a VideoFrame),
    Texture {
        texture: Arc<wgpu::Texture>,
        size: (u32, u32),
    },
}

struct PreparedLayer<'a> {
    cache_key: u64,
    source: PreparedSource<'a>,
    mask: Option<PreparedMask>,
    timeline_layer: i32,
    transform: aviqtl_render::LayerTransform,
    blend_mode: aviqtl_render::BlendMode,
    crop: aviqtl_render::LayerCrop,
    effects: &'a [aviqtl_render::VisualEffect],
}

struct PreparedMask {
    cache_key: u64,
    texture: Arc<wgpu::Texture>,
    size: (u32, u32),
}

fn composition_layers<'a>(
    prepared: &'a [PreparedLayer<'a>],
    generation: u64,
) -> Vec<CompositionLayer<'a>> {
    prepared
        .iter()
        .map(|layer| CompositionLayer {
            cache_key: layer.cache_key,
            source: match &layer.source {
                PreparedSource::Frame(frame) => CompositionSource::Frame(frame),
                PreparedSource::Texture { texture, size } => CompositionSource::Texture {
                    texture,
                    size: *size,
                    stamp: generation,
                },
            },
            timeline_layer: layer.timeline_layer,
            transform: layer.transform,
            blend_mode: layer.blend_mode,
            crop: layer.crop,
            mask: layer.mask.as_ref().map(|mask| CompositionMask {
                cache_key: mask.cache_key,
                texture: &mask.texture,
                size: mask.size,
            }),
            effects: layer.effects,
        })
        .collect()
}

struct SceneSurface {
    compositor: Compositor,
    texture: Arc<wgpu::Texture>,
    size: (u32, u32),
}

impl SceneSurface {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        msaa_samples: u32,
    ) -> Self {
        let pixels = vec![0_u8; size.0 as usize * size.1 as usize * 4];
        Self {
            compositor: Compositor::new_with_sample_count(
                device,
                wgpu::TextureFormat::Rgba8Unorm,
                msaa_samples,
            ),
            texture: Arc::new(create_texture(device, queue, size.0, size.1, &pixels)),
            size,
        }
    }

    fn ensure_size(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: (u32, u32)) {
        if self.size == size {
            return;
        }
        let pixels = vec![0_u8; size.0 as usize * size.1 as usize * 4];
        self.texture = Arc::new(create_texture(device, queue, size.0, size.1, &pixels));
        self.size = size;
    }
}

fn create_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aviqtl-preview"),
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
    write_pixels(queue, &texture, width, height, pixels);
    texture
}

fn placeholder_pixels() -> Vec<u8> {
    let mut pixels = vec![0_u8; WIDTH as usize * HEIGHT as usize * 4];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let offset = ((y * WIDTH + x) * 4) as usize;
            let checker = u8::from((x / 40 + y / 40) % 2 == 0) * 8;
            pixels[offset] = 31_u8.saturating_add((x * 26 / WIDTH) as u8 + checker);
            pixels[offset + 1] = 36_u8.saturating_add((y * 34 / HEIGHT) as u8 + checker);
            pixels[offset + 2] = 48_u8.saturating_add(checker);
            pixels[offset + 3] = 255;
        }
    }
    pixels
}

fn write_pixels(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    pixels: &[u8],
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_media::VideoFrame;
    use aviqtl_render::{BlendMode, LayerCrop, LayerTransform};

    #[test]
    fn preview_scale_preserves_aspect_and_qt_bounds() {
        assert_eq!(scaled_preview_size((1920, 1080), 1.0), (1920, 1080));
        assert_eq!(scaled_preview_size((1920, 1080), 0.75), (1440, 810));
        assert_eq!(scaled_preview_size((1920, 1080), 0.5), (960, 540));
        assert_eq!(scaled_preview_size((1920, 1080), 0.25), (480, 270));
        assert_eq!(scaled_preview_size((0, 0), f32::NAN), (1, 1));
    }

    #[test]
    #[ignore = "requires a real GPU adapter"]
    fn scaled_msaa_preview_composes_fixed_and_complex_blends_without_validation_errors() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("real GPU adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("GPU device");
        let frame = |rgba: [u8; 4]| VideoFrame {
            width: 64,
            height: 32,
            rgba: rgba.into_iter().cycle().take(64 * 32 * 4).collect(),
            timestamp_seconds: 0.0,
        };
        let scene = DecodedScene {
            instance_key: 1,
            width: 64,
            height: 32,
            camera: None,
            opaque_background: false,
            layers: vec![
                crate::decode::DecodedLayer {
                    cache_key: 1,
                    timeline_layer: 1,
                    transform: LayerTransform::default(),
                    blend_mode: BlendMode::Normal,
                    crop: LayerCrop::default(),
                    mask: None,
                    effects: Vec::new(),
                    content: DecodedContent::Frame(frame([255, 0, 0, 255])),
                },
                crate::decode::DecodedLayer {
                    cache_key: 2,
                    timeline_layer: 0,
                    transform: LayerTransform::default(),
                    blend_mode: BlendMode::Overlay,
                    crop: LayerCrop::default(),
                    mask: None,
                    effects: Vec::new(),
                    content: DecodedContent::Frame(frame([0, 0, 255, 128])),
                },
            ],
        };
        let mut surface = PreviewSurface::new(device.clone(), queue);
        assert!(surface.set_render_scale(0.5));
        assert!(surface.set_msaa_samples(4));

        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        assert!(surface.compose(&scene, 1));
        assert_eq!(surface.size(), (32, 16));
        let pixels = surface.read_rgba().expect("scaled MSAA preview readback");
        let validation_error = pollster::block_on(error_scope.pop());

        assert!(validation_error.is_none(), "{validation_error:?}");
        assert_eq!(pixels.len(), 32 * 16 * 4);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
    }
}
