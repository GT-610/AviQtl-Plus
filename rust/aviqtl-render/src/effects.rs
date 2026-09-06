use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq)]
pub enum VisualEffect {
    Fade {
        opacity: f32,
    },
    Monochrome {
        strength: f32,
        preserve_luma: bool,
        color: [f32; 3],
    },
    ColorCorrection {
        brightness: f32,
        contrast: f32,
        hue_degrees: f32,
        luminance: f32,
        saturation: f32,
        limited_range: bool,
    },
    ExtendedColorSettings {
        red: f32,
        green: f32,
        blue: f32,
        hue_degrees: f32,
        saturation: f32,
        value: f32,
    },
    LuminanceKey {
        threshold: f32,
        blend: f32,
        invert: bool,
    },
    ColorKey {
        color: [f32; 3],
        similarity: f32,
        blend: f32,
        invert: bool,
    },
    ChromaKey {
        hue: f32,
        hue_range: f32,
        similarity: f32,
        blend: f32,
        invert: bool,
    },
    SpecificColorRange {
        target_hue: f32,
        hue_range: f32,
        color: [f32; 3],
        strength: f32,
    },
    Gradient {
        strength: f32,
        center: [f32; 2],
        angle_degrees: f32,
        width: f32,
        shape: u32,
        start_color: [f32; 4],
        end_color: [f32; 4],
    },
    Flash {
        intensity: f32,
        speed: f32,
        kind: u32,
        frame: f32,
        color: [f32; 3],
    },
    Noise {
        strength: f32,
        seed: f32,
        time: f32,
    },
    Vignette {
        radius: f32,
        softness: f32,
        amount: f32,
    },
    Light {
        kind: u32,
        intensity: f32,
        radius: f32,
        position: [f32; 2],
        color: [f32; 3],
    },
    Mask {
        kind: u32,
        invert: bool,
        strength: f32,
    },
    DiagonalClipping {
        center: [f32; 2],
        angle_degrees: f32,
        width: f32,
        blur: f32,
    },
    Raster {
        width: f32,
        height: f32,
        speed: f32,
        angle_degrees: f32,
        frame: f32,
    },
    Sharpen {
        strength: f32,
        range: f32,
    },
    Emboss {
        width: f32,
        height: f32,
        angle_degrees: f32,
        strength: f32,
    },
    EdgeDetection {
        strength: f32,
        threshold: f32,
        luminance_edge: bool,
        alpha_edge: bool,
        color: [f32; 3],
    },
    Blur {
        size: f32,
        quality: f32,
    },
    BorderBlur {
        size: f32,
        aspect: f32,
        blur_alpha: bool,
    },
    DirectionalBlur {
        angle_degrees: f32,
        length: f32,
        fixed_size: bool,
    },
    RadialBlur {
        samples: f32,
        strength: f32,
        center: [f32; 2],
    },
    MotionBlur {
        quality: f32,
        shutter_speed: f32,
        velocity: [f32; 2],
        trail: bool,
    },
    LensBlur {
        radius: f32,
        brightness: f32,
    },
    Border {
        size: f32,
        blur: f32,
        color: [f32; 3],
    },
    DiffuseLight {
        strength: f32,
        diffusion: f32,
    },
    Emission {
        strength: f32,
        diffusion: f32,
        threshold: f32,
        color: [f32; 3],
        use_custom_color: bool,
    },
    Shadow {
        offset: [f32; 2],
        opacity: f32,
        diffusion: f32,
        color: [f32; 3],
    },
    Glow {
        intensity: f32,
        radius: f32,
        threshold: f32,
        color: [f32; 3],
    },
    DropShadow {
        radius: f32,
        offset: [f32; 2],
        strength: f32,
        color: [f32; 4],
    },
    ChromaticAberration {
        red_offset: [f32; 2],
        green_offset: [f32; 2],
        blue_offset: [f32; 2],
    },
    ImageLoop {
        count: [f32; 2],
        interval: [f32; 2],
        mirror: bool,
    },
    Mirror {
        transparency: f32,
        decay: f32,
        direction: i32,
        center_offset: f32,
    },
    Mosaic {
        size: f32,
    },
    PolarTransform {
        center: [f32; 2],
        scale: f32,
        angle_offset_degrees: f32,
    },
    Ripple {
        amplitude: f32,
        frequency: f32,
        speed: f32,
        center: [f32; 2],
        frame: f32,
    },
    Vibration {
        strength: [f32; 2],
        time: f32,
    },
    DisplacementMap {
        intensity: f32,
        kind: u32,
        scale: [f32; 2],
    },
    Glitch {
        scanline_intensity: f32,
        color_shift: f32,
        noise_amount: f32,
        speed: f32,
        frame: f32,
    },
    PixelSorter {
        block_size: f32,
        min_luma: f32,
        max_luma: f32,
        mix_amount: f32,
        direction: i32,
        reverse: bool,
    },
}

