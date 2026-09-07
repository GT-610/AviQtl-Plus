//! wgpu composition independent of egui and window-system types.

mod effects;
mod procedural;
mod shape;
mod text;

pub use effects::VisualEffect;
pub use procedural::{ProceduralRasterError, rasterize_procedural_object};
pub use shape::{ShapeRasterError, rasterize_shape};
pub use text::{TextRasterError, TextRasterizer};

use aviqtl_media::VideoFrame;
use aviqtl_rust_core::api::CameraRenderPlan;
use effects::{encode_effect_chain, encode_effect_pass};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct LayerUniform {
    corner_bottom_left: vec4<f32>,
    corner_bottom_right: vec4<f32>,
    corner_top_left: vec4<f32>,
    corner_top_right: vec4<f32>,
    crop_uv: vec4<f32>,
    opacity_blend_visible: vec4<f32>,
};

struct EffectChain {
    header: vec4<f32>,
    values: array<vec4<f32>>,
};

@group(0) @binding(0) var layer_texture: texture_2d<f32>;
@group(0) @binding(1) var layer_sampler: sampler;
@group(0) @binding(2) var<uniform> layer: LayerUniform;
@group(0) @binding(3) var mask_texture: texture_2d<f32>;
@group(0) @binding(4) var<storage, read> effect_chain: EffectChain;
@group(1) @binding(0) var background_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec4<f32>, 6>(
        layer.corner_bottom_left, layer.corner_bottom_right, layer.corner_top_left,
        layer.corner_top_left, layer.corner_bottom_right, layer.corner_top_right
    );
    let uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0)
    );
    var output: VertexOutput;
    output.position = positions[index];
    output.uv = uvs[index];
    return output;
}

fn effect_rgb_to_hsl(color: vec3<f32>) -> vec3<f32> {
    let high = max(max(color.r, color.g), color.b);
    let low = min(min(color.r, color.g), color.b);
    let delta = high - low;
    let lightness = (high + low) * 0.5;
    var saturation_value = 0.0;
    var hue_value = 0.0;
    if delta > 0.0001 {
        saturation_value = delta / (1.0 - abs(2.0 * lightness - 1.0));
        if high == color.r {
            hue_value = fract((color.g - color.b) / delta / 6.0);
        } else if high == color.g {
            hue_value = ((color.b - color.r) / delta + 2.0) / 6.0;
        } else {
            hue_value = ((color.r - color.g) / delta + 4.0) / 6.0;
        }
    }
    return vec3<f32>(hue_value, saturation_value, lightness);
}

fn effect_hue_to_rgb(p: f32, q: f32, input_t: f32) -> f32 {
    var t = input_t;
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    return p;
}

fn effect_hsl_to_rgb(color: vec3<f32>) -> vec3<f32> {
    let q = select(
        color.z + color.y - color.z * color.y,
        color.z * (1.0 + color.y),
        color.z < 0.5
    );
    let p = 2.0 * color.z - q;
    return vec3<f32>(
        effect_hue_to_rgb(p, q, color.x + 1.0 / 3.0),
        effect_hue_to_rgb(p, q, color.x),
        effect_hue_to_rgb(p, q, color.x - 1.0 / 3.0)
    );
}

fn effect_rgb_to_hsv(color: vec3<f32>) -> vec3<f32> {
    let high = max(max(color.r, color.g), color.b);
    let low = min(min(color.r, color.g), color.b);
    let delta = high - low;
    var hue_value = 0.0;
    if delta > 0.0000000001 {
        if high == color.r {
            hue_value = fract((color.g - color.b) / delta / 6.0);
        } else if high == color.g {
            hue_value = ((color.b - color.r) / delta + 2.0) / 6.0;
        } else {
            hue_value = ((color.r - color.g) / delta + 4.0) / 6.0;
        }
    }
    let saturation_value = select(delta / high, 0.0, high <= 0.0000000001);
    return vec3<f32>(hue_value, saturation_value, high);
}

fn effect_hsv_to_rgb(color: vec3<f32>) -> vec3<f32> {
    let p = abs(fract(color.xxx + vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return color.z * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), color.y);
}

fn effect_hash(point: vec2<f32>, seed: f32) -> f32 {
    var p3 = fract(vec3<f32>(point.x, point.y, point.x) * 0.1031 + vec3<f32>(seed * 0.01));
    p3 += vec3<f32>(dot(p3, p3.yzx + vec3<f32>(33.33)));
    return fract((p3.x + p3.y) * p3.z);
}

