use aviqtl_media::VideoFrame;
use aviqtl_rust_core::api::{
    LensFlareRenderPlan, ParticleFieldRenderPlan, ProceduralObjectRenderPlan,
    RadialLinesRenderPlan, RgbaColor, TrackLineRenderPlan,
};
use std::error::Error;
use std::f32::consts::PI;
use std::fmt::{Display, Formatter};
use tiny_skia::{
    BlendMode, Color, FillRule, GradientStop, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Point,
    RadialGradient, Rect, SpreadMode, Stroke, StrokeDash, Transform,
};

const MAX_RASTER_DIMENSION: f32 = 8_192.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProceduralRasterError {
    InvalidDimensions,
    InvalidPath,
    AllocationFailed,
}

impl Display for ProceduralRasterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDimensions => {
                "procedural object dimensions are invalid or exceed 8192 pixels"
            }
            Self::InvalidPath => "procedural object path could not be constructed",
            Self::AllocationFailed => "procedural object pixel buffer could not be allocated",
        })
    }
}

impl Error for ProceduralRasterError {}

pub fn rasterize_procedural_object(
    plan: &ProceduralObjectRenderPlan,
    timestamp_seconds: f64,
) -> Result<VideoFrame, ProceduralRasterError> {
    let pixmap = match plan {
        ProceduralObjectRenderPlan::TrackLine(plan) => rasterize_track_line(plan)?,
        ProceduralObjectRenderPlan::ParticleField(plan) => rasterize_particle_field(plan)?,
        ProceduralObjectRenderPlan::RadialLines(plan) => rasterize_radial_lines(plan)?,
        ProceduralObjectRenderPlan::LensFlare(plan) => rasterize_lens_flare(plan)?,
    };
    Ok(VideoFrame {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: straight_alpha_rgba(pixmap.data()),
        timestamp_seconds,
    })
}