impl Hash for VisualEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        for value in self.encoded()[1..].iter().copied() {
            value.to_bits().hash(state);
        }
    }
}

impl VisualEffect {
    pub(crate) fn requires_sequential_passes(&self) -> bool {
        matches!(
            self,
            Self::Sharpen { .. }
                | Self::Emboss { .. }
                | Self::EdgeDetection { .. }
                | Self::Blur { .. }
                | Self::BorderBlur { .. }
                | Self::DirectionalBlur { .. }
                | Self::RadialBlur { .. }
                | Self::MotionBlur { .. }
                | Self::LensBlur { .. }
                | Self::Border { .. }
                | Self::DiffuseLight { .. }
                | Self::Emission { .. }
                | Self::Shadow { .. }
                | Self::Glow { .. }
                | Self::DropShadow { .. }
                | Self::ChromaticAberration { .. }
                | Self::ImageLoop { .. }
                | Self::Mirror { .. }
                | Self::Mosaic { .. }
                | Self::PolarTransform { .. }
                | Self::Ripple { .. }
                | Self::Vibration { .. }
                | Self::DisplacementMap { .. }
                | Self::Glitch { .. }
                | Self::PixelSorter { .. }
        )
    }

    pub(crate) fn pass_count(&self) -> usize {
        match self {
            Self::Blur { .. } => 2,
            Self::BorderBlur { .. } => 3,
            Self::Glow { .. } | Self::DropShadow { .. } => 2,
            _ => 1,
        }
    }