fn effect_sine_hash(point: vec2<f32>) -> f32 {
    return fract(sin(dot(point, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn effect_glitch_hash(point: vec2<f32>) -> f32 {
    return fract(sin(dot(point, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn effect_pixel_luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn effect_circle_mask(uv: vec2<f32>, center: vec2<f32>, radius: f32) -> f32 {
    return smoothstep(radius, radius - 0.01, length(uv - center));
}

fn effect_rect_mask(uv: vec2<f32>, center: vec2<f32>, size: vec2<f32>) -> f32 {
    let distance = abs(uv - center) - size;
    return smoothstep(0.01, 0.0, max(distance.x, distance.y));
}

fn effect_star_mask(uv: vec2<f32>, center: vec2<f32>, radius: f32) -> f32 {
    let delta = uv - center;
    let angle = atan2(delta.y, delta.x);
    let distance = length(delta);
    let edge = radius * (0.5 + 0.5 * cos(5.0 * angle));
    return smoothstep(edge, edge - 0.01, distance);
}

fn effect_heart_mask(uv: vec2<f32>, center: vec2<f32>, size: f32) -> f32 {
    var point = (uv - center) / size;
    point.y = -point.y;
    let base = point.x * point.x + point.y * point.y - 1.0;
    let curve = base * base * base - point.x * point.x * point.y * point.y * point.y;
    return smoothstep(0.01, 0.0, curve);
}

fn effect_sample_value(uv: vec2<f32>, use_luma: f32, use_alpha: f32) -> f32 {
    let sampled = textureSample(layer_texture, layer_sampler, uv);
    if use_alpha > 0.5 {
        return sampled.a;
    }
    if use_luma > 0.5 {
        return dot(sampled.rgb, vec3<f32>(0.299, 0.587, 0.114));
    }
    return (sampled.r + sampled.g + sampled.b) * 0.333;
}

fn apply_visual_effects(
    sampled: vec4<f32>,
    uv: vec2<f32>,
    target_size: vec2<f32>,
) -> vec4<f32> {
    var color = sampled;
    let count = min(u32(effect_chain.header.x), arrayLength(&effect_chain.values) / 4u);
    for (var index = 0u; index < count; index += 1u) {
        let base = index * 4u;
        let first = effect_chain.values[base];
        let second = effect_chain.values[base + 1u];
        let third = effect_chain.values[base + 2u];
        let fourth = effect_chain.values[base + 3u];
        let kind = u32(first.x);
        switch kind {
            case 1u: {
                color.a *= first.y;
            }
            case 2u: {
                let luma = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
                var gray = vec3<f32>(luma);
                if first.z <= 0.5 {
                    let base_luma = dot(third.rgb, vec3<f32>(0.299, 0.587, 0.114));
                    if base_luma > 0.001 {
                        gray = third.rgb * (luma / base_luma);
                    }
                }
                color = vec4<f32>(mix(color.rgb, gray, first.y), color.a);
            }
            case 3u: {
                var hsl = effect_rgb_to_hsl(color.rgb);
                hsl.x = fract(hsl.x + first.w / 360.0);
                hsl.y = clamp(hsl.y * (1.0 + second.y), 0.0, 1.0);
                hsl.z = clamp(hsl.z + first.y + second.x * (1.0 - hsl.z), 0.0, 1.0);
                var rgb = effect_hsl_to_rgb(hsl);
                rgb = clamp((rgb - 0.5) * (1.0 + first.z) + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
                if second.z > 0.5 {
                    rgb = clamp(rgb, vec3<f32>(16.0 / 255.0), vec3<f32>(235.0 / 255.0));
                }
                color = vec4<f32>(rgb, color.a);
            }
            case 4u: {
                let adjusted = color.rgb * first.yzw;
                var hsv = effect_rgb_to_hsv(adjusted);
                hsv.x = fract(hsv.x + second.x / 360.0);
                hsv.y = clamp(hsv.y * second.y, 0.0, 1.0);
                hsv.z = clamp(hsv.z * second.z, 0.0, 1.0);
                color = vec4<f32>(effect_hsv_to_rgb(hsv), color.a);
            }
            case 5u: {
                let luma = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
                var key_mask = smoothstep(first.y, first.y - first.z, luma);
                if first.w > 0.5 {
                    key_mask = 1.0 - key_mask;
                }
                color.a *= key_mask;
            }
            case 6u: {
                let difference = abs(color.rgb - third.rgb);
                let maximum = max(max(difference.r, difference.g), difference.b);
                var key_mask = smoothstep(first.y, first.y - first.z, maximum);
                if first.w > 0.5 {
                    key_mask = 1.0 - key_mask;
                }
                color.a *= key_mask;
            }
            case 7u: {
                let hsl = effect_rgb_to_hsl(color.rgb);
                var hue_difference = abs(hsl.x - first.y);
                if hue_difference > 0.5 {
                    hue_difference = 1.0 - hue_difference;
                }
                let hue_mask = 1.0 - smoothstep(first.z, first.z + second.x, hue_difference);
                let saturation_mask = smoothstep(0.0, first.w * 0.5, hsl.y);
                var key = hue_mask * saturation_mask;
                if second.y > 0.5 {
                    key = 1.0 - key;
                }
                color.a *= 1.0 - key;
            }
            case 8u: {
                let hsv = effect_rgb_to_hsv(color.rgb);
                var hue_difference = abs(hsv.x - first.y);
                if hue_difference > 0.5 {
                    hue_difference = 1.0 - hue_difference;
                }
                let range_mask = smoothstep(first.z, first.z * 0.5, hue_difference);
                color = vec4<f32>(mix(color.rgb, third.rgb, range_mask * first.w), color.a);
            }
            case 9u: {
                let offset = uv - first.zw;
                var t = 0.0;
                if second.z < 0.5 {
                    let angle = radians(second.x);
                    let direction = vec2<f32>(cos(angle), sin(angle));
                    t = dot(offset, direction) / max(second.y, 0.001) + 0.5;
                } else if second.z < 1.5 {
                    t = length(offset) / max(second.y, 0.001);
                } else {
                    t = max(abs(offset.x), abs(offset.y)) / max(second.y, 0.001);
                }
                t = clamp(t, 0.0, 1.0);
                var gradient = mix(third, fourth, t);
                gradient.a *= first.y;
                let result = mix(color.rgb, gradient.rgb, gradient.a);
                color = vec4<f32>(result, max(color.a, gradient.a));
            }
            case 10u: {
                let center = vec2<f32>(0.5);
                let time = fract(second.x * first.z * 0.1);
                var flash_effect = 0.0;
                if first.w < 0.5 {
                    flash_effect = smoothstep(time, time - 0.1, length(uv - center)) * first.y;
                } else if first.w < 1.5 {
                    flash_effect = smoothstep(time, time - 0.1, uv.x) * first.y;
                } else {
                    let ring = abs(length(uv - center) - time);
                    flash_effect = smoothstep(0.1, 0.0, ring) * first.y;
                }
                color = vec4<f32>(color.rgb + third.rgb * flash_effect * color.a, color.a);
            }
            case 11u: {
                let noise = effect_hash(floor(uv * 500.0 + vec2<f32>(first.w * 50.0)), first.z)
                    * 2.0 - 1.0;
                color = vec4<f32>(clamp(color.rgb + vec3<f32>(noise * first.y), vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
            }
            case 12u: {
                let centered = uv - vec2<f32>(0.5);
                let distance = length(centered) / (length(vec2<f32>(0.5)) * first.y);
                var vignette = smoothstep(1.0, 1.0 - first.z, distance);
                vignette = mix(1.0, vignette, first.w);
                color = vec4<f32>(color.rgb * vignette, color.a);
            }
            case 13u: {
                let light_position = second.xy;
                var light_effect = 0.0;
                if first.y < 0.5 {
                    light_effect = smoothstep(first.w, 0.0, length(uv - light_position)) * first.z;
                } else if first.y < 1.5 {
                    let direction = normalize(uv - light_position);
                    let angle = dot(direction, vec2<f32>(0.0, -1.0));
                    let spot_effect = smoothstep(0.7, 1.0, angle);
                    light_effect = spot_effect * smoothstep(first.w, 0.0, length(uv - light_position)) * first.z;
                } else {
                    light_effect = first.z * 0.5;
                }
                color = vec4<f32>(color.rgb + third.rgb * light_effect * color.a, color.a);
            }
            case 14u: {
                let center = vec2<f32>(0.5);
                var mask = 0.0;
                if first.y < 0.5 {
                    mask = effect_circle_mask(uv, center, 0.4);
                } else if first.y < 1.5 {
                    mask = effect_rect_mask(uv, center, vec2<f32>(0.3, 0.2));
                } else if first.y < 2.5 {
                    mask = effect_star_mask(uv, center, 0.4);
                } else {
                    mask = effect_heart_mask(uv, center, 0.4);
                }
                if first.z > 0.5 {
                    mask = 1.0 - mask;
                }
                color.a *= mask * first.w;
            }
            case 15u: {
                let pixel = uv * target_size;
                let origin = target_size * 0.5 + first.yz;
                let delta = pixel - origin;
                let angle = radians(first.w);
                let distance = delta.x * cos(angle) + delta.y * sin(angle);
                let blur = max(second.y, 0.001);
                var alpha_factor = 0.0;
                if second.x == 0.0 {
                    alpha_factor = smoothstep(-blur, 0.0, distance);
                } else if second.x > 0.0 {
                    alpha_factor = smoothstep(0.0, blur, second.x * 0.5 - abs(distance));
                } else {
                    alpha_factor = smoothstep(0.0, blur, abs(distance) - (-second.x * 0.5));
                }
                color *= alpha_factor;
            }
            case 16u: {
                var pixel = uv * target_size;
                let angle = radians(second.x);
                let centered = pixel - target_size * 0.5;
                let rotated = vec2<f32>(
                    centered.x * cos(angle) - centered.y * sin(angle),
                    centered.x * sin(angle) + centered.y * cos(angle),
                );
                pixel = rotated + target_size * 0.5;
                let time_offset = second.y * first.w * 0.1;
                let raster_x = sin((pixel.x + time_offset) * 3.14159 / first.y);
                let raster_y = sin((pixel.y + time_offset) * 3.14159 / first.z);
                color = vec4<f32>(color.rgb * (0.8 + 0.2 * raster_x * raster_y), color.a);
            }
            case 17u: {
                let texel = vec2<f32>(1.0) / target_size;
                let horizontal = vec2<f32>(texel.x * first.z, 0.0);
                let vertical = vec2<f32>(0.0, texel.y * first.z);
                let top = textureSample(layer_texture, layer_sampler, uv - vertical);
                let bottom = textureSample(layer_texture, layer_sampler, uv + vertical);
                let left = textureSample(layer_texture, layer_sampler, uv - horizontal);
                let right = textureSample(layer_texture, layer_sampler, uv + horizontal);
                color = clamp(
                    color * (1.0 + 4.0 * first.y) - (top + bottom + left + right) * first.y,
                    vec4<f32>(0.0),
                    vec4<f32>(1.0),
                );
            }
            case 18u: {
                let texel = vec2<f32>(1.0) / target_size;
                let angle = radians(first.w);
                let direction = vec2<f32>(cos(angle), sin(angle));
                let offset = texel * first.yz * direction;
                let light = textureSample(layer_texture, layer_sampler, uv + offset);
                let dark = textureSample(layer_texture, layer_sampler, uv - offset);
                let difference = light.rgb - dark.rgb;
                let value = 0.5 + (difference.r + difference.g + difference.b) * 0.333 * second.x;
                color = vec4<f32>(mix(color.rgb, vec3<f32>(value), second.x), color.a);
            }
            case 19u: {
                let texel = vec2<f32>(1.0) / target_size;
                let horizontal = vec2<f32>(texel.x, 0.0);
                let vertical = vec2<f32>(0.0, texel.y);
                let top_left = effect_sample_value(uv - horizontal - vertical, first.w, second.x);
                let top = effect_sample_value(uv - vertical, first.w, second.x);
                let top_right = effect_sample_value(uv + horizontal - vertical, first.w, second.x);
                let left = effect_sample_value(uv - horizontal, first.w, second.x);
                let right = effect_sample_value(uv + horizontal, first.w, second.x);
                let bottom_left = effect_sample_value(uv - horizontal + vertical, first.w, second.x);
                let bottom = effect_sample_value(uv + vertical, first.w, second.x);
                let bottom_right = effect_sample_value(uv + horizontal + vertical, first.w, second.x);
                let gradient_x = -top_left - 2.0 * left - bottom_left + top_right
                    + 2.0 * right + bottom_right;
                let gradient_y = -top_left - 2.0 * top - top_right + bottom_left
                    + 2.0 * bottom + bottom_right;
                var edge = sqrt(gradient_x * gradient_x + gradient_y * gradient_y) * first.y;
                edge = smoothstep(first.z, first.z + 0.05, edge);
                color = vec4<f32>(mix(color.rgb, third.rgb, edge), mix(color.a, 1.0, edge));
            }
            case 20u: {
                let texel = vec2<f32>(1.0) / target_size;
                let radius = i32(clamp(first.y, 0.0, 64.0));
                let sample_radius = radius * i32(clamp(first.z, 1.0, 3.0));
                if sample_radius >= 1 {
                    var sum = vec4<f32>(0.0);
                    var total = 0.0;
                    let sigma_squared = f32(sample_radius * sample_radius) * 2.0;
                    for (var offset_index = -sample_radius; offset_index <= sample_radius; offset_index += 1) {
                        let weight = exp(-f32(offset_index * offset_index) / sigma_squared);
                        var offset = vec2<f32>(0.0, f32(offset_index) * texel.y);
                        if second.w < 0.5 {
                            offset = vec2<f32>(f32(offset_index) * texel.x, 0.0);
                        }
                        sum += textureSample(layer_texture, layer_sampler, uv + offset) * weight;
                        total += weight;
                    }
                    color = sum / max(total, 0.0001);
                }
            }
            case 21u: {
                if second.w < 1.5 {
                    let texel = vec2<f32>(1.0) / target_size;
                    let base_radius = clamp(first.y, 0.0, 64.0);
                    let horizontal_ratio = select(1.0, (100.0 - first.z) / 100.0, first.z > 0.0);
                    let vertical_ratio = select(1.0, (100.0 + first.z) / 100.0, first.z < 0.0);
                    let ratio = select(vertical_ratio, horizontal_ratio, second.w < 0.5);
                    let radius = i32(base_radius * ratio);
                    if radius >= 1 {
                        var sum = vec4<f32>(0.0);
                        var total = 0.0;
                        let sigma_squared = f32(radius * radius) * 2.0;
                        for (var offset_index = -radius; offset_index <= radius; offset_index += 1) {
                            let weight = exp(-f32(offset_index * offset_index) / sigma_squared);
                            var offset = vec2<f32>(0.0, f32(offset_index) * texel.y);
                            if second.w < 0.5 {
                                offset = vec2<f32>(f32(offset_index) * texel.x, 0.0);
                            }
                            sum += textureSample(layer_texture, layer_sampler, uv + offset) * weight;
                            total += weight;
                        }
                        color = sum / max(total, 0.0001);
                    }
                } else {
                    let original = textureSample(mask_texture, layer_sampler, uv);
                    let edge = abs(original.a - color.a) * 4.0;
                    let mix_factor = clamp(edge, 0.0, 1.0);
                    let output_alpha = select(original.a, mix(original.a, color.a, mix_factor), first.w > 0.5);
                    color = vec4<f32>(mix(original.rgb, color.rgb, mix_factor), output_alpha);
                }
            }
            case 22u: {
                let angle = radians(first.y);
                let direction = vec2<f32>(cos(angle), -sin(angle));
                let step = direction / target_size * first.z;
                var sum = vec4<f32>(0.0);
                var total = 0.0;
                for (var sample_index = 0; sample_index < 16; sample_index += 1) {
                    let position = f32(sample_index) / 15.0 - 0.5;
                    let weight = max(1.0 - abs(position) * 1.6, 0.0);
                    sum += textureSample(layer_texture, layer_sampler, uv + step * position * 2.0)
                        * weight;
                    total += weight;
                }
                let blurred = sum / max(total, 0.0001);
                color = select(blurred, vec4<f32>(blurred.rgb, blurred.a * color.a), first.w > 0.5);
            }
            case 23u: {
                let center = vec2<f32>(
                    0.5 + first.w / target_size.x,
                    0.5 - second.x / target_size.y,
                );
                let direction = uv - center;
                let sample_count = min(64, max(1, i32(first.y)));
                let step = first.z / f32(sample_count) * 0.002;
                var sum = vec4<f32>(0.0);
                for (var sample_index = 0; sample_index < sample_count; sample_index += 1) {
                    let position = f32(sample_index) / f32(max(sample_count - 1, 1));
                    let sample_uv = uv - direction * position * step * f32(sample_count);
                    sum += textureSample(
                        layer_texture,
                        layer_sampler,
                        clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0)),
                    );
                }
                color = sum / f32(sample_count);
            }
            case 24u: {
                let sample_count = max(1, i32(first.y));
                let velocity = vec2<f32>(first.w / target_size.x, -second.x / target_size.y)
                    * first.z * 0.01;
                if dot(velocity, velocity) >= 0.000001 {
                    var sum = vec4<f32>(0.0);
                    var total = 0.0;
                    for (var sample_index = 0; sample_index < sample_count; sample_index += 1) {
                        let position = f32(sample_index) / f32(max(sample_count - 1, 1));
                        let weight = select(1.0, 1.0 - position * 0.7, second.y > 0.5);
                        let sample_uv = uv - velocity * position;
                        sum += textureSample(
                            layer_texture,
                            layer_sampler,
                            clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0)),
                        ) * weight;
                        total += weight;
                    }
                    color = sum / max(total, 0.0001);
                }
            }
            case 25u: {
                if first.y >= 0.5 {
                    let texel = vec2<f32>(first.y) / target_size;
                    var sum = vec4<f32>(0.0);
                    for (var sample_index = 0; sample_index < 64; sample_index += 1) {
                        let radius = sqrt(f32(sample_index + 1) / 65.0);
                        let angle = f32(sample_index) * 2.399963229728;
                        let offset = vec2<f32>(cos(angle), sin(angle)) * radius * texel;
                        let sampled = textureSample(layer_texture, layer_sampler, uv + offset);
                        let luma = dot(sampled.rgb, vec3<f32>(0.299, 0.587, 0.114));
                        let boost = 1.0 + first.z * luma * 0.05;
                        sum += sampled * boost;
                    }
                    color = sum / 64.0;
                }
            }
            case 26u: {
                let texel = vec2<f32>(1.0) / target_size;
                let radius = i32(max(1.0, first.y + first.z));
                var maximum_distance = 0.0;
                for (var offset_x = -10; offset_x <= 10; offset_x += 1) {
                    for (var offset_y = -10; offset_y <= 10; offset_y += 1) {
                        if offset_x * offset_x + offset_y * offset_y <= radius * radius {
                            let offset = vec2<f32>(f32(offset_x), f32(offset_y)) * texel;
                            let alpha = textureSample(layer_texture, layer_sampler, uv + offset).a;
                            if alpha > 0.1 {
                                maximum_distance = max(
                                    maximum_distance,
                                    sqrt(f32(offset_x * offset_x + offset_y * offset_y)),
                                );
                            }
                        }
                    }
                }
                let border_mask = smoothstep(
                    first.y + first.z,
                    first.y,
                    maximum_distance,
                ) * (1.0 - color.a);
                color = vec4<f32>(
                    mix(color.rgb, third.rgb, border_mask),
                    max(color.a, border_mask),
                );
            }
            case 27u: {
                let texel = vec2<f32>(1.0) / target_size;
                let steps = i32(clamp(first.z * 0.5, 1.0, 10.0));
                let step_size = first.z / f32(max(1, steps));
                var blurred = vec4<f32>(0.0);
                var total_weight = 0.0;
                for (var offset_x = -steps; offset_x <= steps; offset_x += 1) {
                    for (var offset_y = -steps; offset_y <= steps; offset_y += 1) {
                        let offset = vec2<f32>(f32(offset_x), f32(offset_y)) * step_size * texel;
                        let squared_distance = f32(offset_x * offset_x + offset_y * offset_y);
                        let weight = exp(-squared_distance / (f32(steps * steps) * 0.5 + 0.1));
                        blurred += textureSample(layer_texture, layer_sampler, uv + offset) * weight;
                        total_weight += weight;
                    }
                }
                if total_weight > 0.0 {
                    blurred /= total_weight;
                }
                let output_rgb = color.rgb + blurred.rgb * first.y
                    - color.rgb * blurred.rgb * first.y;
                let output_alpha = min(1.0, color.a + blurred.a * first.y);
                color = vec4<f32>(output_rgb * output_alpha, output_alpha);
            }
            case 28u: {
                let luma = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
                let glow_mask = smoothstep(first.w - 0.05, first.w + 0.05, luma);
                let texel = vec2<f32>(1.0) / target_size;
                let radius = i32(max(1.0, first.z * 10.0));
                var sum = vec3<f32>(0.0);
                var total = 0.0;
                for (var offset_x = -4; offset_x <= 4; offset_x += 1) {
                    for (var offset_y = -4; offset_y <= 4; offset_y += 1) {
                        if offset_x * offset_x + offset_y * offset_y <= radius * radius {
                            let offset = vec2<f32>(f32(offset_x), f32(offset_y))
                                * texel * first.z * 3.0;
                            let sampled = textureSample(layer_texture, layer_sampler, uv + offset);
                            let sampled_luma = dot(sampled.rgb, vec3<f32>(0.299, 0.587, 0.114));
                            let weight = smoothstep(
                                first.w - 0.05,
                                first.w + 0.05,
                                sampled_luma,
                            );
                            sum += sampled.rgb * weight;
                            total += weight;
                        }
                    }
                }
                var blurred = vec3<f32>(0.0);
                if total > 0.0 {
                    blurred = sum / total;
                }
                let glow_color = mix(blurred, third.rgb, second.x);
                color = vec4<f32>(
                    clamp(
                        color.rgb + glow_color * glow_mask * first.y,
                        vec3<f32>(0.0),
                        vec3<f32>(1.0),
                    ),
                    color.a,
                );
            }
            case 29u: {
                let texel = vec2<f32>(1.0) / target_size;
                let shift = first.yz * texel;
                if second.x < 0.001 {
                    let shifted = textureSample(layer_texture, layer_sampler, uv - shift);
                    let shadow_alpha = shifted.a * first.w;
                    let result = mix(third.rgb * shadow_alpha, color.rgb, color.a);
                    color = vec4<f32>(result, max(color.a, shadow_alpha));
                } else {
                    let radius = i32(max(1.0, second.x * 15.0));
                    var sum = vec3<f32>(0.0);
                    var total = 0.0;
                    for (var offset_x = -3; offset_x <= 3; offset_x += 1) {
                        for (var offset_y = -3; offset_y <= 3; offset_y += 1) {
                            if offset_x * offset_x + offset_y * offset_y <= radius * radius {
                                let offset = vec2<f32>(f32(offset_x), f32(offset_y))
                                    * texel * second.x * 2.0;
                                let sampled = textureSample(
                                    layer_texture,
                                    layer_sampler,
                                    uv - shift + offset,
                                );
                                sum += sampled.rgb * sampled.a;
                                total += sampled.a;
                            }
                        }
                    }
                    var shadow_base = vec3<f32>(0.0);
                    if total > 0.0 {
                        shadow_base = sum / total;
                    }
                    let diameter = 2 * radius + 1;
                    let shadow_alpha = total / f32(diameter * diameter) * first.w;
                    let shadow_color = third.rgb * shadow_alpha;
                    let result = mix(shadow_color, color.rgb, color.a);
                    color = vec4<f32>(result, max(color.a, shadow_alpha));
                }
            }
            case 30u: {
                let texel = vec2<f32>(1.0) / target_size;
                let radius = i32(clamp(first.z, 1.0, 16.0));
                let sigma_squared = f32(radius * radius) * 2.0;
                var sum = vec4<f32>(0.0);
                var total = 0.0;
                if second.w < 0.5 {
                    let luma = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
                    let glow_amount = smoothstep(first.w, first.w + 0.1, luma);
                    for (var offset_x = -radius; offset_x <= radius; offset_x += 1) {
                        let weight = exp(-f32(offset_x * offset_x) / sigma_squared);
                        let offset = vec2<f32>(f32(offset_x) * texel.x, 0.0);
                        sum += textureSample(layer_texture, layer_sampler, uv + offset) * weight;
                        total += weight;
                    }
                    let blurred = sum / max(total, 0.0001);
                    color = vec4<f32>(blurred.rgb * glow_amount, color.a);
                } else {
                    for (var offset_y = -radius; offset_y <= radius; offset_y += 1) {
                        let weight = exp(-f32(offset_y * offset_y) / sigma_squared);
                        let offset = vec2<f32>(0.0, f32(offset_y) * texel.y);
                        sum += textureSample(layer_texture, layer_sampler, uv + offset) * weight;
                        total += weight;
                    }
                    let blurred = sum / max(total, 0.0001);
                    let original = textureSample(mask_texture, layer_sampler, uv);
                    let glow = blurred.rgb * third.rgb * first.y;
                    color = vec4<f32>(original.rgb + glow * original.a, original.a);
                }
            }
            case 31u: {
                let texel = vec2<f32>(1.0) / max(target_size, vec2<f32>(1.0));
                let offset_uv = uv - first.zw * texel;
                let steps = i32(clamp(first.y * 0.5, 1.0, 10.0));
                let step_size = first.y / f32(max(1, steps));
                let sigma_squared = f32(steps * steps) * 0.5 + 0.1;
                var sum = vec4<f32>(0.0);
                var total = 0.0;
                if second.w < 0.5 {
                    if offset_uv.x >= 0.0 && offset_uv.x <= 1.0
                        && offset_uv.y >= 0.0 && offset_uv.y <= 1.0 {
                        for (var offset_index = -steps; offset_index <= steps; offset_index += 1) {
                            let sample_uv = offset_uv
                                + vec2<f32>(f32(offset_index) * step_size * texel.x, 0.0);
                            if sample_uv.x >= 0.0 && sample_uv.x <= 1.0 {
                                let weight = exp(-f32(offset_index * offset_index) / sigma_squared);
                                sum += textureSample(layer_texture, layer_sampler, sample_uv) * weight;
                                total += weight;
                            }
                        }
                    }
                    if total > 0.0 {
                        sum /= total;
                    }
                    color = sum;
                } else {
                    for (var offset_index = -steps; offset_index <= steps; offset_index += 1) {
                        let sample_uv = uv
                            + vec2<f32>(0.0, f32(offset_index) * step_size * texel.y);
                        if sample_uv.y >= 0.0 && sample_uv.y <= 1.0 {
                            let weight = exp(-f32(offset_index * offset_index) / sigma_squared);
                            sum += textureSample(layer_texture, layer_sampler, sample_uv) * weight;
                            total += weight;
                        }
                    }
                    if total > 0.0 {
                        sum /= total;
                    }
                    let original = textureSample(mask_texture, layer_sampler, uv);
                    let shadow_alpha = sum.a * second.x * third.a;
                    let shadow_color = third.rgb * shadow_alpha;
                    let output_alpha = original.a + shadow_alpha * (1.0 - original.a);
                    var output_rgb = vec3<f32>(0.0);
                    if output_alpha > 0.0 {
                        output_rgb = (
                            original.rgb * original.a
                            + shadow_color * (1.0 - original.a)
                        ) / output_alpha;
                    }
                    color = vec4<f32>(output_rgb * output_alpha, output_alpha);
                }
            }
            case 32u: {
                let texel = vec2<f32>(1.0) / max(target_size, vec2<f32>(1.0));
                let red_uv = uv + first.yz * texel;
                let green_uv = uv + vec2<f32>(first.w, second.x) * texel;
                let blue_uv = uv + second.yz * texel;
                let red = textureSample(layer_texture, layer_sampler, red_uv);
                let green = textureSample(layer_texture, layer_sampler, green_uv);
                let blue = textureSample(layer_texture, layer_sampler, blue_uv);
                color = vec4<f32>(
                    red.r,
                    green.g,
                    blue.b,
                    (red.a + green.a + blue.a) / 3.0,
                );
            }
            case 33u: {
                let tile_size = vec2<f32>(1.0) / max(first.yz, vec2<f32>(1.0));
                let tile_index = floor(uv / tile_size);
                var local_uv = (uv - tile_index * tile_size) / tile_size;
                if second.y > 0.5 {
                    let alternating = tile_index - floor(tile_index * 0.5) * 2.0;
                    if alternating.x >= 1.0 {
                        local_uv.x = 1.0 - local_uv.x;
                    }
                    if alternating.y >= 1.0 {
                        local_uv.y = 1.0 - local_uv.y;
                    }
                }
                color = textureSample(layer_texture, layer_sampler, local_uv);
            }
            case 34u: {
                let direction = i32(first.w);
                let center = 0.5 + second.x * 0.5;
                var mirrored_uv = uv;
                var distance = 0.0;
                if direction == 0 || direction == 2 {
                    mirrored_uv.x = 2.0 * center - uv.x;
                    distance = abs(uv.x - center);
                } else {
                    mirrored_uv.y = 2.0 * center - uv.y;
                    distance = abs(uv.y - center);
                }
                let mirrored = textureSample(layer_texture, layer_sampler, mirrored_uv);
                let mirror_alpha = max(0.0, first.y * (1.0 - first.z * distance * 2.0));
                let in_mirror_zone = (direction == 0 && uv.x > center)
                    || (direction == 1 && uv.y > center)
                    || (direction == 2 && uv.x < center)
                    || (direction != 0 && direction != 1 && direction != 2 && uv.y < center);
                if in_mirror_zone {
                    let mirrored_alpha = mirrored.a * mirror_alpha;
                    color = vec4<f32>(
                        mix(color.rgb, mirrored.rgb, mirrored_alpha),
                        max(color.a, mirrored_alpha),
                    );
                }
            }
            case 35u: {
                if first.y > 1.0 {
                    let pixel = uv * target_size;
                    let mosaic_pixel = floor(pixel / first.y) * first.y + vec2<f32>(first.y * 0.5);
                    color = textureSample(
                        layer_texture,
                        layer_sampler,
                        mosaic_pixel / target_size,
                    );
                }
            }
            case 36u: {
                let center = first.yz;
                let difference = uv - center;
                let distance = length(difference);
                let angle = atan2(difference.y, difference.x) + radians(second.x);
                let polar_uv = vec2<f32>(cos(angle), sin(angle)) * distance * first.w;
                color = textureSample(
                    layer_texture,
                    layer_sampler,
                    clamp(polar_uv + center, vec2<f32>(0.0), vec2<f32>(1.0)),
                );
            }
            case 37u: {
                let center = second.xy;
                let difference = uv - center;
                let distance = length(difference);
                let angle = atan2(difference.y, difference.x);
                let ripple = sin(
                    distance * first.z * 3.14159 * 2.0 - second.z * first.w * 0.1,
                ) * first.y;
                let distortion = vec2<f32>(cos(angle), sin(angle)) * ripple * 0.01;
                color = textureSample(
                    layer_texture,
                    layer_sampler,
                    clamp(uv + distortion, vec2<f32>(0.0), vec2<f32>(1.0)),
                );
            }
            case 38u: {
                let time = floor(first.w);
                let offset = vec2<f32>(
                    (effect_sine_hash(vec2<f32>(time, 0.0)) - 0.5) * 2.0
                        * first.y / target_size.x,
                    (effect_sine_hash(vec2<f32>(0.0, time)) - 0.5) * 2.0
                        * first.z / target_size.y,
                );
                color = textureSample(layer_texture, layer_sampler, uv + offset);
            }
            case 39u: {
                let scaled_uv = uv * vec2<f32>(first.w, second.x);
                var map_value = 0.0;
                if first.z < 0.5 {
                    map_value = effect_sine_hash(scaled_uv);
                } else if first.z < 1.5 {
                    map_value = sin(length(scaled_uv - vec2<f32>(0.5)) * 20.0) * 0.5 + 0.5;
                } else if first.z < 2.5 {
                    let checker = floor(scaled_uv * 10.0);
                    let total = checker.x + checker.y;
                    map_value = total - floor(total * 0.5) * 2.0;
                } else {
                    map_value = scaled_uv.x;
                }
                let displacement = vec2<f32>(map_value - 0.5) * first.y * 0.1;
                color = textureSample(
                    layer_texture,
                    layer_sampler,
                    clamp(uv + displacement, vec2<f32>(0.0), vec2<f32>(1.0)),
                );
            }
            case 40u: {
                let time = second.y * second.x;
                let shift = first.z * 0.01;
                let red = textureSample(
                    layer_texture,
                    layer_sampler,
                    uv + vec2<f32>(shift, 0.0),
                ).r;
                let blue = textureSample(
                    layer_texture,
                    layer_sampler,
                    uv - vec2<f32>(shift, 0.0),
                ).b;
                let scanline_phase = uv.y * target_size.y * 3.14159 + time * 10.0;
                let scanline = mix(
                    1.0,
                    sin(scanline_phase) * 0.5 + 0.5,
                    first.y,
                );
                let noise = effect_glitch_hash(
                    floor(uv * target_size) + vec2<f32>(time * 100.0),
                );
                color = vec4<f32>(
                    mix(vec3<f32>(red, color.g, blue) * scanline, vec3<f32>(noise), first.w),
                    color.a,
                );
            }
            case 41u: {
                let size = vec2<i32>(textureDimensions(layer_texture));
                let gid = clamp(
                    vec2<i32>(floor(uv * target_size)),
                    vec2<i32>(0),
                    size - vec2<i32>(1),
                );
                let block_size = clamp(i32(first.y), 2, 64);
                let direction = i32(second.y);
                let coordinate = select(gid.y, gid.x, direction == 0);
                let maximum_coordinate = select(size.y, size.x, direction == 0);
                let original = textureLoad(layer_texture, gid, 0);
                let original_luma = effect_pixel_luma(original.rgb);
                if original_luma >= first.z && original_luma <= first.w {
                    let block_start = coordinate / block_size * block_size;
                    let pixel_offset = coordinate - block_start;
                    var sorted_pixels: array<vec4<f32>, 64>;
                    var sorted_luma: array<f32, 64>;
                    var count = 0;
                    var valid_offset = 0;

                    for (var offset = 0; offset < 64; offset += 1) {
                        if offset >= block_size {
                            break;
                        }
                        let sample_coordinate = block_start + offset;
                        if sample_coordinate >= maximum_coordinate {
                            break;
                        }
                        let sample_pixel = select(
                            vec2<i32>(gid.x, sample_coordinate),
                            vec2<i32>(sample_coordinate, gid.y),
                            direction == 0,
                        );
                        let sampled = textureLoad(layer_texture, sample_pixel, 0);
                        let sampled_luma = effect_pixel_luma(sampled.rgb);
                        if sampled_luma < first.z || sampled_luma > first.w {
                            continue;
                        }
                        if offset < pixel_offset {
                            valid_offset += 1;
                        }
                        sorted_pixels[u32(count)] = sampled;
                        sorted_luma[u32(count)] = sampled_luma;
                        count += 1;
                    }

                    if count > 1 {
                        var padded_size = 1;
                        loop {
                            if padded_size >= count {
                                break;
                            }
                            padded_size *= 2;
                        }
                        for (var index = count; index < 64; index += 1) {
                            if index >= padded_size {
                                break;
                            }
                            sorted_luma[u32(index)] = 1e30;
                            sorted_pixels[u32(index)] = vec4<f32>(0.0);
                        }

                        var sequence_size = 2;
                        loop {
                            if sequence_size > padded_size {
                                break;
                            }
                            var comparison_distance = sequence_size / 2;
                            loop {
                                if comparison_distance <= 0 {
                                    break;
                                }
                                for (var index = 0; index < 64; index += 1) {
                                    if index >= padded_size {
                                        break;
                                    }
                                    let paired_index = index ^ comparison_distance;
                                    if paired_index > index && paired_index < padded_size {
                                        var ascending = (index & sequence_size) == 0;
                                        if second.z > 0.5 {
                                            ascending = !ascending;
                                        }
                                        let should_swap = select(
                                            sorted_luma[u32(index)] < sorted_luma[u32(paired_index)],
                                            sorted_luma[u32(index)] > sorted_luma[u32(paired_index)],
                                            ascending,
                                        );
                                        if should_swap {
                                            let luma_value = sorted_luma[u32(index)];
                                            sorted_luma[u32(index)] = sorted_luma[u32(paired_index)];
                                            sorted_luma[u32(paired_index)] = luma_value;

                                            let pixel_value = sorted_pixels[u32(index)];
                                            sorted_pixels[u32(index)] = sorted_pixels[u32(paired_index)];
                                            sorted_pixels[u32(paired_index)] = pixel_value;
                                        }
                                    }
                                }
                                comparison_distance /= 2;
                            }
                            sequence_size *= 2;
                        }
                        color = mix(original, sorted_pixels[u32(valid_offset)], second.x);
                    } else {
                        color = original;
                    }
                } else {
                    color = original;
                }
            }
            default: {}
        }
        color = clamp(color, vec4<f32>(0.0), vec4<f32>(1.0));
    }
    return color;
}

@vertex
fn effect_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0)
    );
    let uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0)
    );
    var output: VertexOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    output.uv = uvs[index];
    return output;
}

@fragment
fn effect_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let target_size = vec2<f32>(textureDimensions(layer_texture));
    return apply_visual_effects(
        textureSample(layer_texture, layer_sampler, input.uv),
        input.uv,
        target_size,
    );
}

fn mask_alpha(position: vec4<f32>) -> f32 {
    if layer.opacity_blend_visible.w < 0.5 {
        return 1.0;
    }
    let dimensions = vec2<f32>(textureDimensions(mask_texture));
    return textureSample(mask_texture, layer_sampler, position.xy / dimensions).a;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if layer.opacity_blend_visible.z < 0.5 {
        discard;
    }
    if input.uv.x < layer.crop_uv.x || input.uv.y < layer.crop_uv.y ||
       input.uv.x > layer.crop_uv.z || input.uv.y > layer.crop_uv.w {
        discard;
    }
    let target_size = vec2<f32>(textureDimensions(layer_texture));
    let color = apply_visual_effects(
        textureSample(layer_texture, layer_sampler, input.uv),
        input.uv,
        target_size,
    );
    let alpha = color.a * layer.opacity_blend_visible.x * mask_alpha(input.position);
    return vec4<f32>(color.rgb * alpha, alpha);
}

fn blend_overlay(base: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    return select(
        2.0 * base * blend,
        1.0 - 2.0 * (1.0 - base) * (1.0 - blend),
        base >= vec3<f32>(0.5)
    );
}

fn blend_soft_light_channel(base: f32, blend: f32) -> f32 {
    if blend <= 0.5 {
        return base - (1.0 - 2.0 * blend) * base * (1.0 - base);
    }
    var curve: f32;
    if base <= 0.25 {
        curve = ((16.0 * base - 12.0) * base + 4.0) * base;
    } else {
        curve = sqrt(base);
    }
    return (curve - base) * (2.0 * blend - 1.0) + base;
}

fn blend_soft_light(base: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        blend_soft_light_channel(base.r, blend.r),
        blend_soft_light_channel(base.g, blend.g),
        blend_soft_light_channel(base.b, blend.b)
    );
}