fn rasterize_track_line(plan: &TrackLineRenderPlan) -> Result<Pixmap, ProceduralRasterError> {
    let values = [
        plan.width,
        plan.height,
        plan.start_x,
        plan.start_y,
        plan.end_x,
        plan.end_y,
        plan.line_width,
        plan.dash_length,
        plan.dash_space,
    ];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(ProceduralRasterError::InvalidDimensions);
    }
    let padding = 24.0_f32.max(plan.line_width * 4.0);
    let mut pixmap = new_pixmap(plan.width + padding * 2.0, plan.height + padding * 2.0)?;
    let center_x = pixmap.width() as f32 / 2.0;
    let center_y = pixmap.height() as f32 / 2.0;
    let x1 = center_x + plan.start_x;
    let y1 = center_y + plan.start_y;
    let x2 = center_x + plan.end_x;
    let y2 = center_y + plan.end_y;
    let mut builder = PathBuilder::new();
    builder.move_to(x1, y1);
    builder.line_to(x2, y2);
    let path = builder.finish().ok_or(ProceduralRasterError::InvalidPath)?;
    let mut paint = color_paint(plan.color, 1.0);
    let mut stroke = Stroke {
        width: plan.line_width.max(0.1),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    if plan.dash_length > 0.0 {
        stroke.dash = StrokeDash::new(vec![plan.dash_length, plan.dash_space.max(0.0)], 0.0);
    }
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    if plan.arrow {
        let angle = (y2 - y1).atan2(x2 - x1);
        let length = 14.0_f32.max(plan.line_width * 4.0);
        let mut arrow = PathBuilder::new();
        arrow.move_to(x2, y2);
        arrow.line_to(
            x2 - (angle - PI / 7.0).cos() * length,
            y2 - (angle - PI / 7.0).sin() * length,
        );
        arrow.line_to(
            x2 - (angle + PI / 7.0).cos() * length,
            y2 - (angle + PI / 7.0).sin() * length,
        );
        arrow.close();
        let arrow = arrow.finish().ok_or(ProceduralRasterError::InvalidPath)?;
        paint.anti_alias = true;
        pixmap.fill_path(
            &arrow,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    Ok(pixmap)
}

fn rasterize_particle_field(
    plan: &ParticleFieldRenderPlan,
) -> Result<Pixmap, ProceduralRasterError> {
    let values = [
        plan.width,
        plan.height,
        plan.speed,
        plan.particle_size,
        plan.spread,
    ];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(ProceduralRasterError::InvalidDimensions);
    }
    let mut pixmap = new_pixmap(plan.width, plan.height)?;
    let width = pixmap.width() as f32;
    let height = pixmap.height() as f32;
    let time = plan.relative_frame as f64 * f64::from(plan.speed);
    for index in 0..plan.count {
        let base = index as f64 * 5.0;
        let x = qml_random(base + 1.0, plan.seed, 37.17) as f32 * width;
        let y = qml_random(base + 2.0, plan.seed, 37.17) as f32 * height;
        let radius_random = qml_random(base + 3.0, plan.seed, 37.17) as f32;
        let phase = qml_random(base + 4.0, plan.seed, 37.17);
        let base_alpha = 0.35 + qml_random(base + 5.0, plan.seed, 37.17) as f32 * 0.65;
        let size = plan.particle_size * (0.45 + radius_random * 1.2);
        let twinkle = 0.45 + (time * 0.08 + phase * std::f64::consts::TAU).sin() as f32 * 0.35;
        let radius = size * (0.65 + twinkle * 0.35);
        let alpha = (base_alpha * twinkle).max(0.1);
        let path = sparkle_path(x, y, radius).ok_or(ProceduralRasterError::InvalidPath)?;
        pixmap.fill_path(
            &path,
            &color_paint(plan.color, alpha),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    Ok(pixmap)
}

fn rasterize_radial_lines(plan: &RadialLinesRenderPlan) -> Result<Pixmap, ProceduralRasterError> {
    let values = [
        plan.width,
        plan.height,
        plan.min_length,
        plan.max_length,
        plan.thickness,
        plan.randomness,
        plan.center_x,
        plan.center_y,
        plan.spin_speed,
    ];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(ProceduralRasterError::InvalidDimensions);
    }
    let mut pixmap = new_pixmap(plan.width, plan.height)?;
    pixmap.fill(Color::BLACK);
    let center_x = pixmap.width() as f32 / 2.0 + plan.center_x;
    let center_y = pixmap.height() as f32 / 2.0 + plan.center_y;
    let rotation = (plan.relative_frame as f32 * plan.spin_speed).to_radians();
    let mut segments = Vec::with_capacity(plan.line_count as usize);
    for index in 0..plan.line_count {
        let index_f64 = f64::from(index);
        let base = index as f32 / plan.line_count as f32;
        let jitter =
            (qml_random(index_f64, plan.seed, 101.3) as f32 - 0.5) * plan.randomness * 0.055;
        let angle = (base + jitter) * std::f32::consts::TAU + rotation;
        let length = plan.min_length
            + (plan.max_length - plan.min_length)
                * qml_random(index_f64 + 900.0, plan.seed, 101.3) as f32;
        let start = 10.0
            + qml_random(index_f64 + 1_800.0, plan.seed, 101.3) as f32
                * plan.max_length
                * 0.08
                * plan.randomness;
        let width = (plan.thickness
            * (0.35 + qml_random(index_f64 + 2_700.0, plan.seed, 101.3) as f32 * 1.8))
            .max(0.1);
        let alpha = 0.16 + qml_random(index_f64 + 3_600.0, plan.seed, 101.3) as f32 * 0.58;
        let cosine = angle.cos();
        let sine = angle.sin();
        segments.push(RadialSegment {
            start: Point::from_xy(center_x + cosine * start, center_y + sine * start),
            bright: Point::from_xy(
                center_x + cosine * (start + length * 0.08),
                center_y + sine * (start + length * 0.08),
            ),
            end: Point::from_xy(center_x + cosine * length, center_y + sine * length),
            width,
            alpha,
        });
    }
    for layer in 0..3 {
        for segment in &segments {
            let (start, alpha, width) = match layer {
                0 => (segment.start, segment.alpha * 0.16, segment.width * 4.2),
                1 => (segment.start, segment.alpha * 0.34, segment.width * 1.8),
                _ => (
                    segment.bright,
                    segment.alpha,
                    (segment.width * 0.42).max(0.6),
                ),
            };
            stroke_segment(&mut pixmap, start, segment.end, plan.color, alpha, width)?;
        }
    }
    let mask_radius = 24.0_f32.max(plan.thickness * 9.0);
    if mask_radius > 0.0 {
        let stops = vec![
            GradientStop::new(0.0, Color::from_rgba8(0, 0, 0, 242)),
            GradientStop::new(1.0, Color::TRANSPARENT),
        ];
        if let Some(shader) = RadialGradient::new(
            Point::from_xy(center_x, center_y),
            0.0,
            Point::from_xy(center_x, center_y),
            mask_radius,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ) && let Some(path) = PathBuilder::from_circle(center_x, center_y, mask_radius)
        {
            let paint = Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            };
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    Ok(pixmap)
}

fn rasterize_lens_flare(plan: &LensFlareRenderPlan) -> Result<Pixmap, ProceduralRasterError> {
    let values = [
        plan.width,
        plan.height,
        plan.center_x,
        plan.center_y,
        plan.radius,
        plan.strength,
    ];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(ProceduralRasterError::InvalidDimensions);
    }
    let mut pixmap = new_pixmap(plan.width, plan.height)?;
    if plan.radius <= 0.0 {
        return Ok(pixmap);
    }
    let width = pixmap.width() as f32;
    let height = pixmap.height() as f32;
    let center_x = width / 2.0 + plan.center_x;
    let center_y = height / 2.0 + plan.center_y;
    fill_additive_radial(
        &mut pixmap,
        center_x,
        center_y,
        plan.radius,
        plan.color,
        [
            (0.0, plan.strength.min(1.0)),
            (0.35, (plan.strength * 0.35).min(0.35)),
            (1.0, 0.0),
        ],
    );

    let mut builder = PathBuilder::new();
    builder.move_to(center_x - plan.radius * 1.3, center_y);
    builder.line_to(center_x + plan.radius * 1.3, center_y);
    builder.move_to(center_x, center_y - plan.radius * 1.3);
    builder.line_to(center_x, center_y + plan.radius * 1.3);
    let path = builder.finish().ok_or(ProceduralRasterError::InvalidPath)?;
    let mut paint = lens_color_paint(plan.color, (plan.strength * 0.55).min(0.75));
    paint.blend_mode = BlendMode::Plus;
    let stroke = Stroke {
        width: (plan.radius * 0.012).max(1.0),
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    for index in 0..plan.ghosts {
        let fraction = (index + 1) as f32 / (plan.ghosts + 1) as f32;
        let ghost_x = width / 2.0 + (width / 2.0 - center_x) * (fraction * 1.4 - 0.25);
        let ghost_y = height / 2.0 + (height / 2.0 - center_y) * (fraction * 1.4 - 0.25);
        let ghost_radius = plan.radius * (0.08 + 0.08 * (index % 3) as f32);
        fill_additive_radial(
            &mut pixmap,
            ghost_x,
            ghost_y,
            ghost_radius,
            plan.color,
            [(0.0, (plan.strength * 0.35).min(0.45)), (1.0, 0.0)],
        );
    }
    Ok(pixmap)
}

struct RadialSegment {
    start: Point,
    bright: Point,
    end: Point,
    width: f32,
    alpha: f32,
}

fn new_pixmap(width: f32, height: f32) -> Result<Pixmap, ProceduralRasterError> {
    if !width.is_finite() || !height.is_finite() {
        return Err(ProceduralRasterError::InvalidDimensions);
    }
    let width = width.ceil().max(1.0);
    let height = height.ceil().max(1.0);
    if width > MAX_RASTER_DIMENSION || height > MAX_RASTER_DIMENSION {
        return Err(ProceduralRasterError::InvalidDimensions);
    }
    Pixmap::new(width as u32, height as u32).ok_or(ProceduralRasterError::AllocationFailed)
}

fn qml_random(index: f64, seed: i32, seed_scale: f64) -> f64 {
    let value = ((index + f64::from(seed) * seed_scale) * 12.9898).sin() * 43_758.5;
    value - value.floor()
}

fn sparkle_path(x: f32, y: f32, radius: f32) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    builder.move_to(x, y - radius);
    builder.line_to(x + radius * 0.26, y - radius * 0.26);
    builder.line_to(x + radius, y);
    builder.line_to(x + radius * 0.26, y + radius * 0.26);
    builder.line_to(x, y + radius);
    builder.line_to(x - radius * 0.26, y + radius * 0.26);
    builder.line_to(x - radius, y);
    builder.line_to(x - radius * 0.26, y - radius * 0.26);
    builder.close();
    builder.finish()
}

fn stroke_segment(
    pixmap: &mut Pixmap,
    start: Point,
    end: Point,
    color: RgbaColor,
    alpha: f32,
    width: f32,
) -> Result<(), ProceduralRasterError> {
    let mut builder = PathBuilder::new();
    builder.move_to(start.x, start.y);
    builder.line_to(end.x, end.y);
    let path = builder.finish().ok_or(ProceduralRasterError::InvalidPath)?;
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(
        &path,
        &color_paint(color, alpha),
        &stroke,
        Transform::identity(),
        None,
    );
    Ok(())
}

fn fill_additive_radial<const N: usize>(
    pixmap: &mut Pixmap,
    center_x: f32,
    center_y: f32,
    radius: f32,
    color: RgbaColor,
    stops: [(f32, f32); N],
) {
    if radius <= 0.0 {
        return;
    }
    let stops = stops
        .into_iter()
        .map(|(position, alpha)| GradientStop::new(position, lens_color_with_alpha(color, alpha)))
        .collect();
    let Some(shader) = RadialGradient::new(
        Point::from_xy(center_x, center_y),
        0.0,
        Point::from_xy(center_x, center_y),
        radius,
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    ) else {
        return;
    };
    let Some(rect) = Rect::from_xywh(
        center_x - radius,
        center_y - radius,
        radius * 2.0,
        radius * 2.0,
    ) else {
        return;
    };
    let paint = Paint {
        shader,
        blend_mode: BlendMode::Plus,
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
}

fn color_paint(color: RgbaColor, alpha: f32) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(color_with_alpha(color, alpha));
    paint
}

fn lens_color_paint(color: RgbaColor, alpha: f32) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(lens_color_with_alpha(color, alpha));
    paint
}

fn color_with_alpha(color: RgbaColor, alpha: f32) -> Color {
    let combined = (f32::from(color.alpha) * alpha.clamp(0.0, 1.0)).round() as u8;
    Color::from_rgba8(color.red, color.green, color.blue, combined)
}

fn lens_color_with_alpha(color: RgbaColor, alpha: f32) -> Color {
    Color::from_rgba8(
        color.red,
        color.green,
        color.blue,
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn straight_alpha_rgba(premultiplied: &[u8]) -> Vec<u8> {
    let mut straight = premultiplied.to_vec();
    for pixel in straight.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    straight
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color() -> RgbaColor {
        RgbaColor {
            red: 255,
            green: 220,
            blue: 160,
            alpha: 255,
        }
    }

    #[test]
    fn track_line_uses_qml_padding_and_draws_an_arrow() {
        let frame = rasterize_procedural_object(
            &ProceduralObjectRenderPlan::TrackLine(TrackLineRenderPlan {
                width: 80.0,
                height: 60.0,
                start_x: -20.0,
                start_y: 10.0,
                end_x: 20.0,
                end_y: -10.0,
                line_width: 8.0,
                dash_length: 0.0,
                dash_space: 12.0,
                arrow: true,
                color: color(),
                opacity: 1.0,
            }),
            0.0,
        )
        .expect("track line rasterizes");
        assert_eq!((frame.width, frame.height), (144, 124));
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
    }

    #[test]
    fn particle_field_is_deterministic_and_changes_with_time() {
        let mut plan = ParticleFieldRenderPlan {
            width: 96.0,
            height: 54.0,
            count: 20,
            speed: 1.0,
            particle_size: 3.0,
            spread: 1.0,
            seed: 7,
            color: color(),
            opacity: 1.0,
            relative_frame: 0,
        };
        let first = rasterize_procedural_object(
            &ProceduralObjectRenderPlan::ParticleField(plan.clone()),
            0.0,
        )
        .expect("particle field rasterizes");
        let repeated = rasterize_procedural_object(
            &ProceduralObjectRenderPlan::ParticleField(plan.clone()),
            0.0,
        )
        .expect("particle field repeats");
        assert_eq!(first.rgba, repeated.rgba);
        plan.relative_frame = 10;
        let advanced =
            rasterize_procedural_object(&ProceduralObjectRenderPlan::ParticleField(plan), 1.0)
                .expect("advanced particle field rasterizes");
        assert_ne!(first.rgba, advanced.rgba);
    }

    #[test]
    fn radial_lines_keep_the_qml_opaque_black_canvas() {
        let frame = rasterize_procedural_object(
            &ProceduralObjectRenderPlan::RadialLines(RadialLinesRenderPlan {
                width: 64.0,
                height: 64.0,
                line_count: 16,
                min_length: 10.0,
                max_length: 40.0,
                thickness: 2.0,
                randomness: 0.75,
                center_x: 0.0,
                center_y: 0.0,
                spin_speed: 1.0,
                seed: 3,
                color: color(),
                opacity: 1.0,
                relative_frame: 2,
            }),
            0.0,
        )
        .expect("radial lines rasterize");
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > 0)
        );
    }

    #[test]
    fn lens_flare_rasterizes_main_glow_and_ghosts() {
        let frame = rasterize_procedural_object(
            &ProceduralObjectRenderPlan::LensFlare(LensFlareRenderPlan {
                width: 96.0,
                height: 54.0,
                center_x: 15.0,
                center_y: -5.0,
                radius: 18.0,
                strength: 1.0,
                ghosts: 4,
                color: color(),
                opacity: 1.0,
            }),
            0.0,
        )
        .expect("lens flare rasterizes");
        assert_eq!((frame.width, frame.height), (96, 54));
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
    }

    #[test]
    fn rejects_unbounded_procedural_allocations() {
        let result = rasterize_procedural_object(
            &ProceduralObjectRenderPlan::LensFlare(LensFlareRenderPlan {
                width: 9_000.0,
                height: 10.0,
                center_x: 0.0,
                center_y: 0.0,
                radius: 1.0,
                strength: 1.0,
                ghosts: 0,
                color: color(),
                opacity: 1.0,
            }),
            0.0,
        );
        assert_eq!(result, Err(ProceduralRasterError::InvalidDimensions));
    }
}
