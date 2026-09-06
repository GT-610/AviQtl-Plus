use crate::decode::{
    DecodeKind, PreviewContent, PreviewScene, PreviewSource, frame_buffer_scene,
    upper_object_mask_scene,
};
use aviqtl_render::{BlendMode, LayerCrop, LayerTransform, VisualEffect};
use aviqtl_rust_core::api::{
    AudioLayerPlan, EvaluatedEffect, MediaKind, ProjectDocument, RenderLayerPlan, SceneRenderPlan,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

pub struct PlannedPreview {
    pub scene: PreviewScene,
    pub audio: Vec<AudioLayerPlan>,
    pub warnings: Vec<String>,
}

pub struct PreviewPlanner {
    plans: BTreeMap<i32, SceneRenderPlan>,
    project_directory: Option<PathBuf>,
}

impl PreviewPlanner {
    pub fn new(document: &ProjectDocument, project_path: Option<&Path>) -> Self {
        Self {
            plans: build_render_plans(document),
            project_directory: project_path.and_then(Path::parent).map(Path::to_path_buf),
        }
    }

    pub fn rebuild(&mut self, document: &ProjectDocument, project_path: Option<&Path>) {
        self.plans = build_render_plans(document);
        self.project_directory = project_path.and_then(Path::parent).map(Path::to_path_buf);
    }

    pub fn build(
        &mut self,
        document: &ProjectDocument,
        scene_id: i32,
        frame: i32,
    ) -> Option<PlannedPreview> {
        let mut stack = vec![scene_id];
        let mut warnings = Vec::new();
        let root_key = scene_instance_key(0, scene_id, scene_id);
        let (scene, audio) = self.build_scene(
            document,
            scene_id,
            frame,
            root_key,
            &mut stack,
            &mut warnings,
        )?;
        Some(PlannedPreview {
            scene,
            audio,
            warnings,
        })
    }

    fn build_scene(
        &mut self,
        document: &ProjectDocument,
        scene_id: i32,
        frame: i32,
        instance_key: u64,
        stack: &mut Vec<i32>,
        warnings: &mut Vec<String>,
    ) -> Option<(PreviewScene, Vec<AudioLayerPlan>)> {
        let (width, height, fps, hidden_layers) = document
            .scenes
            .iter()
            .find(|scene| scene.id == scene_id)
            .map(|scene| {
                (
                    scene.width.max(1) as u32,
                    scene.height.max(1) as u32,
                    scene.fps,
                    scene.hidden_layers.clone(),
                )
            })?;
        let frame_plan = self.plans.get_mut(&scene_id)?.evaluate(frame);
        let camera = frame_plan.camera;
        let audio = frame_plan.audio;
        let mut planned_layers = frame_plan.layers;
        planned_layers.sort_by_key(|layer| layer.timeline_layer);
        planned_layers.retain(|layer| !hidden_layers.contains(&layer.timeline_layer));
        let mut sources = Vec::with_capacity(planned_layers.len());
        for layer in planned_layers {
            let clear_below = layer.clip_type == "frame_buffer"
                && effect_bool(&layer, "frame_buffer", "clearBelow");
            let blend_mode = if clear_below {
                BlendMode::Normal
            } else {
                layer_blend_mode(&layer)
            };
            let crop = layer_crop(&layer);
            let aspect = effect_number(&layer, "transform", "aspect");
            let pivot_x = effect_number(&layer, "transform", "cx");
            let pivot_y = effect_number(&layer, "transform", "cy");
            let pivot_z = effect_number(&layer, "transform", "cz");
            let backface_visible = effect_bool_or(&layer, "transform", "backfaceVisible", true);
            let effects =
                layer_visual_effects(&layer.effects, layer.relative_frame, layer.duration_frames);
            let (content, content_opacity) = if layer.clip_type == "frame_buffer" {
                let capture_key =
                    auxiliary_scene_instance_key(instance_key, layer.clip_id, "frame-buffer");
                (
                    PreviewContent::Scene {
                        scene: Box::new(frame_buffer_scene(
                            capture_key,
                            width,
                            height,
                            camera,
                            layer.timeline_layer,
                            clear_below,
                            &sources,
                        )),
                    },
                    1.0,
                )
            } else if let Some(media) = layer.media {
                let path = resolve_project_path(self.project_directory.as_deref(), &media.path);
                let kind = match media.kind {
                    MediaKind::Image => DecodeKind::Image,
                    MediaKind::Video => DecodeKind::Video,
                };
                let opacity = media.opacity;
                (
                    PreviewContent::Media {
                        path,
                        kind,
                        playback: media,
                    },
                    opacity,
                )
            } else if let Some(shape) = layer.shape {
                let opacity = shape.opacity;
                (
                    PreviewContent::Shape {
                        plan: shape,
                        timestamp_seconds: f64::from(layer.relative_frame) / fps.max(1.0),
                    },
                    opacity,
                )
            } else if let Some(text) = layer.text {
                let opacity = text.opacity;
                (PreviewContent::Text { plan: text }, opacity)
            } else if let Some(procedural) = layer.procedural {
                let opacity = procedural.opacity();
                (
                    PreviewContent::Procedural {
                        plan: procedural,
                        timestamp_seconds: f64::from(layer.relative_frame) / fps.max(1.0),
                    },
                    opacity,
                )
            } else if let Some(nested) = layer.nested_scene {
                if stack.contains(&nested.target_scene_id) {
                    warnings.push(format!(
                        "clip #{} recursively references scene #{}",
                        layer.clip_id, nested.target_scene_id
                    ));
                    continue;
                }
                if stack.len() >= 32 {
                    warnings.push(format!(
                        "clip #{} exceeds the 32-scene nesting limit",
                        layer.clip_id
                    ));
                    continue;
                }
                stack.push(nested.target_scene_id);
                let nested_key =
                    scene_instance_key(instance_key, layer.clip_id, nested.target_scene_id);
                let nested_scene = self
                    .build_scene(
                        document,
                        nested.target_scene_id,
                        nested.target_frame,
                        nested_key,
                        stack,
                        warnings,
                    )
                    .map(|(scene, _)| scene);
                stack.pop();
                let Some(nested_scene) = nested_scene else {
                    warnings.push(format!(
                        "clip #{} targets missing scene #{}",
                        layer.clip_id, nested.target_scene_id
                    ));
                    continue;
                };
                (
                    PreviewContent::Scene {
                        scene: Box::new(nested_scene),
                    },
                    nested.opacity,
                )
            } else {
                if layer.clip_type == "scene" {
                    warnings.push(format!("clip #{} has no valid target scene", layer.clip_id));
                }
                continue;
            };
            let mask = if layer.clip_by_upper_object && layer.timeline_layer > 0 {
                let mask_key =
                    auxiliary_scene_instance_key(instance_key, layer.clip_id, "upper-mask");
                upper_object_mask_scene(
                    mask_key,
                    width,
                    height,
                    camera,
                    layer.timeline_layer,
                    &sources,
                )
                .map(Box::new)
            } else {
                None
            };
            sources.push(PreviewSource {
                clip_id: layer.clip_id,
                content,
                timeline_layer: layer.timeline_layer,
                transform: LayerTransform {
                    x: layer.transform.x,
                    y: layer.transform.y,
                    z: layer.transform.z,
                    scale_percent: layer.transform.scale_percent,
                    aspect,
                    rotation_x_degrees: layer.transform.rotation_x,
                    rotation_y_degrees: layer.transform.rotation_y,
                    rotation_z_degrees: layer.transform.rotation_z,
                    pivot_x,
                    pivot_y,
                    pivot_z,
                    opacity: content_opacity * layer.transform.opacity,
                    backface_visible,
                },
                blend_mode,
                crop,
                mask,
                effects,
            });
        }
        Some((
            PreviewScene {
                instance_key,
                width,
                height,
                camera,
                opaque_background: false,
                layers: sources,
            },
            audio,
        ))
    }
}

fn build_render_plans(document: &ProjectDocument) -> BTreeMap<i32, SceneRenderPlan> {
    document
        .scenes
        .iter()
        .filter_map(|scene| {
            SceneRenderPlan::from_document(document, scene.id)
                .ok()
                .map(|plan| (scene.id, plan))
        })
        .collect()
}

fn resolve_project_path(project_directory: Option<&Path>, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_relative()
        && let Some(directory) = project_directory
    {
        return directory.join(path);
    }
    path
}

fn scene_instance_key(parent: u64, clip_id: i32, scene_id: i32) -> u64 {
    let mut hasher = DefaultHasher::new();
    parent.hash(&mut hasher);
    clip_id.hash(&mut hasher);
    scene_id.hash(&mut hasher);
    hasher.finish()
}

fn auxiliary_scene_instance_key(parent: u64, clip_id: i32, kind: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    parent.hash(&mut hasher);
    clip_id.hash(&mut hasher);
    kind.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Copy)]