fn blend_hard_light(base: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    return select(
        2.0 * base * blend,
        1.0 - 2.0 * (1.0 - base) * (1.0 - blend),
        blend >= vec3<f32>(0.5)
    );
}

fn luminosity(color: vec3<f32>) -> f32 {
    return 0.3 * color.r + 0.59 * color.g + 0.11 * color.b;
}

fn set_luminosity(color: vec3<f32>, desired: f32) -> vec3<f32> {
    var result = color + vec3<f32>(desired - luminosity(color));
    let minimum = min(min(result.r, result.g), result.b);
    let maximum = max(max(result.r, result.g), result.b);
    if minimum < 0.0 {
        result = desired + ((result - desired) * desired) / (desired - minimum);
    }
    if maximum > 1.0 {
        result = desired + ((result - desired) * (1.0 - desired)) / (maximum - desired);
    }
    return result;
}

fn saturation(color: vec3<f32>) -> f32 {
    return max(max(color.r, color.g), color.b) - min(min(color.r, color.g), color.b);
}

fn saturation_values(minimum: f32, middle: f32, maximum: f32, desired: f32) -> vec3<f32> {
    if maximum > minimum {
        return vec3<f32>(0.0, (middle - minimum) * desired / (maximum - minimum), desired);
    }
    return vec3<f32>(0.0);
}