    pub(crate) fn encoded(&self) -> [f32; 16] {
        let mut values = [0.0; 16];
        match *self {
            Self::Fade { opacity } => {
                values[0] = 1.0;
                values[1] = opacity;
            }
            Self::Monochrome {
                strength,
                preserve_luma,
                color,
            } => {
                values[0] = 2.0;
                values[1] = strength;
                values[2] = f32::from(u8::from(preserve_luma));
                values[8..11].copy_from_slice(&color);
            }
            Self::ColorCorrection {
                brightness,
                contrast,
                hue_degrees,
                luminance,
                saturation,
                limited_range,
            } => {
                values[0] = 3.0;
                values[1] = brightness;
                values[2] = contrast;
                values[3] = hue_degrees;
                values[4] = luminance;
                values[5] = saturation;
                values[6] = f32::from(u8::from(limited_range));
            }
            Self::ExtendedColorSettings {
                red,
                green,
                blue,
                hue_degrees,
                saturation,
                value,
            } => {
                values[0] = 4.0;
                values[1] = red;
                values[2] = green;
                values[3] = blue;
                values[4] = hue_degrees;
                values[5] = saturation;
                values[6] = value;
            }
            Self::LuminanceKey {
                threshold,
                blend,
                invert,
            } => {
                values[0] = 5.0;
                values[1] = threshold;
                values[2] = blend;
                values[3] = f32::from(u8::from(invert));
            }
            Self::ColorKey {
                color,
                similarity,
                blend,
                invert,
            } => {
                values[0] = 6.0;
                values[1] = similarity;
                values[2] = blend;
                values[3] = f32::from(u8::from(invert));
                values[8..11].copy_from_slice(&color);
            }
            Self::ChromaKey {
                hue,
                hue_range,
                similarity,
                blend,
                invert,
            } => {
                values[0] = 7.0;
                values[1] = hue;
                values[2] = hue_range;
                values[3] = similarity;
                values[4] = blend;
                values[5] = f32::from(u8::from(invert));
            }
            Self::SpecificColorRange {
                target_hue,
                hue_range,
                color,
                strength,
            } => {
                values[0] = 8.0;
                values[1] = target_hue;
                values[2] = hue_range;
                values[3] = strength;
                values[8..11].copy_from_slice(&color);
            }
            Self::Gradient {
                strength,
                center,
                angle_degrees,
                width,
                shape,
                start_color,
                end_color,
            } => {
                values[0] = 9.0;
                values[1] = strength;
                values[2..4].copy_from_slice(&center);
                values[4] = angle_degrees;
                values[5] = width;
                values[6] = shape as f32;
                values[8..12].copy_from_slice(&start_color);
                values[12..16].copy_from_slice(&end_color);
            }
            Self::Flash {
                intensity,
                speed,
                kind,
                frame,
                color,
            } => {
                values[0] = 10.0;
                values[1] = intensity;
                values[2] = speed;
                values[3] = kind as f32;
                values[4] = frame;
                values[8..11].copy_from_slice(&color);
            }
            Self::Noise {
                strength,
                seed,
                time,
            } => {
                values[0] = 11.0;
                values[1] = strength;
                values[2] = seed;
                values[3] = time;
            }
            Self::Vignette {
                radius,
                softness,
                amount,
            } => {
                values[0] = 12.0;
                values[1] = radius;
                values[2] = softness;
                values[3] = amount;
            }
            Self::Light {
                kind,
                intensity,
                radius,
                position,
                color,
            } => {
                values[0] = 13.0;
                values[1] = kind as f32;
                values[2] = intensity;
                values[3] = radius;
                values[4..6].copy_from_slice(&position);
                values[8..11].copy_from_slice(&color);
            }
            Self::Mask {
                kind,
                invert,
                strength,
            } => {
                values[0] = 14.0;
                values[1] = kind as f32;
                values[2] = f32::from(u8::from(invert));
                values[3] = strength;
            }
            Self::DiagonalClipping {
                center,
                angle_degrees,
                width,
                blur,
            } => {
                values[0] = 15.0;
                values[1..3].copy_from_slice(&center);
                values[3] = angle_degrees;
                values[4] = width;
                values[5] = blur;
            }
            Self::Raster {
                width,
                height,
                speed,
                angle_degrees,
                frame,
            } => {
                values[0] = 16.0;
                values[1] = width;
                values[2] = height;
                values[3] = speed;
                values[4] = angle_degrees;
                values[5] = frame;
            }
            Self::Sharpen { strength, range } => {
                values[0] = 17.0;
                values[1] = strength;
                values[2] = range;
            }
            Self::Emboss {
                width,
                height,
                angle_degrees,
                strength,
            } => {
                values[0] = 18.0;
                values[1] = width;
                values[2] = height;
                values[3] = angle_degrees;
                values[4] = strength;
            }
            Self::EdgeDetection {
                strength,
                threshold,
                luminance_edge,
                alpha_edge,
                color,
            } => {
                values[0] = 19.0;
                values[1] = strength;
                values[2] = threshold;
                values[3] = f32::from(u8::from(luminance_edge));
                values[4] = f32::from(u8::from(alpha_edge));
                values[8..11].copy_from_slice(&color);
            }
            Self::Blur { size, quality } => {
                values[0] = 20.0;
                values[1] = size;
                values[2] = quality;
            }
            Self::BorderBlur {
                size,
                aspect,
                blur_alpha,
            } => {
                values[0] = 21.0;
                values[1] = size;
                values[2] = aspect;
                values[3] = f32::from(u8::from(blur_alpha));
            }
            Self::DirectionalBlur {
                angle_degrees,
                length,
                fixed_size,
            } => {
                values[0] = 22.0;
                values[1] = angle_degrees;
                values[2] = length;
                values[3] = f32::from(u8::from(fixed_size));
            }
            Self::RadialBlur {
                samples,
                strength,
                center,
            } => {
                values[0] = 23.0;
                values[1] = samples;
                values[2] = strength;
                values[3..5].copy_from_slice(&center);
            }
            Self::MotionBlur {
                quality,
                shutter_speed,
                velocity,
                trail,
            } => {
                values[0] = 24.0;
                values[1] = quality;
                values[2] = shutter_speed;
                values[3] = velocity[0];
                values[4] = velocity[1];
                values[5] = f32::from(u8::from(trail));
            }
            Self::LensBlur { radius, brightness } => {
                values[0] = 25.0;
                values[1] = radius;
                values[2] = brightness;
            }
            Self::Border { size, blur, color } => {
                values[0] = 26.0;
                values[1] = size;
                values[2] = blur;
                values[8..11].copy_from_slice(&color);
            }
            Self::DiffuseLight {
                strength,
                diffusion,
            } => {
                values[0] = 27.0;
                values[1] = strength;
                values[2] = diffusion;
            }
            Self::Emission {
                strength,
                diffusion,
                threshold,
                color,
                use_custom_color,
            } => {
                values[0] = 28.0;
                values[1] = strength;
                values[2] = diffusion;
                values[3] = threshold;
                values[4] = f32::from(u8::from(use_custom_color));
                values[8..11].copy_from_slice(&color);
            }
            Self::Shadow {
                offset,
                opacity,
                diffusion,
                color,
            } => {
                values[0] = 29.0;
                values[1..3].copy_from_slice(&offset);
                values[3] = opacity;
                values[4] = diffusion;
                values[8..11].copy_from_slice(&color);
            }
            Self::Glow {
                intensity,
                radius,
                threshold,
                color,
            } => {
                values[0] = 30.0;
                values[1] = intensity;
                values[2] = radius;
                values[3] = threshold;
                values[8..11].copy_from_slice(&color);
            }
            Self::DropShadow {
                radius,
                offset,
                strength,
                color,
            } => {
                values[0] = 31.0;
                values[1] = radius;
                values[2..4].copy_from_slice(&offset);
                values[4] = strength;
                values[8..12].copy_from_slice(&color);
            }
            Self::ChromaticAberration {
                red_offset,
                green_offset,
                blue_offset,
            } => {
                values[0] = 32.0;
                values[1..3].copy_from_slice(&red_offset);
                values[3] = green_offset[0];
                values[4] = green_offset[1];
                values[5..7].copy_from_slice(&blue_offset);
            }
            Self::ImageLoop {
                count,
                interval,
                mirror,
            } => {
                values[0] = 33.0;
                values[1..3].copy_from_slice(&count);
                values[3] = interval[0];
                values[4] = interval[1];
                values[5] = f32::from(u8::from(mirror));
            }
            Self::Mirror {
                transparency,
                decay,
                direction,
                center_offset,
            } => {
                values[0] = 34.0;
                values[1] = transparency;
                values[2] = decay;
                values[3] = direction as f32;
                values[4] = center_offset;
            }
            Self::Mosaic { size } => {
                values[0] = 35.0;
                values[1] = size;
            }
            Self::PolarTransform {
                center,
                scale,
                angle_offset_degrees,
            } => {
                values[0] = 36.0;
                values[1..3].copy_from_slice(&center);
                values[3] = scale;
                values[4] = angle_offset_degrees;
            }
            Self::Ripple {
                amplitude,
                frequency,
                speed,
                center,
                frame,
            } => {
                values[0] = 37.0;
                values[1] = amplitude;
                values[2] = frequency;
                values[3] = speed;
                values[4..6].copy_from_slice(&center);
                values[6] = frame;
            }
            Self::Vibration { strength, time } => {
                values[0] = 38.0;
                values[1..3].copy_from_slice(&strength);
                values[3] = time;
            }
            Self::DisplacementMap {
                intensity,
                kind,
                scale,
            } => {
                values[0] = 39.0;
                values[1] = intensity;
                values[2] = kind as f32;
                values[3] = scale[0];
                values[4] = scale[1];
            }
            Self::Glitch {
                scanline_intensity,
                color_shift,
                noise_amount,
                speed,
                frame,
            } => {
                values[0] = 40.0;
                values[1] = scanline_intensity;
                values[2] = color_shift;
                values[3] = noise_amount;
                values[4] = speed;
                values[5] = frame;
            }
            Self::PixelSorter {
                block_size,
                min_luma,
                max_luma,
                mix_amount,
                direction,
                reverse,
            } => {
                values[0] = 41.0;
                values[1] = block_size;
                values[2] = min_luma;
                values[3] = max_luma;
                values[4] = mix_amount;
                values[5] = direction as f32;
                values[6] = f32::from(u8::from(reverse));
            }
        }
        values
    }
}