struct ParsedColor([u8; 4]);

impl ParsedColor {
    fn to_srgba_unmultiplied(self) -> [u8; 4] {
        self.0
    }
}

fn parse_qt_color(value: &str) -> Option<ParsedColor> {
    let digits = value.strip_prefix('#')?;
    let parsed = u32::from_str_radix(digits, 16).ok()?;
    Some(ParsedColor(match digits.len() {
        6 => [
            ((parsed >> 16) & 0xff) as u8,
            ((parsed >> 8) & 0xff) as u8,
            (parsed & 0xff) as u8,
            255,
        ],
        8 => [
            ((parsed >> 16) & 0xff) as u8,
            ((parsed >> 8) & 0xff) as u8,
            (parsed & 0xff) as u8,
            ((parsed >> 24) & 0xff) as u8,
        ],
        _ => return None,
    }))
}
fn effect_value<'a>(
    layer: &'a RenderLayerPlan,
    effect_id: &str,
    name: &str,
) -> Option<&'a serde_json::Value> {
    layer
        .effects
        .iter()
        .find(|effect| effect.id == effect_id)
        .and_then(|effect| effect.params.get(name))
        .map(value_payload)
}

fn evaluated_effect_value<'a>(effect: &'a EvaluatedEffect, name: &str) -> Option<&'a Value> {
    effect.params.get(name).map(value_payload)
}