fn set_saturation(color: vec3<f32>, desired: f32) -> vec3<f32> {
    var values: vec3<f32>;
    if color.r <= color.g {
        if color.g <= color.b {
            values = saturation_values(color.r, color.g, color.b, desired);
            return values;
        }
        if color.r <= color.b {
            values = saturation_values(color.r, color.b, color.g, desired);
            return vec3<f32>(values.x, values.z, values.y);
        }
        values = saturation_values(color.b, color.r, color.g, desired);
        return vec3<f32>(values.y, values.z, values.x);
    }
    if color.r <= color.b {
        values = saturation_values(color.g, color.r, color.b, desired);
        return vec3<f32>(values.y, values.x, values.z);
    }
    if color.g <= color.b {
        values = saturation_values(color.g, color.b, color.r, desired);
        return vec3<f32>(values.z, values.x, values.y);
    }
    values = saturation_values(color.b, color.g, color.r, desired);
    return vec3<f32>(values.z, values.y, values.x);
}

fn complex_blend(mode: u32, base: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    if mode == 3u {
        return blend_overlay(base, blend);
    }
    if mode == 5u {
        return max(base - blend, vec3<f32>(0.0));
    }
    if mode == 6u {
        return max(base, blend);
    }
    if mode == 7u {
        return min(base, blend);
    }
    if mode == 8u {
        return 1.0 - base;
    }
    if mode == 9u {
        return blend_soft_light(base, blend);
    }
    if mode == 10u {
        return blend_hard_light(base, blend);
    }
    if mode == 11u {
        return abs(base - blend);
    }
    if mode == 12u {
        return set_luminosity(set_saturation(blend, saturation(base)), luminosity(base));
    }
    if mode == 13u {
        return set_luminosity(set_saturation(base, saturation(blend)), luminosity(base));
    }
    if mode == 14u {
        return set_luminosity(blend, luminosity(base));
    }
    if mode == 15u {
        return set_luminosity(base, luminosity(blend));
    }
    return blend;
}