pub(crate) fn encode_effect_pass(effect: &VisualEffect, pass_index: usize) -> Vec<f32> {
    let mut values = Vec::with_capacity(20);
    values.extend_from_slice(&[1.0, 0.0, 0.0, 0.0]);
    let mut encoded = effect.encoded();
    encoded[7] = pass_index as f32;
    values.extend_from_slice(&encoded);
    values
}

pub(crate) fn encode_effect_chain(effects: &[VisualEffect]) -> Vec<f32> {
    // wgpu validates a runtime-sized WGSL array as if it contains at least one
    // element. Keep one zeroed vec4 after the header for an empty chain so the
    // storage binding satisfies EffectChain's 32-byte minimum size.
    let payload_len = if effects.is_empty() {
        4
    } else {
        effects.len() * 16
    };
    let mut values = Vec::with_capacity(4 + payload_len);
    values.extend_from_slice(&[effects.len() as f32, 0.0, 0.0, 0.0]);
    if effects.is_empty() {
        values.extend_from_slice(&[0.0; 4]);
    }
    for effect in effects {
        values.extend_from_slice(&effect.encoded());
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_effect_chain_satisfies_wgsl_runtime_array_minimum_size() {
        let values = encode_effect_chain(&[]);

        assert_eq!(values, vec![0.0; 8]);
    }

    #[test]
    fn effect_chain_uses_four_wgsl_vec4_values_per_effect() {
        let values = encode_effect_chain(&[
            VisualEffect::Fade { opacity: 0.5 },
            VisualEffect::ColorKey {
                color: [0.0, 1.0, 0.0],
                similarity: 0.4,
                blend: 0.1,
                invert: false,
            },
        ]);
        assert_eq!(values.len(), 36);
        assert_eq!(values[0], 2.0);
        assert_eq!(values[4], 1.0);
        assert_eq!(values[20], 6.0);
        assert_eq!(&values[28..31], &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn effect_chain_encodes_procedural_effect_colors_and_timing() {
        let values = encode_effect_chain(&[
            VisualEffect::Gradient {
                strength: 1.5,
                center: [0.25, 0.75],
                angle_degrees: 90.0,
                width: 0.5,
                shape: 2,
                start_color: [0.1, 0.2, 0.3, 0.4],
                end_color: [0.5, 0.6, 0.7, 0.8],
            },
            VisualEffect::Raster {
                width: 10.0,
                height: 20.0,
                speed: -5.0,
                angle_degrees: 45.0,
                frame: 12.0,
            },
        ]);
        assert_eq!(values.len(), 36);
        assert_eq!(&values[4..11], &[9.0, 1.5, 0.25, 0.75, 90.0, 0.5, 2.0]);
        assert_eq!(&values[12..16], &[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(&values[16..20], &[0.5, 0.6, 0.7, 0.8]);
        assert_eq!(&values[20..26], &[16.0, 10.0, 20.0, -5.0, 45.0, 12.0]);
    }

    #[test]
    fn neighborhood_effects_request_ordered_render_passes() {
        let color_only = VisualEffect::Fade { opacity: 0.5 };
        let sharpen = VisualEffect::Sharpen {
            strength: 0.5,
            range: 1.0,
        };
        let edge = VisualEffect::EdgeDetection {
            strength: 1.0,
            threshold: 0.1,
            luminance_edge: true,
            alpha_edge: false,
            color: [1.0; 3],
        };
        assert!(!color_only.requires_sequential_passes());
        assert!(sharpen.requires_sequential_passes());
        assert!(edge.requires_sequential_passes());
    }

    #[test]
    fn compute_style_effects_encode_each_qt_dispatch_pass() {
        let blur = VisualEffect::Blur {
            size: 5.0,
            quality: 2.0,
        };
        let border_blur = VisualEffect::BorderBlur {
            size: 8.0,
            aspect: -25.0,
            blur_alpha: true,
        };
        assert_eq!(blur.pass_count(), 2);
        assert_eq!(border_blur.pass_count(), 3);
        let final_pass = encode_effect_pass(&border_blur, 2);
        assert_eq!(final_pass.len(), 20);
        assert_eq!(final_pass[0], 1.0);
        assert_eq!(&final_pass[4..8], &[21.0, 8.0, -25.0, 1.0]);
        assert_eq!(final_pass[11], 2.0);
    }

    #[test]
    fn coordinate_effects_encode_qt_shader_uniform_layouts() {
        let chromatic = VisualEffect::ChromaticAberration {
            red_offset: [1.0, 2.0],
            green_offset: [3.0, 4.0],
            blue_offset: [5.0, 6.0],
        };
        assert_eq!(
            &chromatic.encoded()[..8],
            &[32.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0]
        );

        let image_loop = VisualEffect::ImageLoop {
            count: [2.0, 3.0],
            interval: [4.0, 5.0],
            mirror: true,
        };
        assert_eq!(
            &image_loop.encoded()[..8],
            &[33.0, 2.0, 3.0, 4.0, 5.0, 1.0, 0.0, 0.0]
        );

        let ripple = VisualEffect::Ripple {
            amplitude: 0.25,
            frequency: 6.0,
            speed: -2.0,
            center: [0.4, 0.6],
            frame: 12.0,
        };
        assert_eq!(
            &ripple.encoded()[..8],
            &[37.0, 0.25, 6.0, -2.0, 0.4, 0.6, 12.0, 0.0]
        );

        let displacement = VisualEffect::DisplacementMap {
            intensity: 0.75,
            kind: 2,
            scale: [1.5, 0.5],
        };
        assert_eq!(
            &displacement.encoded()[..8],
            &[39.0, 0.75, 2.0, 1.5, 0.5, 0.0, 0.0, 0.0]
        );
        assert!(chromatic.requires_sequential_passes());
        assert!(image_loop.requires_sequential_passes());
        assert!(ripple.requires_sequential_passes());
        assert!(displacement.requires_sequential_passes());
    }

    #[test]
    fn compute_stylize_effects_encode_qt_shader_uniform_layouts() {
        let glitch = VisualEffect::Glitch {
            scanline_intensity: 0.3,
            color_shift: 2.0,
            noise_amount: 0.1,
            speed: 1.5,
            frame: 12.0,
        };
        assert_eq!(
            &glitch.encoded()[..8],
            &[40.0, 0.3, 2.0, 0.1, 1.5, 12.0, 0.0, 0.0]
        );

        let pixel_sorter = VisualEffect::PixelSorter {
            block_size: 24.0,
            min_luma: 0.15,
            max_luma: 0.9,
            mix_amount: 0.75,
            direction: 1,
            reverse: true,
        };
        assert_eq!(
            &pixel_sorter.encoded()[..8],
            &[41.0, 24.0, 0.15, 0.9, 0.75, 1.0, 1.0, 0.0]
        );
        assert!(glitch.requires_sequential_passes());
        assert!(pixel_sorter.requires_sequential_passes());
    }
}