fn evaluated_effect_number(effect: &EvaluatedEffect, name: &str, fallback: f32) -> f32 {
    evaluated_effect_value(effect, name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .map_or(fallback, |value| value as f32)
}

fn evaluated_effect_bool(effect: &EvaluatedEffect, name: &str, fallback: bool) -> bool {
    let Some(value) = evaluated_effect_value(effect, name) else {
        return fallback;
    };
    value.as_bool().unwrap_or_else(|| {
        value
            .as_f64()
            .is_some_and(|number| number.is_finite() && number != 0.0)
    })
}

fn evaluated_effect_color(effect: &EvaluatedEffect, name: &str, fallback: &str) -> [f32; 3] {
    let [red, green, blue, _] = evaluated_effect_rgba(effect, name, fallback);
    [red, green, blue]
}

fn evaluated_effect_rgba(effect: &EvaluatedEffect, name: &str, fallback: &str) -> [f32; 4] {
    let color = evaluated_effect_value(effect, name)
        .and_then(Value::as_str)
        .and_then(parse_qt_color)
        .or_else(|| parse_qt_color(fallback))
        .expect("built-in effect fallback colors are valid");
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    [
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        f32::from(alpha) / 255.0,
    ]
}

fn layer_visual_effects(
    effects: &[EvaluatedEffect],
    relative_frame: i32,
    duration_frames: i32,
) -> Vec<VisualEffect> {
    effects
        .iter()
        .filter_map(|effect| match effect.id.as_str() {
            "fade" => {
                let fade_in = evaluated_effect_number(effect, "fadeIn", 0.0).clamp(0.0, 100.0);
                let fade_out = evaluated_effect_number(effect, "fadeOut", 0.0).clamp(0.0, 100.0);
                let fade_in_duration =
                    evaluated_effect_number(effect, "fadeInDuration", 30.0).clamp(1.0, 300.0);
                let fade_out_duration =
                    evaluated_effect_number(effect, "fadeOutDuration", 30.0).clamp(1.0, 300.0);
                let frame = relative_frame as f32;
                let duration = duration_frames as f32;
                let opacity = if fade_in > 0.0 && frame < fade_in_duration {
                    frame / fade_in_duration
                } else if fade_out > 0.0 && frame > duration - fade_out_duration {
                    (duration - frame) / fade_out_duration
                } else {
                    1.0
                };
                Some(VisualEffect::Fade { opacity })
            }
            "monochrome" => Some(VisualEffect::Monochrome {
                strength: evaluated_effect_number(effect, "strength", 100.0) / 100.0,
                preserve_luma: evaluated_effect_bool(effect, "preserveLuma", true),
                color: evaluated_effect_color(effect, "color", "#ffffff"),
            }),
            "color_correction" => Some(VisualEffect::ColorCorrection {
                brightness: evaluated_effect_number(effect, "brightness", 100.0) / 100.0 - 1.0,
                contrast: evaluated_effect_number(effect, "contrast", 100.0) / 100.0 - 1.0,
                hue_degrees: evaluated_effect_number(effect, "hue", 0.0),
                luminance: evaluated_effect_number(effect, "lightness", 100.0) / 100.0 - 1.0,
                saturation: evaluated_effect_number(effect, "saturation", 100.0) / 100.0 - 1.0,
                limited_range: evaluated_effect_bool(effect, "saturate", false),
            }),
            "extended_color_settings" => Some(VisualEffect::ExtendedColorSettings {
                red: evaluated_effect_number(effect, "red", 100.0).clamp(0.0, 200.0) / 100.0,
                green: evaluated_effect_number(effect, "green", 100.0).clamp(0.0, 200.0) / 100.0,
                blue: evaluated_effect_number(effect, "blue", 100.0).clamp(0.0, 200.0) / 100.0,
                hue_degrees: evaluated_effect_number(effect, "hue", 0.0).clamp(-180.0, 180.0),
                saturation: evaluated_effect_number(effect, "saturation", 100.0).clamp(0.0, 200.0)
                    / 100.0,
                value: evaluated_effect_number(effect, "value", 100.0).clamp(0.0, 200.0) / 100.0,
            }),
            "luminance_key" => Some(VisualEffect::LuminanceKey {
                threshold: evaluated_effect_number(effect, "threshold", 50.0).clamp(0.0, 100.0)
                    / 100.0,
                blend: evaluated_effect_number(effect, "blend", 10.0).clamp(0.0, 100.0) / 100.0,
                invert: evaluated_effect_bool(effect, "invert", false),
            }),
            "color_key" => Some(VisualEffect::ColorKey {
                color: evaluated_effect_color(effect, "keyColor", "#00ff00"),
                similarity: evaluated_effect_number(effect, "similarity", 40.0).clamp(0.0, 100.0)
                    / 100.0,
                blend: evaluated_effect_number(effect, "blend", 10.0).clamp(0.0, 100.0) / 100.0,
                invert: evaluated_effect_bool(effect, "invert", false),
            }),
            "chroma_key" => Some(VisualEffect::ChromaKey {
                hue: evaluated_effect_number(effect, "hue", 120.0) / 360.0,
                hue_range: evaluated_effect_number(effect, "hueRange", 30.0) / 360.0,
                similarity: evaluated_effect_number(effect, "similarity", 40.0) / 100.0,
                blend: evaluated_effect_number(effect, "blend", 10.0) / 100.0,
                invert: evaluated_effect_bool(effect, "invert", false),
            }),
            "specific_color_range" => Some(VisualEffect::SpecificColorRange {
                target_hue: evaluated_effect_number(effect, "targetHue", 120.0).clamp(0.0, 360.0)
                    / 360.0,
                hue_range: evaluated_effect_number(effect, "hueRange", 30.0).clamp(0.0, 180.0)
                    / 360.0,
                color: evaluated_effect_color(effect, "targetColor", "#ff0000"),
                strength: evaluated_effect_number(effect, "strength", 100.0).clamp(0.0, 100.0)
                    / 100.0,
            }),
            "gradient" => Some(VisualEffect::Gradient {
                strength: evaluated_effect_number(effect, "strength", 100.0) / 100.0,
                center: [
                    evaluated_effect_number(effect, "centerX", 50.0) / 100.0,
                    evaluated_effect_number(effect, "centerY", 50.0) / 100.0,
                ],
                angle_degrees: evaluated_effect_number(effect, "angle", 0.0),
                width: evaluated_effect_number(effect, "width", 100.0) / 100.0,
                shape: evaluated_effect_number(effect, "shape", 0.0)
                    .round()
                    .clamp(0.0, 2.0) as u32,
                start_color: evaluated_effect_rgba(effect, "startColor", "#00000000"),
                end_color: evaluated_effect_rgba(effect, "endColor", "#ffffffff"),
            }),
            "flash" => Some(VisualEffect::Flash {
                intensity: evaluated_effect_number(effect, "intensity", 100.0).clamp(0.0, 200.0)
                    / 100.0,
                speed: evaluated_effect_number(effect, "speed", 50.0).clamp(0.0, 100.0) / 100.0,
                kind: evaluated_effect_number(effect, "type", 0.0)
                    .round()
                    .clamp(0.0, 2.0) as u32,
                frame: relative_frame as f32,
                color: evaluated_effect_color(effect, "color", "#ffffff"),
            }),
            "noise" => Some(VisualEffect::Noise {
                strength: evaluated_effect_number(effect, "strength", 30.0) / 100.0,
                seed: evaluated_effect_number(effect, "seed", 0.0),
                time: relative_frame as f32 * evaluated_effect_number(effect, "speed", 0.0) * 0.01,
            }),
            "vignette" => Some(VisualEffect::Vignette {
                radius: evaluated_effect_number(effect, "radius", 0.75),
                softness: evaluated_effect_number(effect, "softness", 0.45),
                amount: evaluated_effect_number(effect, "amount", 1.0),
            }),
            "light" => Some(VisualEffect::Light {
                kind: evaluated_effect_number(effect, "lightType", 0.0)
                    .round()
                    .clamp(0.0, 2.0) as u32,
                intensity: evaluated_effect_number(effect, "intensity", 100.0).clamp(0.0, 200.0)
                    / 100.0,
                radius: evaluated_effect_number(effect, "radius", 50.0).clamp(0.0, 100.0) / 100.0,
                position: [
                    evaluated_effect_number(effect, "x", 50.0).clamp(0.0, 100.0) / 100.0,
                    evaluated_effect_number(effect, "y", 50.0).clamp(0.0, 100.0) / 100.0,
                ],
                color: evaluated_effect_color(effect, "color", "#ffffff"),
            }),
            "mask" => Some(VisualEffect::Mask {
                kind: evaluated_effect_number(effect, "maskType", 0.0)
                    .round()
                    .clamp(0.0, 3.0) as u32,
                invert: evaluated_effect_bool(effect, "invertMask", false),
                strength: evaluated_effect_number(effect, "maskStrength", 100.0).clamp(0.0, 100.0)
                    / 100.0,
            }),
            "diagonal_clipping" => Some(VisualEffect::DiagonalClipping {
                center: [
                    evaluated_effect_number(effect, "centerX", 0.0),
                    evaluated_effect_number(effect, "centerY", 0.0),
                ],
                angle_degrees: evaluated_effect_number(effect, "angle", 0.0),
                width: evaluated_effect_number(effect, "width", 0.0),
                blur: evaluated_effect_number(effect, "blur", 0.0).max(0.0),
            }),
            "raster" => Some(VisualEffect::Raster {
                width: evaluated_effect_number(effect, "width", 10.0).clamp(1.0, 100.0),
                height: evaluated_effect_number(effect, "height", 10.0).clamp(1.0, 100.0),
                speed: evaluated_effect_number(effect, "speed", 0.0).clamp(-100.0, 100.0),
                angle_degrees: evaluated_effect_number(effect, "angle", 0.0).clamp(0.0, 360.0),
                frame: relative_frame as f32,
            }),
            "sharpen" => Some(VisualEffect::Sharpen {
                strength: evaluated_effect_number(effect, "strength", 50.0) / 100.0,
                range: evaluated_effect_number(effect, "range", 100.0) / 100.0,
            }),
            "emboss" => Some(VisualEffect::Emboss {
                width: evaluated_effect_number(effect, "width", 200.0) / 100.0,
                height: evaluated_effect_number(effect, "height", 200.0) / 100.0,
                angle_degrees: evaluated_effect_number(effect, "angle", 135.0),
                strength: evaluated_effect_number(effect, "strength", 100.0) / 100.0,
            }),
            "edge_detection" => Some(VisualEffect::EdgeDetection {
                strength: evaluated_effect_number(effect, "strength", 100.0) / 100.0,
                threshold: evaluated_effect_number(effect, "threshold", 20.0) / 255.0,
                luminance_edge: evaluated_effect_bool(effect, "luminanceEdge", true),
                alpha_edge: evaluated_effect_bool(effect, "alphaEdge", false),
                color: evaluated_effect_color(effect, "color", "#ffffff"),
            }),
            "blur" => Some(VisualEffect::Blur {
                size: evaluated_effect_number(effect, "size", 5.0).max(0.0),
                quality: evaluated_effect_number(effect, "quality", 1.0)
                    .round()
                    .clamp(1.0, 3.0),
            }),
            "border_blur" => Some(VisualEffect::BorderBlur {
                size: evaluated_effect_number(effect, "size", 5.0).max(0.0),
                aspect: evaluated_effect_number(effect, "aspect", 0.0).clamp(-100.0, 100.0),
                blur_alpha: evaluated_effect_bool(effect, "blur_alpha", true),
            }),
            "directional_blur" => Some(VisualEffect::DirectionalBlur {
                angle_degrees: evaluated_effect_number(effect, "angle", 0.0),
                length: evaluated_effect_number(effect, "length", 10.0).max(0.0),
                fixed_size: evaluated_effect_bool(effect, "fixedSize", false),
            }),
            "radial_blur" => Some(VisualEffect::RadialBlur {
                samples: evaluated_effect_number(effect, "samples", 10.0),
                strength: (evaluated_effect_number(effect, "strength", 0.1) * 2000.0).max(0.0),
                center: [
                    evaluated_effect_number(effect, "x", 0.0),
                    evaluated_effect_number(effect, "y", 0.0),
                ],
            }),
            "motion_blur" => Some(VisualEffect::MotionBlur {
                quality: evaluated_effect_number(effect, "quality", 16.0).clamp(1.0, 100.0),
                shutter_speed: evaluated_effect_number(effect, "shutterSpeed", 50.0)
                    .clamp(0.0, 100.0),
                velocity: [
                    evaluated_effect_number(effect, "velX", 0.0),
                    evaluated_effect_number(effect, "velY", 0.0),
                ],
                trail: evaluated_effect_bool(effect, "trail", false),
            }),
            "lens_blur" => Some(VisualEffect::LensBlur {
                radius: evaluated_effect_number(effect, "radius", 10.0).max(0.0),
                brightness: evaluated_effect_number(effect, "brightness", 0.0).max(0.0),
            }),
            "border" => Some(VisualEffect::Border {
                size: evaluated_effect_number(effect, "size", 3.0),
                blur: evaluated_effect_number(effect, "blur", 0.0),
                color: evaluated_effect_color(effect, "color", "#ff0000"),
            }),
            "diffuse_light" => Some(VisualEffect::DiffuseLight {
                strength: evaluated_effect_number(effect, "strength", 100.0).max(0.0) / 100.0,
                diffusion: evaluated_effect_number(effect, "diffusion", 10.0).max(0.0),
            }),
            "emission" => {
                let custom_color = evaluated_effect_value(effect, "color")
                    .and_then(Value::as_str)
                    .filter(|color| !color.is_empty())
                    .and_then(parse_qt_color)
                    .map(|color| {
                        let [red, green, blue, _] = color.to_srgba_unmultiplied();
                        [
                            f32::from(red) / 255.0,
                            f32::from(green) / 255.0,
                            f32::from(blue) / 255.0,
                        ]
                    });
                Some(VisualEffect::Emission {
                    strength: evaluated_effect_number(effect, "strength", 50.0) / 100.0,
                    diffusion: evaluated_effect_number(effect, "diffusion", 20.0) / 100.0,
                    threshold: evaluated_effect_number(effect, "threshold", 80.0) / 100.0,
                    color: custom_color.unwrap_or([0.0; 3]),
                    use_custom_color: custom_color.is_some(),
                })
            }
            "shadow" => Some(VisualEffect::Shadow {
                offset: [
                    evaluated_effect_number(effect, "x", 5.0),
                    evaluated_effect_number(effect, "y", 5.0),
                ],
                opacity: evaluated_effect_number(effect, "opacity", 80.0) / 100.0,
                diffusion: evaluated_effect_number(effect, "diffusion", 5.0) / 100.0,
                color: evaluated_effect_color(effect, "color", "#000000"),
            }),
            "glow" => Some(VisualEffect::Glow {
                intensity: evaluated_effect_number(effect, "intensity", 100.0).clamp(0.0, 200.0)
                    / 100.0,
                radius: evaluated_effect_number(effect, "radius", 10.0).clamp(0.0, 50.0),
                threshold: evaluated_effect_number(effect, "threshold", 50.0).clamp(0.0, 100.0)
                    / 100.0,
                color: evaluated_effect_color(effect, "color", "#ffffff"),
            }),
            "drop_shadow" => Some(VisualEffect::DropShadow {
                radius: evaluated_effect_number(effect, "radius", 10.0).max(0.0),
                offset: [
                    evaluated_effect_number(effect, "x", 5.0),
                    evaluated_effect_number(effect, "y", 5.0),
                ],
                strength: evaluated_effect_number(effect, "opacity", 100.0).clamp(0.0, 100.0)
                    / 100.0,
                color: evaluated_effect_rgba(effect, "color", "#80000000"),
            }),
            "chromatic_aberration" => Some(VisualEffect::ChromaticAberration {
                red_offset: [
                    evaluated_effect_number(effect, "redX", 5.0),
                    evaluated_effect_number(effect, "redY", 0.0),
                ],
                green_offset: [
                    evaluated_effect_number(effect, "greenX", 0.0),
                    evaluated_effect_number(effect, "greenY", 0.0),
                ],
                blue_offset: [
                    evaluated_effect_number(effect, "blueX", -5.0),
                    evaluated_effect_number(effect, "blueY", 0.0),
                ],
            }),
            "image_loop" => Some(VisualEffect::ImageLoop {
                count: [
                    evaluated_effect_number(effect, "countX", 2.0)
                        .round()
                        .max(1.0),
                    evaluated_effect_number(effect, "countY", 2.0)
                        .round()
                        .max(1.0),
                ],
                interval: [
                    evaluated_effect_number(effect, "intervalX", 0.0),
                    evaluated_effect_number(effect, "intervalY", 0.0),
                ],
                mirror: evaluated_effect_bool(effect, "mirror", false),
            }),
            "mirror" => Some(VisualEffect::Mirror {
                transparency: evaluated_effect_number(effect, "transparency", 50.0) / 100.0,
                decay: evaluated_effect_number(effect, "decay", 0.0) / 100.0,
                direction: evaluated_effect_number(effect, "direction", 0.0) as i32,
                center_offset: evaluated_effect_number(effect, "centerOffset", 0.0) / 100.0,
            }),
            "mosaic" => Some(VisualEffect::Mosaic {
                size: evaluated_effect_number(effect, "size", 10.0).max(1.0),
            }),
            "polar_transform" => Some(VisualEffect::PolarTransform {
                center: [
                    evaluated_effect_number(effect, "centerX", 50.0).clamp(0.0, 100.0) / 100.0,
                    evaluated_effect_number(effect, "centerY", 50.0).clamp(0.0, 100.0) / 100.0,
                ],
                scale: evaluated_effect_number(effect, "scale", 100.0).clamp(10.0, 200.0) / 100.0,
                angle_offset_degrees: evaluated_effect_number(effect, "angleOffset", 0.0)
                    .clamp(0.0, 360.0),
            }),
            "ripple" => Some(VisualEffect::Ripple {
                amplitude: evaluated_effect_number(effect, "amplitude", 10.0).clamp(0.0, 100.0)
                    / 100.0,
                frequency: evaluated_effect_number(effect, "frequency", 5.0).clamp(1.0, 50.0),
                speed: evaluated_effect_number(effect, "speed", 1.0).clamp(-10.0, 10.0),
                center: [
                    evaluated_effect_number(effect, "centerX", 50.0).clamp(0.0, 100.0) / 100.0,
                    evaluated_effect_number(effect, "centerY", 50.0).clamp(0.0, 100.0) / 100.0,
                ],
                frame: relative_frame as f32,
            }),
            "vibration" => Some(VisualEffect::Vibration {
                strength: [
                    evaluated_effect_number(effect, "x", 10.0),
                    evaluated_effect_number(effect, "y", 10.0),
                ],
                time: relative_frame as f32 * evaluated_effect_number(effect, "speed", 20.0) / 10.0
                    + evaluated_effect_number(effect, "seed", 0.0),
            }),
            "displacement_map" => Some(VisualEffect::DisplacementMap {
                intensity: evaluated_effect_number(effect, "intensity", 10.0).clamp(0.0, 100.0)
                    / 100.0,
                kind: evaluated_effect_number(effect, "mapType", 0.0)
                    .round()
                    .clamp(0.0, 3.0) as u32,
                scale: [
                    evaluated_effect_number(effect, "scaleX", 100.0).clamp(10.0, 200.0) / 100.0,
                    evaluated_effect_number(effect, "scaleY", 100.0).clamp(10.0, 200.0) / 100.0,
                ],
            }),
            "glitch" => Some(VisualEffect::Glitch {
                scanline_intensity: evaluated_effect_number(effect, "scanlineIntensity", 0.3),
                color_shift: evaluated_effect_number(effect, "colorShift", 2.0),
                noise_amount: evaluated_effect_number(effect, "noiseAmount", 0.1),
                speed: evaluated_effect_number(effect, "speed", 1.0),
                frame: relative_frame as f32,
            }),
            "pixelsorter" => Some(VisualEffect::PixelSorter {
                block_size: evaluated_effect_number(effect, "blockSize", 24.0),
                min_luma: evaluated_effect_number(effect, "minLuma", 0.15),
                max_luma: evaluated_effect_number(effect, "maxLuma", 0.9),
                mix_amount: evaluated_effect_number(effect, "mix", 1.0),
                direction: evaluated_effect_number(effect, "direction", 0.0) as i32,
                reverse: evaluated_effect_bool(effect, "reverse", false),
            }),
            // The Qt catalog wrapper does not provide blend_layer.comp's required background
            // texture, so ComputeRenderNode displays its unchanged input fallback. Actual layer
            // and frame-buffer blending is handled by the compositor instead.
            "blend_layer" => None,
            _ => None,
        })
        .collect()
}

fn value_payload(value: &serde_json::Value) -> &serde_json::Value {
    value
        .as_object()
        .filter(|object| object.contains_key("$aviqtlType"))
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
}

fn effect_number(layer: &RenderLayerPlan, effect_id: &str, name: &str) -> f32 {
    effect_value(layer, effect_id, name)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(0.0) as f32
}

fn effect_bool(layer: &RenderLayerPlan, effect_id: &str, name: &str) -> bool {
    effect_bool_or(layer, effect_id, name, false)
}

fn effect_bool_or(layer: &RenderLayerPlan, effect_id: &str, name: &str, fallback: bool) -> bool {
    let Some(value) = effect_value(layer, effect_id, name) else {
        return fallback;
    };
    value.as_bool().unwrap_or_else(|| {
        value
            .as_f64()
            .is_some_and(|value| value.is_finite() && value != 0.0)
    })
}

fn layer_blend_mode(layer: &RenderLayerPlan) -> BlendMode {
    effect_value(layer, "transform", "blendMode")
        .and_then(serde_json::Value::as_str)
        .map_or(BlendMode::Normal, BlendMode::from_aviqtl_name)
}

fn layer_crop(layer: &RenderLayerPlan) -> LayerCrop {
    LayerCrop {
        top: effect_number(layer, "clipping", "top"),
        bottom: effect_number(layer, "clipping", "bottom"),
        left: effect_number(layer, "clipping", "left"),
        right: effect_number(layer, "clipping", "right"),
        recenter: effect_bool(layer, "clipping", "center"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::TimelineState;
    use serde_json::json;

    fn document(value: serde_json::Value) -> ProjectDocument {
        let bytes = serde_json::to_vec(&value).expect("preview fixture serializes");
        TimelineState::from_json(&bytes)
            .expect("preview fixture parses")
            .snapshot()
    }

    #[test]
    fn empty_scene_keeps_project_dimensions_and_has_no_layers() {
        let document = document(json!({
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
            "scenes": [{
                "id": 1,
                "name": "Root",
                "width": 1280,
                "height": 720,
                "fps": 30.0,
                "duration": 300
            }],
            "clips": []
        }));
        let mut planner = PreviewPlanner::new(&document, None);

        let planned = planner.build(&document, 1, 0).expect("scene plans");

        assert_eq!((planned.scene.width, planned.scene.height), (1280, 720));
        assert!(planned.scene.layers.is_empty());
        assert!(planned.audio.is_empty());
        assert!(planned.warnings.is_empty());
    }

    #[test]
    fn media_paths_are_resolved_relative_to_the_project_file() {
        let document = document(json!({
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
            "scenes": [{"id": 1, "name": "Root", "duration": 300}],
            "clips": [{
                "id": 7,
                "sceneId": 1,
                "type": "image",
                "start": 0,
                "duration": 60,
                "layer": 2,
                "effects": [{"id": "image", "params": {"path": "media/still.png"}}]
            }]
        }));
        let project_path = Path::new("/tmp/aviqtl-preview/project.aviqtl");
        let mut planner = PreviewPlanner::new(&document, Some(project_path));

        let planned = planner.build(&document, 1, 0).expect("scene plans");

        let PreviewContent::Media { path, .. } = &planned.scene.layers[0].content else {
            panic!("image clip should remain a media source");
        };
        assert_eq!(path, Path::new("/tmp/aviqtl-preview/media/still.png"));
    }

    #[test]
    fn recursive_scene_references_are_skipped_with_a_warning() {
        let document = document(json!({
            "version": 3,
            "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
            "scenes": [
                {"id": 1, "name": "Root", "duration": 300},
                {"id": 2, "name": "Nested", "duration": 300}
            ],
            "clips": [
                {
                    "id": 10,
                    "sceneId": 1,
                    "type": "scene",
                    "start": 0,
                    "duration": 60,
                    "layer": 1,
                    "effects": [{"id": "scene", "params": {"targetSceneId": 2}}]
                },
                {
                    "id": 11,
                    "sceneId": 2,
                    "type": "scene",
                    "start": 0,
                    "duration": 60,
                    "layer": 1,
                    "effects": [{"id": "scene", "params": {"targetSceneId": 1}}]
                }
            ]
        }));
        let mut planner = PreviewPlanner::new(&document, None);

        let planned = planner.build(&document, 1, 0).expect("scene plans");

        assert_eq!(planned.warnings.len(), 1);
        assert!(planned.warnings[0].contains("recursively references scene #1"));
        let PreviewContent::Scene { scene } = &planned.scene.layers[0].content else {
            panic!("outer scene reference should remain visible");
        };
        assert!(scene.layers.is_empty());
    }
}