@fragment
fn complex_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if layer.opacity_blend_visible.z < 0.5 {
        discard;
    }
    if input.uv.x < layer.crop_uv.x || input.uv.y < layer.crop_uv.y ||
       input.uv.x > layer.crop_uv.z || input.uv.y > layer.crop_uv.w {
        discard;
    }
    let target_size = vec2<f32>(textureDimensions(layer_texture));
    let source = apply_visual_effects(
        textureSample(layer_texture, layer_sampler, input.uv),
        input.uv,
        target_size,
    );
    let source_alpha = source.a * layer.opacity_blend_visible.x * mask_alpha(input.position);
    if source_alpha <= 0.0 {
        discard;
    }
    let background = textureLoad(background_texture, vec2<i32>(input.position.xy), 0);
    var base = vec3<f32>(0.0);
    if background.a > 0.0 {
        base = background.rgb / background.a;
    }
    let blended = complex_blend(u32(layer.opacity_blend_visible.y), base, source.rgb);
    let output_alpha = source_alpha + background.a * (1.0 - source_alpha);
    let output_rgb = background.rgb * (1.0 - source_alpha) + source_alpha *
        ((1.0 - background.a) * source.rgb + background.a * blended);
    return vec4<f32>(output_rgb, output_alpha);
}
"#;

/// AviQtl layer blend modes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Screen,
    Multiply,
    Overlay,
    Add,
    Subtract,
    Lighten,
    Darken,
    Invert,
    SoftLight,
    HardLight,
    Difference,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Maps AviQtl's persisted transform labels to a supported blend mode.
    pub fn from_aviqtl_name(name: &str) -> Self {
        match name {
            "スクリーン" | "screen" => Self::Screen,
            "乗算" | "multiply" => Self::Multiply,
            "オーバーレイ" | "overlay" => Self::Overlay,
            "加算" | "add" => Self::Add,
            "減算" | "subtract" => Self::Subtract,
            "比較（明）" | "比較明" | "lighten" => Self::Lighten,
            "比較（暗）" | "比較暗" | "darken" => Self::Darken,
            "色反転" | "invert" => Self::Invert,
            "ソフトライト" | "soft light" | "soft_light" | "softlight" => Self::SoftLight,
            "ハードライト" | "hard light" | "hard_light" | "hardlight" => Self::HardLight,
            "差の絶対値" | "difference" => Self::Difference,
            "色相" | "hue" => Self::Hue,
            "彩度" | "saturation" => Self::Saturation,
            "カラー" | "color" => Self::Color,
            "輝度" | "luminosity" => Self::Luminosity,
            _ => Self::Normal,
        }
    }

    fn shader_id(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Screen => 1,
            Self::Multiply => 2,
            Self::Overlay => 3,
            Self::Add => 4,
            Self::Subtract => 5,
            Self::Lighten => 6,
            Self::Darken => 7,
            Self::Invert => 8,
            Self::SoftLight => 9,
            Self::HardLight => 10,
            Self::Difference => 11,
            Self::Hue => 12,
            Self::Saturation => 13,
            Self::Color => 14,
            Self::Luminosity => 15,
        }
    }
}

/// Source-pixel crop applied before the layer transform.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LayerCrop {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub recenter: bool,
}

/// Presentation-space transform for one decoded media layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerTransform {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub scale_percent: f32,
    pub aspect: f32,
    pub rotation_x_degrees: f32,
    pub rotation_y_degrees: f32,
    pub rotation_z_degrees: f32,
    pub pivot_x: f32,
    pub pivot_y: f32,
    pub pivot_z: f32,
    pub opacity: f32,
    pub backface_visible: bool,
}

impl Default for LayerTransform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            scale_percent: 100.0,
            aspect: 0.0,
            rotation_x_degrees: 0.0,
            rotation_y_degrees: 0.0,
            rotation_z_degrees: 0.0,
            pivot_x: 0.0,
            pivot_y: 0.0,
            pivot_z: 0.0,
            opacity: 1.0,
            backface_visible: true,
        }
    }
}

/// CPU or GPU source pixels for one composition layer.
#[derive(Clone, Copy)]
pub enum CompositionSource<'a> {
    Frame(&'a VideoFrame),
    Texture {
        texture: &'a wgpu::Texture,
        size: (u32, u32),
        stamp: u64,
    },
}

impl CompositionSource<'_> {
    fn size(self) -> (u32, u32) {
        match self {
            Self::Frame(frame) => (frame.width, frame.height),
            Self::Texture { size, .. } => size,
        }
    }

    fn stamp(self) -> FrameStamp {
        match self {
            Self::Frame(frame) => FrameStamp::from(frame),
            Self::Texture { size, stamp, .. } => FrameStamp {
                width: size.0,
                height: size.1,
                timestamp_bits: stamp,
            },
        }
    }
}

/// A decoded or offscreen source and its timeline layer number.
pub struct CompositionLayer<'a> {
    /// Stable for one clip and media source while the preview is alive.
    pub cache_key: u64,
    pub source: CompositionSource<'a>,
    pub timeline_layer: i32,
    pub transform: LayerTransform,
    pub blend_mode: BlendMode,
    pub crop: LayerCrop,
    pub mask: Option<CompositionMask<'a>>,
    pub effects: &'a [VisualEffect],
}

#[derive(Clone, Copy)]
pub struct CompositionMask<'a> {
    pub cache_key: u64,
    pub texture: &'a wgpu::Texture,
    pub size: (u32, u32),
}

/// Reusable render pipeline for alpha-compositing decoded RGBA frames.
pub struct Compositor {
    effect_pipeline: wgpu::RenderPipeline,
    normal_pipeline: wgpu::RenderPipeline,
    screen_pipeline: wgpu::RenderPipeline,
    multiply_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    complex_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    background_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    target_format: wgpu::TextureFormat,
    targets: Option<CompositionTargets>,
    layers: HashMap<u64, GpuLayer>,
}

#[derive(Clone, Copy)]
struct ProjectionContext<'a> {
    target_size: (u32, u32),
    camera: Option<&'a CameraRenderPlan>,
}

#[derive(Clone, Copy)]
struct GpuContext<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    layer_layout: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    target_format: wgpu::TextureFormat,
}

fn ordered_composition_layers<'layers, 'source>(
    layers: &'layers [CompositionLayer<'source>],
    projection: ProjectionContext<'_>,
) -> Vec<&'layers CompositionLayer<'source>> {
    let camera = projection
        .camera
        .copied()
        .unwrap_or_else(|| default_camera(projection.target_size.1.max(1) as f32));
    let mut ordered = layers.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        let left_depth = qt_transparent_camera_depth(left.transform, camera);
        let right_depth = qt_transparent_camera_depth(right.transform, camera);
        right_depth
            .partial_cmp(&left_depth)
            .unwrap_or(Ordering::Equal)
            .then_with(|| right.timeline_layer.cmp(&left.timeline_layer))
    });
    ordered
}

/// Qt Quick3D sorts transparent renderables from far to near with
/// `dot(worldCenter - cameraPosition, cameraDirection)`. The compositor uses
/// the same object-center key; equal-depth layers retain AviQtl's timeline
/// stacking order as a deterministic tie-breaker.
fn qt_transparent_camera_depth(transform: LayerTransform, camera: CameraRenderPlan) -> f32 {
    let (scale_x, scale_y, scale_z) = layer_transform_scales(transform);
    let center = transform_point([0.0, 0.0, 0.0], transform, scale_x, scale_y, scale_z);
    let position = [camera.position_x, camera.position_y, camera.position_z];
    let target = [camera.target_x, camera.target_y, camera.target_z];
    let direction = normalize(subtract(target, position)).unwrap_or([0.0, 0.0, -1.0]);
    dot(subtract(center, position), direction)
}

