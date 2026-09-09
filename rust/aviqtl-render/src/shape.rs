use aviqtl_media::VideoFrame;
use aviqtl_rust_core::api::{RgbaColor, ShapeGradientKind, ShapeKind, ShapeRenderPlan};
use std::error::Error;
use std::f32::consts::PI;
use std::fmt::{Display, Formatter};
use tiny_skia::{
    Color, FillRule, GradientStop, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pixmap,
    Point, RadialGradient, SpreadMode, Stroke, StrokeDash, Transform,
};

const MAX_RASTER_DIMENSION: f32 = 8_192.0;

/// Failure to create a bounded RGBA frame for a procedural shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeRasterError {
    InvalidDimensions,
    InvalidPath,
    AllocationFailed,
}

impl Display for ShapeRasterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDimensions => "shape dimensions are invalid or exceed 8192 pixels",
            Self::InvalidPath => "shape path could not be constructed",
            Self::AllocationFailed => "shape pixel buffer could not be allocated",
        })
    }
}

impl Error for ShapeRasterError {}

/// Rasterizes one evaluated built-in shape into straight-alpha RGBA pixels.
pub fn rasterize_shape(
    plan: &ShapeRenderPlan,
    timestamp_seconds: f64,
) -> Result<VideoFrame, ShapeRasterError> {
    if !plan.width.is_finite()
        || !plan.height.is_finite()
        || !plan.stroke_width.is_finite()
        || plan.width < 0.0
        || plan.height < 0.0
        || plan.stroke_width < 0.0
    {
        return Err(ShapeRasterError::InvalidDimensions);
    }
    let padding = plan.stroke_width / 2.0 + plan.edge_padding;
    let raster_width = (plan.width + padding * 2.0).ceil().max(1.0);
    let raster_height = (plan.height + padding * 2.0).ceil().max(1.0);
    if raster_width > MAX_RASTER_DIMENSION || raster_height > MAX_RASTER_DIMENSION {
        return Err(ShapeRasterError::InvalidDimensions);
    }
    let width = raster_width as u32;
    let height = raster_height as u32;
    let mut pixmap = Pixmap::new(width, height).ok_or(ShapeRasterError::AllocationFailed)?;
    if plan.width > 0.0 && plan.height > 0.0 {
        let center = Point::from_xy(raster_width / 2.0, raster_height / 2.0);
        let path = build_path(plan, center).ok_or(ShapeRasterError::InvalidPath)?;
        if plan.kind != ShapeKind::Arc {
            let fill = fill_paint(plan, center);
            pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
        }
        if plan.stroke_width > 0.0 {
            let mut paint = Paint {
                anti_alias: true,
                ..Paint::default()
            };
            paint.set_color(to_skia_color(plan.stroke_color));
            let mut stroke = Stroke {
                width: plan.stroke_width,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            if plan.dash_length > 0.0 || plan.dash_space > 0.0 {
                stroke.dash = StrokeDash::new(
                    vec![plan.dash_length.max(1.0), plan.dash_space.max(0.0)],
                    0.0,
                );
            }
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
    Ok(VideoFrame {
        width,
        height,
        rgba: straight_alpha_rgba(pixmap.data()),
        timestamp_seconds,
    })
}

fn fill_paint(plan: &ShapeRenderPlan, center: Point) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    if !plan.use_gradient {
        paint.set_color(to_skia_color(plan.fill_color));
        return paint;
    }
    let stops = vec![
        GradientStop::new(0.0, to_skia_color(plan.fill_color)),
        GradientStop::new(1.0, to_skia_color(plan.gradient_color)),
    ];
    paint.shader = match plan.gradient_kind {
        ShapeGradientKind::Linear => LinearGradient::new(
            Point::from_xy(center.x, center.y - plan.height / 2.0),
            Point::from_xy(center.x, center.y + plan.height / 2.0),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ),
        ShapeGradientKind::Radial => RadialGradient::new(
            center,
            0.0,
            center,
            plan.width.max(plan.height) / 2.0,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ),
    }
    .unwrap_or_else(|| tiny_skia::Shader::SolidColor(to_skia_color(plan.fill_color)));
    paint
}

fn build_path(plan: &ShapeRenderPlan, center: Point) -> Option<Path> {
    match plan.kind {
        ShapeKind::Polygon | ShapeKind::Star => polygon_path(plan, center),
        ShapeKind::Pie | ShapeKind::Arc | ShapeKind::Donut => arc_path(plan, center),
    }
}

fn polygon_path(plan: &ShapeRenderPlan, center: Point) -> Option<Path> {
    let sides = plan.sides.clamp(3, 64) as usize;
    let count = if plan.kind == ShapeKind::Star {
        sides * 2
    } else {
        sides
    };
    let base_rotation = -PI / 2.0
        - if plan.even_sides_half_step && sides.is_multiple_of(2) {
            PI / sides as f32
        } else {
            0.0
        };
    let rotation = plan.rotation_degrees.to_radians();
    let (sine, cosine) = rotation.sin_cos();
    let mut raw = Vec::with_capacity(count);
    for index in 0..count {
        let angle = if plan.kind == ShapeKind::Star {
            base_rotation + index as f32 * PI / sides as f32
        } else {
            base_rotation + index as f32 * 2.0 * PI / sides as f32
        };
        let radius = if plan.kind == ShapeKind::Star && index % 2 == 1 {
            plan.inner_radius_percent / 100.0
        } else {
            1.0
        };
        let x = angle.cos() * radius;
        let y = angle.sin() * radius;
        raw.push(Point::from_xy(x * cosine - y * sine, x * sine + y * cosine));
    }
    let vertices = if plan.normalize_vertices {
        let min_x = raw
            .iter()
            .map(|point| point.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = raw
            .iter()
            .map(|point| point.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = raw
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = raw
            .iter()
            .map(|point| point.y)
            .fold(f32::NEG_INFINITY, f32::max);
        let span_x = (max_x - min_x).max(f32::EPSILON);
        let span_y = (max_y - min_y).max(f32::EPSILON);
        raw.into_iter()
            .map(|point| {
                Point::from_xy(
                    center.x + ((point.x - min_x) / span_x - 0.5) * plan.width,
                    center.y + ((point.y - min_y) / span_y - 0.5) * plan.height,
                )
            })
            .collect::<Vec<_>>()
    } else {
        raw.into_iter()
            .map(|point| {
                Point::from_xy(
                    center.x + point.x * plan.width / 2.0,
                    center.y + point.y * plan.height / 2.0,
                )
            })
            .collect::<Vec<_>>()
    };
    closed_polygon_path(
        &vertices,
        if plan.kind == ShapeKind::Star {
            0.0
        } else {
            plan.corner_radius
        },
    )
}

fn closed_polygon_path(vertices: &[Point], corner_radius: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    if corner_radius < 0.5 {
        let first = vertices.first()?;
        builder.move_to(first.x, first.y);
        for vertex in &vertices[1..] {
            builder.line_to(vertex.x, vertex.y);
        }
        builder.close();
        return builder.finish();
    }
    for index in 0..vertices.len() {
        let previous = vertices[(index + vertices.len() - 1) % vertices.len()];
        let current = vertices[index];
        let next = vertices[(index + 1) % vertices.len()];
        let incoming = current - previous;
        let outgoing = next - current;
        let radius = corner_radius
            .min(incoming.length() / 2.0)
            .min(outgoing.length() / 2.0);
        let incoming_length = incoming.length().max(f32::EPSILON);
        let outgoing_length = outgoing.length().max(f32::EPSILON);
        let before = Point::from_xy(
            current.x - incoming.x / incoming_length * radius,
            current.y - incoming.y / incoming_length * radius,
        );
        let after = Point::from_xy(
            current.x + outgoing.x / outgoing_length * radius,
            current.y + outgoing.y / outgoing_length * radius,
        );
        if index == 0 {
            builder.move_to(before.x, before.y);
        } else {
            builder.line_to(before.x, before.y);
        }
        builder.quad_to(current.x, current.y, after.x, after.y);
    }
    builder.close();
    builder.finish()
}

fn arc_path(plan: &ShapeRenderPlan, center: Point) -> Option<Path> {
    let arc_degrees = plan.sweep_degrees.clamp(1.0, 360.0);
    let steps = 32_u32.max((arc_degrees / 3.0).ceil() as u32);
    let start = (plan.rotation_degrees - 90.0).to_radians();
    let radius_x = plan.width / 2.0;
    let radius_y = plan.height / 2.0;
    let point = |index: u32, scale: f32| {
        let angle = start + arc_degrees.to_radians() * index as f32 / steps as f32;
        Point::from_xy(
            center.x + angle.cos() * radius_x * scale,
            center.y + angle.sin() * radius_y * scale,
        )
    };
    let mut builder = PathBuilder::new();
    match plan.kind {
        ShapeKind::Pie => {
            builder.move_to(center.x, center.y);
            for index in 0..=steps {
                let point = point(index, 1.0);
                builder.line_to(point.x, point.y);
            }
            builder.close();
        }
        ShapeKind::Arc => {
            let first = point(0, 1.0);
            builder.move_to(first.x, first.y);
            for index in 1..=steps {
                let point = point(index, 1.0);
                builder.line_to(point.x, point.y);
            }
        }
        ShapeKind::Donut => {
            let first = point(0, 1.0);
            builder.move_to(first.x, first.y);
            for index in 1..=steps {
                let point = point(index, 1.0);
                builder.line_to(point.x, point.y);
            }
            let inner_scale = (plan.inner_radius_percent / 100.0).max(f32::EPSILON);
            for index in (0..=steps).rev() {
                let point = point(index, inner_scale);
                builder.line_to(point.x, point.y);
            }
            builder.close();
        }
        ShapeKind::Polygon | ShapeKind::Star => unreachable!("dispatched by build_path"),
    }
    builder.finish()
}

fn to_skia_color(color: RgbaColor) -> Color {
    Color::from_rgba8(color.red, color.green, color.blue, color.alpha)
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

    fn plan(kind: ShapeKind) -> ShapeRenderPlan {
        ShapeRenderPlan {
            kind,
            width: 80.0,
            height: 60.0,
            sides: 5,
            sweep_degrees: 5.0,
            corner_radius: 8.0,
            inner_radius_percent: 45.0,
            rotation_degrees: 0.0,
            normalize_vertices: true,
            even_sides_half_step: true,
            edge_padding: 2.0,
            fill_color: RgbaColor {
                red: 200,
                green: 80,
                blue: 40,
                alpha: 128,
            },
            use_gradient: false,
            gradient_color: RgbaColor {
                red: 20,
                green: 40,
                blue: 220,
                alpha: 255,
            },
            gradient_kind: ShapeGradientKind::Linear,
            stroke_color: RgbaColor {
                red: 255,
                green: 255,
                blue: 255,
                alpha: 255,
            },
            stroke_width: 4.0,
            dash_length: 5.0,
            dash_space: 3.0,
            opacity: 1.0,
        }
    }

    #[test]
    fn rasterizes_antialiased_straight_alpha_polygon() {
        let frame = rasterize_shape(&plan(ShapeKind::Polygon), 1.25).expect("shape rasterizes");
        assert_eq!((frame.width, frame.height), (88, 68));
        assert_eq!(frame.timestamp_seconds, 1.25);
        assert_eq!(frame.rgba.len(), 88 * 68 * 4);
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 128)
        );
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 128 && pixel[0] >= 198)
        );
    }

    #[test]
    fn donut_keeps_its_center_transparent() {
        let mut donut = plan(ShapeKind::Donut);
        donut.sides = 300;
        donut.sweep_degrees = 300.0;
        donut.stroke_width = 0.0;
        let frame = rasterize_shape(&donut, 0.0).expect("donut rasterizes");
        let center = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
        assert_eq!(frame.rgba[center + 3], 0);
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
    fn custom_shape_padding_and_fractional_sweep_match_qml_inputs() {
        let mut pie = plan(ShapeKind::Pie);
        pie.width = 80.0;
        pie.height = 60.0;
        pie.stroke_width = 4.0;
        pie.edge_padding = 4.0;
        pie.sweep_degrees = 123.5;
        let frame = rasterize_shape(&pie, 0.0).expect("custom pie rasterizes");
        assert_eq!((frame.width, frame.height), (92, 72));
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
    fn rejects_unbounded_shape_allocations() {
        let mut huge = plan(ShapeKind::Polygon);
        huge.width = 9_000.0;
        assert_eq!(
            rasterize_shape(&huge, 0.0),
            Err(ShapeRasterError::InvalidDimensions)
        );
    }
}