impl Compositor {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aviqtl-compositor-layer-layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let background_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aviqtl-compositor-background-layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        let fixed_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aviqtl-compositor-pipeline-layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });
        let complex_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aviqtl-compositor-complex-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&bind_group_layout),
                    Some(&background_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aviqtl-compositor-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let pipeline = |label: &'static str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&fixed_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let effect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aviqtl-effect-pass-pipeline"),
            layout: Some(&fixed_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("effect_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("effect_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let normal_pipeline = pipeline(
            "aviqtl-compositor-normal-pipeline",
            wgpu::BlendState {
                color: alpha,
                alpha,
            },
        );
        let screen_pipeline = pipeline(
            "aviqtl-compositor-screen-pipeline",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha,
            },
        );
        let multiply_pipeline = pipeline(
            "aviqtl-compositor-multiply-pipeline",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha,
            },
        );
        let add_pipeline = pipeline(
            "aviqtl-compositor-add-pipeline",
            wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha,
            },
        );
        let complex_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aviqtl-compositor-complex-pipeline"),
            layout: Some(&complex_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("complex_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aviqtl-compositor-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            effect_pipeline,
            normal_pipeline,
            screen_pipeline,
            multiply_pipeline,
            add_pipeline,
            complex_pipeline,
            bind_group_layout,
            background_bind_group_layout,
            sampler,
            target_format,
            targets: None,
            layers: HashMap::new(),
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        target_size: (u32, u32),
        layers: &[CompositionLayer<'_>],
        camera: Option<&CameraRenderPlan>,
    ) {
        let projection = ProjectionContext {
            target_size,
            camera,
        };
        self.render_with_clear(
            device,
            queue,
            target,
            layers,
            wgpu::Color {
                r: 0.015,
                g: 0.017,
                b: 0.022,
                a: 1.0,
            },
            projection,
        );
    }

    /// Renders an offscreen scene while preserving transparent empty pixels.
    pub fn render_transparent(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        target_size: (u32, u32),
        layers: &[CompositionLayer<'_>],
        camera: Option<&CameraRenderPlan>,
    ) {
        let projection = ProjectionContext {
            target_size,
            camera,
        };
        self.render_with_clear(
            device,
            queue,
            target,
            layers,
            wgpu::Color::TRANSPARENT,
            projection,
        );
    }

    pub fn render_opaque_black(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        target_size: (u32, u32),
        layers: &[CompositionLayer<'_>],
        camera: Option<&CameraRenderPlan>,
    ) {
        let projection = ProjectionContext {
            target_size,
            camera,
        };
        self.render_with_clear(
            device,
            queue,
            target,
            layers,
            wgpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            projection,
        );
    }

    fn render_with_clear(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        layers: &[CompositionLayer<'_>],
        clear_color: wgpu::Color,
        projection: ProjectionContext<'_>,
    ) {
        let ordered = ordered_composition_layers(layers, projection);
        let active_keys = ordered
            .iter()
            .map(|layer| layer.cache_key)
            .collect::<HashSet<_>>();
        self.layers.retain(|key, _| active_keys.contains(key));
        let gpu = GpuContext {
            device,
            queue,
            layer_layout: &self.bind_group_layout,
            sampler: &self.sampler,
            target_format: self.target_format,
        };
        for layer in &ordered {
            if let Some(gpu_layer) = self.layers.get_mut(&layer.cache_key) {
                gpu_layer.update(gpu, layer, projection);
            } else {
                self.layers
                    .insert(layer.cache_key, GpuLayer::new(gpu, layer, projection));
            }
        }
        self.ensure_targets(device, projection.target_size);
        let targets = self
            .targets
            .as_ref()
            .expect("composition targets were created above");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("aviqtl-compositor-encoder"),
        });
        for layer in &ordered {
            self.layers
                .get(&layer.cache_key)
                .expect("active layer was prepared before effect rendering")
                .encode_effect_passes(&mut encoder, &self.effect_pipeline);
        }
        {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aviqtl-compositor-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.views[0],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let mut current_target = 0;
        for layer in ordered {
            let gpu_layer = self
                .layers
                .get(&layer.cache_key)
                .expect("active layer was prepared before rendering");
            if let Some(pipeline) = self.fixed_pipeline(gpu_layer.blend_mode) {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("aviqtl-compositor-fixed-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &targets.views[current_target],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &gpu_layer.bind_group, &[]);
                pass.draw(0..6, 0..1);
                continue;
            }
            let next_target = 1 - current_target;
            copy_texture(
                &mut encoder,
                &targets.textures[current_target],
                &targets.textures[next_target],
                projection.target_size,
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aviqtl-compositor-complex-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.views[next_target],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.complex_pipeline);
            pass.set_bind_group(0, &gpu_layer.bind_group, &[]);
            pass.set_bind_group(1, &targets.background_bind_groups[current_target], &[]);
            pass.draw(0..6, 0..1);
            drop(pass);
            current_target = next_target;
        }
        copy_texture(
            &mut encoder,
            &targets.textures[current_target],
            target,
            projection.target_size,
        );
        queue.submit([encoder.finish()]);
    }

    fn fixed_pipeline(&self, blend_mode: BlendMode) -> Option<&wgpu::RenderPipeline> {
        match blend_mode {
            BlendMode::Normal => Some(&self.normal_pipeline),
            BlendMode::Screen => Some(&self.screen_pipeline),
            BlendMode::Multiply => Some(&self.multiply_pipeline),
            BlendMode::Add => Some(&self.add_pipeline),
            _ => None,
        }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self
            .targets
            .as_ref()
            .is_some_and(|targets| targets.size == size)
        {
            return;
        }
        self.targets = Some(CompositionTargets::new(
            device,
            &self.background_bind_group_layout,
            self.target_format,
            size,
        ));
    }
}

struct CompositionTargets {
    size: (u32, u32),
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    background_bind_groups: [wgpu::BindGroup; 2],
}

impl CompositionTargets {
    fn new(
        device: &wgpu::Device,
        background_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> Self {
        let textures = [
            create_composition_texture(device, format, size),
            create_composition_texture(device, format, size),
        ];
        let views = [
            textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
            textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        let background_bind_groups = [
            create_background_bind_group(device, background_layout, &views[0]),
            create_background_bind_group(device, background_layout, &views[1]),
        ];
        Self {
            size,
            textures,
            views,
            background_bind_groups,
        }
    }
}

fn create_composition_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aviqtl-compositor-target"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn create_background_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("aviqtl-compositor-background-bind-group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}

fn copy_texture(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    destination: &wgpu::Texture,
    size: (u32, u32),
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: destination,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EffectPassPlan {
    effect_index: usize,
    pass_index: usize,
    source: Option<usize>,
    original: Option<usize>,
    destination: usize,
}

fn effect_pass_plan(effects: &[VisualEffect]) -> Vec<EffectPassPlan> {
    let mut plan = Vec::with_capacity(effects.iter().map(VisualEffect::pass_count).sum());
    let mut current_source = None;
    for (effect_index, effect) in effects.iter().enumerate() {
        let original = current_source;
        let available_destinations = [0, 1, 2]
            .into_iter()
            .filter(|candidate| Some(*candidate) != original)
            .collect::<Vec<_>>();
        for pass_index in 0..effect.pass_count() {
            let destination = available_destinations[pass_index % available_destinations.len()];
            plan.push(EffectPassPlan {
                effect_index,
                pass_index,
                source: current_source,
                original,
                destination,
            });
            current_source = Some(destination);
        }
    }
    plan
}

struct GpuEffectPasses {
    _textures: [wgpu::Texture; 3],
    views: [wgpu::TextureView; 3],
    uniforms: Vec<wgpu::Buffer>,
    bind_groups: Vec<wgpu::BindGroup>,
    destinations: Vec<usize>,
    effect_pass_counts: Vec<usize>,
    output_index: usize,
}

impl GpuEffectPasses {
    fn new(
        gpu: GpuContext<'_>,
        size: (u32, u32),
        source_view: &wgpu::TextureView,
        layer_uniform: &wgpu::Buffer,
        effects: &[VisualEffect],
    ) -> Self {
        debug_assert!(effects.iter().any(VisualEffect::requires_sequential_passes));
        let textures = [
            create_composition_texture(gpu.device, gpu.target_format, size),
            create_composition_texture(gpu.device, gpu.target_format, size),
            create_composition_texture(gpu.device, gpu.target_format, size),
        ];
        let views = [
            textures[0].create_view(&wgpu::TextureViewDescriptor::default()),
            textures[1].create_view(&wgpu::TextureViewDescriptor::default()),
            textures[2].create_view(&wgpu::TextureViewDescriptor::default()),
        ];
        let plan = effect_pass_plan(effects);
        let mut uniforms = Vec::with_capacity(plan.len());
        let mut bind_groups = Vec::with_capacity(plan.len());
        let mut destinations = Vec::with_capacity(plan.len());
        let effect_pass_counts = effects
            .iter()
            .map(VisualEffect::pass_count)
            .collect::<Vec<_>>();
        for pass_plan in &plan {
            let input_view = pass_plan.source.map_or(source_view, |index| &views[index]);
            let original_view = pass_plan
                .original
                .map_or(source_view, |index| &views[index]);
            let values = encode_effect_pass(&effects[pass_plan.effect_index], pass_plan.pass_index);
            uniforms.push(
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("aviqtl-single-effect-uniform"),
                        contents: &f32_slice_bytes(&values),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    }),
            );
            bind_groups.push(
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("aviqtl-single-effect-bind-group"),
                    layout: gpu.layer_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(input_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(gpu.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: layer_uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(original_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: uniforms
                                .last()
                                .expect("the effect uniform was inserted above")
                                .as_entire_binding(),
                        },
                    ],
                }),
            );
            destinations.push(pass_plan.destination);
        }
        Self {
            _textures: textures,
            views,
            uniforms,
            bind_groups,
            destinations,
            effect_pass_counts,
            output_index: plan
                .last()
                .expect("a sequential chain contains at least one pass")
                .destination,
        }
    }

    fn update(&self, queue: &wgpu::Queue, effects: &[VisualEffect]) {
        debug_assert!(self.matches_effects(effects));
        let mut uniform_index = 0;
        for effect in effects {
            for pass_index in 0..effect.pass_count() {
                let values = encode_effect_pass(effect, pass_index);
                queue.write_buffer(&self.uniforms[uniform_index], 0, &f32_slice_bytes(&values));
                uniform_index += 1;
            }
        }
    }

    fn matches_effects(&self, effects: &[VisualEffect]) -> bool {
        self.effect_pass_counts
            .iter()
            .copied()
            .eq(effects.iter().map(VisualEffect::pass_count))
    }

    fn output_view(&self) -> &wgpu::TextureView {
        &self.views[self.output_index]
    }

    fn encode(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline) {
        for (bind_group, destination) in self.bind_groups.iter().zip(&self.destinations) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aviqtl-single-effect-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.views[*destination],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

struct GpuLayer {
    texture: Option<wgpu::Texture>,
    uniform: wgpu::Buffer,
    effect_uniform: wgpu::Buffer,
    effect_uniform_len: usize,
    effect_passes: Option<GpuEffectPasses>,
    bind_group: wgpu::BindGroup,
    blend_mode: BlendMode,
    frame_stamp: FrameStamp,
    mask_stamp: Option<(u64, (u32, u32))>,
}

impl GpuLayer {
    fn new(
        gpu: GpuContext<'_>,
        layer: &CompositionLayer<'_>,
        projection: ProjectionContext<'_>,
    ) -> Self {
        let (texture, view) = match layer.source {
            CompositionSource::Frame(frame) => {
                let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("aviqtl-compositor-source"),
                    size: wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                write_frame(gpu.queue, &texture, frame);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(texture), view)
            }
            CompositionSource::Texture { texture, .. } => (
                None,
                texture.create_view(&wgpu::TextureViewDescriptor::default()),
            ),
        };
        let uniform_values = layer_uniform(
            projection.target_size,
            layer.source.size(),
            layer.transform,
            layer.crop,
            layer.blend_mode,
            projection.camera,
            layer.mask.is_some(),
        );
        let uniform = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aviqtl-compositor-layer-uniform"),
                contents: &f32_bytes(uniform_values),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let effect_passes = layer
            .effects
            .iter()
            .any(VisualEffect::requires_sequential_passes)
            .then(|| {
                GpuEffectPasses::new(gpu, layer.source.size(), &view, &uniform, layer.effects)
            });
        let effect_values = if effect_passes.is_some() {
            encode_effect_chain(&[])
        } else {
            encode_effect_chain(layer.effects)
        };
        let effect_uniform = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aviqtl-compositor-effect-uniform"),
                contents: &f32_slice_bytes(&effect_values),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            });
        let mask_view = layer.mask.map(|mask| {
            mask.texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        });
        let composition_view = effect_passes
            .as_ref()
            .map_or(&view, GpuEffectPasses::output_view);
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aviqtl-compositor-layer-bind-group"),
            layout: gpu.layer_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(composition_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(gpu.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        mask_view.as_ref().unwrap_or(&view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: effect_uniform.as_entire_binding(),
                },
            ],
        });
        Self {
            texture,
            uniform,
            effect_uniform,
            effect_uniform_len: effect_values.len(),
            effect_passes,
            bind_group,
            blend_mode: layer.blend_mode,
            frame_stamp: layer.source.stamp(),
            mask_stamp: layer.mask.map(|mask| (mask.cache_key, mask.size)),
        }
    }

    fn update(
        &mut self,
        gpu: GpuContext<'_>,
        layer: &CompositionLayer<'_>,
        projection: ProjectionContext<'_>,
    ) {
        let frame_stamp = layer.source.stamp();
        let mask_stamp = layer.mask.map(|mask| (mask.cache_key, mask.size));
        let requires_sequential_passes = layer
            .effects
            .iter()
            .any(VisualEffect::requires_sequential_passes);
        let effect_values = if requires_sequential_passes {
            encode_effect_chain(&[])
        } else {
            encode_effect_chain(layer.effects)
        };
        if self.frame_stamp.size() != frame_stamp.size()
            || self.mask_stamp != mask_stamp
            || self.effect_uniform_len != effect_values.len()
            || self.effect_passes.is_some() != requires_sequential_passes
            || self
                .effect_passes
                .as_ref()
                .is_some_and(|passes| !passes.matches_effects(layer.effects))
        {
            *self = Self::new(gpu, layer, projection);
            return;
        }
        match (&self.texture, layer.source) {
            (Some(texture), CompositionSource::Frame(frame)) => {
                if self.frame_stamp != frame_stamp {
                    write_frame(gpu.queue, texture, frame);
                }
            }
            (None, CompositionSource::Texture { .. }) => {}
            _ => {
                *self = Self::new(gpu, layer, projection);
                return;
            }
        }
        self.frame_stamp = frame_stamp;
        self.mask_stamp = mask_stamp;
        if let Some(effect_passes) = &self.effect_passes {
            effect_passes.update(gpu.queue, layer.effects);
        }
        gpu.queue
            .write_buffer(&self.effect_uniform, 0, &f32_slice_bytes(&effect_values));
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &f32_bytes(layer_uniform(
                projection.target_size,
                layer.source.size(),
                layer.transform,
                layer.crop,
                layer.blend_mode,
                projection.camera,
                layer.mask.is_some(),
            )),
        );
        self.blend_mode = layer.blend_mode;
    }

    fn encode_effect_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
    ) {
        if let Some(effect_passes) = &self.effect_passes {
            effect_passes.encode(encoder, pipeline);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameStamp {
    width: u32,
    height: u32,
    timestamp_bits: u64,
}

impl FrameStamp {
    fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl From<&VideoFrame> for FrameStamp {
    fn from(frame: &VideoFrame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            timestamp_bits: frame.timestamp_seconds.to_bits(),
        }
    }
}

fn write_frame(queue: &wgpu::Queue, texture: &wgpu::Texture, frame: &VideoFrame) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &frame.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(frame.width * 4),
            rows_per_image: Some(frame.height),
        },
        wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        },
    );
}

fn layer_uniform(
    target_size: (u32, u32),
    source_size: (u32, u32),
    transform: LayerTransform,
    crop: LayerCrop,
    blend_mode: BlendMode,
    camera: Option<&CameraRenderPlan>,
    has_mask: bool,
) -> [f32; 24] {
    let target_width = target_size.0.max(1) as f32;
    let target_height = target_size.1.max(1) as f32;
    let source_width = source_size.0.max(1) as f32;
    let source_height = source_size.1.max(1) as f32;
    let (scale_x, scale_y, scale) = layer_transform_scales(transform);
    let (offset_x, offset_y) = if crop.recenter {
        (
            (crop.right.max(0.0) - crop.left.max(0.0)) * 0.5,
            -(crop.bottom.max(0.0) - crop.top.max(0.0)) * 0.5,
        )
    } else {
        (0.0, 0.0)
    };
    let local = [
        [
            -source_width / 2.0 + offset_x,
            -source_height / 2.0 + offset_y,
            0.0,
        ],
        [
            source_width / 2.0 + offset_x,
            -source_height / 2.0 + offset_y,
            0.0,
        ],
        [
            -source_width / 2.0 + offset_x,
            source_height / 2.0 + offset_y,
            0.0,
        ],
        [
            source_width / 2.0 + offset_x,
            source_height / 2.0 + offset_y,
            0.0,
        ],
    ];
    let camera = camera
        .copied()
        .unwrap_or_else(|| default_camera(target_height));
    let corners = local.map(|point| {
        let world = transform_point(point, transform, scale_x, scale_y, scale);
        project_point(world, camera, target_width / target_height)
    });
    let center = transform_point([0.0, 0.0, 0.0], transform, scale_x, scale_y, scale);
    let normal = rotate_transform_vector([0.0, 0.0, 1.0], transform);
    let camera_direction = [
        camera.position_x - center[0],
        camera.position_y - center[1],
        camera.position_z - center[2],
    ];
    let front_facing = dot(normal, camera_direction) >= 0.0;
    let visible = (transform.backface_visible || front_facing)
        && corners.iter().any(|corner| corner[3] > f32::EPSILON);
    [
        corners[0][0],
        corners[0][1],
        corners[0][2],
        corners[0][3],
        corners[1][0],
        corners[1][1],
        corners[1][2],
        corners[1][3],
        corners[2][0],
        corners[2][1],
        corners[2][2],
        corners[2][3],
        corners[3][0],
        corners[3][1],
        corners[3][2],
        corners[3][3],
        (crop.left.max(0.0) / source_width).clamp(0.0, 1.0),
        (crop.top.max(0.0) / source_height).clamp(0.0, 1.0),
        (1.0 - crop.right.max(0.0) / source_width).clamp(0.0, 1.0),
        (1.0 - crop.bottom.max(0.0) / source_height).clamp(0.0, 1.0),
        transform.opacity.clamp(0.0, 1.0),
        blend_mode.shader_id() as f32,
        if visible { 1.0 } else { 0.0 },
        if has_mask { 1.0 } else { 0.0 },
    ]
}

fn layer_transform_scales(transform: LayerTransform) -> (f32, f32, f32) {
    let scale = transform.scale_percent.max(0.0) / 100.0;
    let scale_x = scale
        * if transform.aspect >= 0.0 {
            1.0 + transform.aspect
        } else {
            1.0
        };
    let scale_y = scale
        * if transform.aspect < 0.0 {
            1.0 - transform.aspect
        } else {
            1.0
        };
    (scale_x, scale_y, scale)
}

fn default_camera(target_height: f32) -> CameraRenderPlan {
    let field_of_view_degrees = 30.0_f32;
    CameraRenderPlan {
        position_x: 0.0,
        position_y: 0.0,
        position_z: target_height / (2.0 * (field_of_view_degrees.to_radians() / 2.0).tan()),
        target_x: 0.0,
        target_y: 0.0,
        target_z: 0.0,
        roll_degrees: 0.0,
        field_of_view_degrees,
    }
}

fn transform_point(
    point: [f32; 3],
    transform: LayerTransform,
    scale_x: f32,
    scale_y: f32,
    scale_z: f32,
) -> [f32; 3] {
    let pivot = [transform.pivot_x, transform.pivot_y, transform.pivot_z];
    let relative = [
        (point[0] - pivot[0]) * scale_x,
        (point[1] - pivot[1]) * scale_y,
        (point[2] - pivot[2]) * scale_z,
    ];
    let rotated = rotate_transform_vector(relative, transform);
    [
        transform.x + pivot[0] + rotated[0],
        transform.y + pivot[1] + rotated[1],
        transform.z + pivot[2] + rotated[2],
    ]
}

fn rotate_transform_vector(mut vector: [f32; 3], transform: LayerTransform) -> [f32; 3] {
    vector = rotate_x(vector, transform.rotation_x_degrees.to_radians());
    vector = rotate_y(vector, -transform.rotation_y_degrees.to_radians());
    rotate_z(vector, -transform.rotation_z_degrees.to_radians())
}

fn project_point(point: [f32; 3], camera: CameraRenderPlan, aspect: f32) -> [f32; 4] {
    let position = [camera.position_x, camera.position_y, camera.position_z];
    let target = [camera.target_x, camera.target_y, camera.target_z];
    let backward = normalize(subtract(position, target)).unwrap_or([0.0, 0.0, 1.0]);
    let mut right = normalize(cross([0.0, 1.0, 0.0], backward)).unwrap_or([1.0, 0.0, 0.0]);
    let mut up = cross(backward, right);
    let roll = camera.roll_degrees.to_radians();
    let (sine, cosine) = roll.sin_cos();
    let rolled_right = [
        right[0] * cosine + up[0] * sine,
        right[1] * cosine + up[1] * sine,
        right[2] * cosine + up[2] * sine,
    ];
    let rolled_up = [
        up[0] * cosine - right[0] * sine,
        up[1] * cosine - right[1] * sine,
        up[2] * cosine - right[2] * sine,
    ];
    right = rolled_right;
    up = rolled_up;
    let relative = subtract(point, position);
    let view_x = dot(relative, right);
    let view_y = dot(relative, up);
    let view_z = dot(relative, backward);
    let field_of_view = camera.field_of_view_degrees.clamp(1.0, 170.0);
    let focal = 1.0 / (field_of_view.to_radians() / 2.0).tan();
    let w = -view_z;
    [
        view_x * focal / aspect.max(f32::EPSILON),
        view_y * focal,
        w * 0.5,
        w,
    ]
}

fn rotate_x(vector: [f32; 3], radians: f32) -> [f32; 3] {
    let (sine, cosine) = radians.sin_cos();
    [
        vector[0],
        vector[1] * cosine - vector[2] * sine,
        vector[1] * sine + vector[2] * cosine,
    ]
}

fn rotate_y(vector: [f32; 3], radians: f32) -> [f32; 3] {
    let (sine, cosine) = radians.sin_cos();
    [
        vector[0] * cosine + vector[2] * sine,
        vector[1],
        -vector[0] * sine + vector[2] * cosine,
    ]
}

fn rotate_z(vector: [f32; 3], radians: f32) -> [f32; 3] {
    let (sine, cosine) = radians.sin_cos();
    [
        vector[0] * cosine - vector[1] * sine,
        vector[0] * sine + vector[1] * cosine,
        vector[2],
    ]
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(vector, vector).sqrt();
    (length > f32::EPSILON).then(|| [vector[0] / length, vector[1] / length, vector[2] / length])
}

fn f32_bytes<const N: usize>(values: [f32; N]) -> Vec<u8> {
    f32_slice_bytes(&values)
}

fn f32_slice_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .copied()
        .flat_map(f32::to_ne_bytes)
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32) -> VideoFrame {
        VideoFrame {
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
            timestamp_seconds: 0.0,
        }
    }

    fn composition_layer(
        frame: &VideoFrame,
        cache_key: u64,
        timeline_layer: i32,
        transform: LayerTransform,
    ) -> CompositionLayer<'_> {
        CompositionLayer {
            cache_key,
            source: CompositionSource::Frame(frame),
            timeline_layer,
            transform,
            blend_mode: BlendMode::Normal,
            crop: LayerCrop::default(),
            mask: None,
            effects: &[],
        }
    }

    fn ordered_cache_keys(
        layers: &[CompositionLayer<'_>],
        camera: Option<&CameraRenderPlan>,
    ) -> Vec<u64> {
        ordered_composition_layers(
            layers,
            ProjectionContext {
                target_size: (1920, 1080),
                camera,
            },
        )
        .into_iter()
        .map(|layer| layer.cache_key)
        .collect()
    }

    #[test]
    fn compositor_shader_parses_and_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(SHADER).expect("valid compositor WGSL");
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("validated compositor WGSL");
    }

    #[test]
    fn transparent_layers_render_far_to_near_before_timeline_order() {
        let pixels = frame(1, 1);
        let layers = [
            composition_layer(
                &pixels,
                1,
                99,
                LayerTransform {
                    z: 100.0,
                    ..Default::default()
                },
            ),
            composition_layer(
                &pixels,
                2,
                1,
                LayerTransform {
                    z: -100.0,
                    ..Default::default()
                },
            ),
        ];
        assert_eq!(ordered_cache_keys(&layers, None), vec![2, 1]);
    }

    #[test]
    fn equal_depth_layers_keep_aviqtl_timeline_stacking_order() {
        let pixels = frame(1, 1);
        let layers = [
            composition_layer(&pixels, 1, 1, LayerTransform::default()),
            composition_layer(&pixels, 2, 12, LayerTransform::default()),
            composition_layer(&pixels, 3, 4, LayerTransform::default()),
        ];
        assert_eq!(ordered_cache_keys(&layers, None), vec![2, 3, 1]);
    }

    #[test]
    fn custom_camera_direction_changes_transparent_layer_order() {
        let pixels = frame(1, 1);
        let layers = [
            composition_layer(
                &pixels,
                1,
                1,
                LayerTransform {
                    x: -100.0,
                    z: 100.0,
                    ..Default::default()
                },
            ),
            composition_layer(
                &pixels,
                2,
                2,
                LayerTransform {
                    x: 100.0,
                    z: -100.0,
                    ..Default::default()
                },
            ),
        ];
        assert_eq!(ordered_cache_keys(&layers, None), vec![2, 1]);

        let side_camera = CameraRenderPlan {
            position_x: 1_000.0,
            position_y: 0.0,
            position_z: 0.0,
            target_x: 0.0,
            target_y: 0.0,
            target_z: 0.0,
            roll_degrees: 0.0,
            field_of_view_degrees: 30.0,
        };
        assert_eq!(ordered_cache_keys(&layers, Some(&side_camera)), vec![1, 2]);
    }

    #[test]
    fn effect_passes_preserve_each_multi_pass_effect_original() {
        let effects = [
            VisualEffect::Fade { opacity: 0.5 },
            VisualEffect::Blur {
                size: 5.0,
                quality: 1.0,
            },
            VisualEffect::BorderBlur {
                size: 8.0,
                aspect: 0.0,
                blur_alpha: true,
            },
        ];
        assert_eq!(
            effect_pass_plan(&effects),
            vec![
                EffectPassPlan {
                    effect_index: 0,
                    pass_index: 0,
                    source: None,
                    original: None,
                    destination: 0,
                },
                EffectPassPlan {
                    effect_index: 1,
                    pass_index: 0,
                    source: Some(0),
                    original: Some(0),
                    destination: 1,
                },
                EffectPassPlan {
                    effect_index: 1,
                    pass_index: 1,
                    source: Some(1),
                    original: Some(0),
                    destination: 2,
                },
                EffectPassPlan {
                    effect_index: 2,
                    pass_index: 0,
                    source: Some(2),
                    original: Some(2),
                    destination: 0,
                },
                EffectPassPlan {
                    effect_index: 2,
                    pass_index: 1,
                    source: Some(0),
                    original: Some(2),
                    destination: 1,
                },
                EffectPassPlan {
                    effect_index: 2,
                    pass_index: 2,
                    source: Some(1),
                    original: Some(2),
                    destination: 0,
                },
            ]
        );
    }

    fn ndc(uniform: &[f32; 24], corner: usize) -> [f32; 2] {
        let offset = corner * 4;
        [
            uniform[offset] / uniform[offset + 3],
            uniform[offset + 1] / uniform[offset + 3],
        ]
    }

    #[test]
    fn transform_maps_pixels_to_clip_space() {
        let uniform = layer_uniform(
            (1920, 1080),
            (960, 540),
            LayerTransform {
                x: 480.0,
                y: 270.0,
                scale_percent: 50.0,
                opacity: 1.5,
                ..Default::default()
            },
            LayerCrop::default(),
            BlendMode::Normal,
            None,
            false,
        );
        let bottom_left = ndc(&uniform, 0);
        let top_right = ndc(&uniform, 3);
        assert!((bottom_left[0] - 0.25).abs() < 0.0001);
        assert!((bottom_left[1] - 0.25).abs() < 0.0001);
        assert!((top_right[0] - 0.75).abs() < 0.0001);
        assert!((top_right[1] - 0.75).abs() < 0.0001);
        assert_eq!(uniform[20], 1.0);
    }

    #[test]
    fn transform_clamps_negative_scale_and_opacity() {
        let uniform = layer_uniform(
            (0, 0),
            (4, 2),
            LayerTransform {
                scale_percent: -1.0,
                opacity: -0.5,
                ..Default::default()
            },
            LayerCrop::default(),
            BlendMode::Normal,
            None,
            false,
        );
        assert_eq!(ndc(&uniform, 0), [0.0, 0.0]);
        assert_eq!(ndc(&uniform, 3), [0.0, 0.0]);
        assert_eq!(uniform[20], 0.0);
    }

    #[test]
    fn crop_maps_source_pixels_and_recenters_the_layer() {
        let uniform = layer_uniform(
            (200, 100),
            (100, 50),
            LayerTransform::default(),
            LayerCrop {
                top: 5.0,
                bottom: 15.0,
                left: 10.0,
                right: 20.0,
                recenter: true,
            },
            BlendMode::Normal,
            None,
            false,
        );
        let centers = [
            (ndc(&uniform, 0)[0] + ndc(&uniform, 3)[0]) / 2.0,
            (ndc(&uniform, 0)[1] + ndc(&uniform, 3)[1]) / 2.0,
        ];
        assert!((centers[0] - 0.05).abs() < 0.0001);
        assert!((centers[1] + 0.1).abs() < 0.0001);
        assert_eq!(&uniform[16..20], &[0.1, 0.1, 0.8, 0.7]);
    }

    #[test]
    fn positive_z_moves_toward_the_default_camera_and_enlarges_the_quad() {
        let base = layer_uniform(
            (1920, 1080),
            (200, 100),
            LayerTransform::default(),
            LayerCrop::default(),
            BlendMode::Normal,
            None,
            false,
        );
        let closer = layer_uniform(
            (1920, 1080),
            (200, 100),
            LayerTransform {
                z: 500.0,
                ..Default::default()
            },
            LayerCrop::default(),
            BlendMode::Normal,
            None,
            false,
        );
        let base_width = ndc(&base, 1)[0] - ndc(&base, 0)[0];
        let closer_width = ndc(&closer, 1)[0] - ndc(&closer, 0)[0];
        assert!(closer_width > base_width);
    }

    #[test]
    fn positive_z_rotation_is_clockwise_like_the_qml_node() {
        let uniform = layer_uniform(
            (200, 100),
            (40, 20),
            LayerTransform {
                rotation_z_degrees: 90.0,
                ..Default::default()
            },
            LayerCrop::default(),
            BlendMode::Normal,
            None,
            false,
        );
        let bottom_left = ndc(&uniform, 0);
        let bottom_right = ndc(&uniform, 1);
        assert!(bottom_right[1] < bottom_left[1]);
        assert!((bottom_right[0] - bottom_left[0]).abs() < 0.0001);
    }

    #[test]
    fn hidden_backfaces_are_removed_before_rasterization() {
        let uniform = layer_uniform(
            (200, 100),
            (40, 20),
            LayerTransform {
                rotation_y_degrees: 180.0,
                backface_visible: false,
                ..Default::default()
            },
            LayerCrop::default(),
            BlendMode::Normal,
            None,
            true,
        );
        assert_eq!(uniform[22], 0.0);
        assert_eq!(uniform[23], 1.0);
    }

    #[test]
    fn persisted_blend_names_map_to_supported_gpu_modes() {
        let cases = [
            ("通常", BlendMode::Normal),
            ("スクリーン", BlendMode::Screen),
            ("乗算", BlendMode::Multiply),
            ("オーバーレイ", BlendMode::Overlay),
            ("加算", BlendMode::Add),
            ("減算", BlendMode::Subtract),
            ("比較（明）", BlendMode::Lighten),
            ("比較（暗）", BlendMode::Darken),
            ("色反転", BlendMode::Invert),
            ("ソフトライト", BlendMode::SoftLight),
            ("ハードライト", BlendMode::HardLight),
            ("差の絶対値", BlendMode::Difference),
            ("色相", BlendMode::Hue),
            ("彩度", BlendMode::Saturation),
            ("カラー", BlendMode::Color),
            ("輝度", BlendMode::Luminosity),
        ];
        for (id, (name, expected)) in cases.into_iter().enumerate() {
            assert_eq!(BlendMode::from_aviqtl_name(name), expected);
            assert_eq!(expected.shader_id(), id as u32);
        }
        assert_eq!(BlendMode::from_aviqtl_name("unknown"), BlendMode::Normal);
    }

    #[test]
    fn uniform_layout_is_six_wgsl_vec4_values() {
        assert_eq!(f32_bytes([0.0; 24]).len(), 96);
    }

    #[test]
    fn frame_stamp_changes_only_for_a_new_decoded_frame_or_size() {
        let first = frame(100, 50);
        let mut same_frame = frame(100, 50);
        same_frame.rgba.fill(255);
        let mut next_frame = frame(100, 50);
        next_frame.timestamp_seconds = 1.0 / 60.0;
        assert_eq!(FrameStamp::from(&first), FrameStamp::from(&same_frame));
        assert_ne!(FrameStamp::from(&first), FrameStamp::from(&next_frame));
        assert_ne!(FrameStamp::from(&first), FrameStamp::from(&frame(200, 50)));
    }
}
