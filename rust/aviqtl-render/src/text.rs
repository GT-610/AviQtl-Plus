use aviqtl_media::VideoFrame;
use aviqtl_rust_core::api::{RgbaColor, TextAlignment, TextRenderPlan};
use fontdb::{Database, Family, ID, Query, Stretch, Style, Weight};
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};
use rustybuzz::{Face, UnicodeBuffer};
use std::error::Error;
use std::fmt::{Display, Formatter};
use tiny_skia::{
    Color, FillRule, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

const MAX_RASTER_DIMENSION: f32 = 8_192.0;
const MAX_TEXT_BYTES: usize = 1_000_000;
const FALLBACK_FAMILIES: &[&str] = &[
    "Hiragino Sans",
    "Noto Sans CJK JP",
    "Noto Sans",
    "Yu Gothic",
    "Meiryo",
    "Microsoft YaHei",
    "Arial Unicode MS",
];

/// Failure to turn an evaluated text object into a bounded RGBA frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRasterError {
    InvalidParameters,
    FontNotFound,
    InvalidFont,
    InvalidDimensions,
    InvalidPath,
    AllocationFailed,
}

impl Display for TextRasterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidParameters => "text parameters are invalid",
            Self::FontNotFound => "no usable system font was found",
            Self::InvalidFont => "the selected font could not be parsed",
            Self::InvalidDimensions => "text dimensions are invalid or exceed 8192 pixels",
            Self::InvalidPath => "text glyph outlines could not be constructed",
            Self::AllocationFailed => "text pixel buffer could not be allocated",
        })
    }
}

impl Error for TextRasterError {}

/// Owns the platform font catalog used by the Rust preview worker.
pub struct TextRasterizer {
    database: Database,
}

impl TextRasterizer {
    /// Loads the platform font directories once for subsequent text frames.
    ///
    /// No `Default` impl by design: construction scans the system font
    /// catalog, so call sites spell out `new()`.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();
        Self { database }
    }

    /// Rasterizes one evaluated built-in text object into straight-alpha RGBA pixels.
    pub fn rasterize(
        &self,
        plan: &TextRenderPlan,
        timestamp_seconds: f64,
    ) -> Result<VideoFrame, TextRasterError> {
        validate_plan(plan)?;
        let face_id = self
            .select_face(plan)
            .ok_or(TextRasterError::FontNotFound)?;
        self.database
            .with_face_data(face_id, |data, face_index| {
                rasterize_with_face(plan, timestamp_seconds, data, face_index)
            })
            .ok_or(TextRasterError::InvalidFont)?
    }

    fn select_face(&self, plan: &TextRenderPlan) -> Option<ID> {
        let weight = if plan.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        };
        let style = if plan.italic {
            Style::Italic
        } else {
            Style::Normal
        };
        let requested = requested_family(plan.font_family.trim());
        let mut families = vec![requested];
        if requested != Family::SansSerif {
            families.push(Family::SansSerif);
        }
        let query = Query {
            families: &families,
            weight,
            stretch: Stretch::Normal,
            style,
        };
        let primary = self.database.query(&query);
        if primary.is_some_and(|id| self.face_supports_text(id, &plan.content)) {
            return primary;
        }

        for family in FALLBACK_FAMILIES {
            let named = [Family::Name(family)];
            let query = Query {
                families: &named,
                weight,
                stretch: Stretch::Normal,
                style,
            };
            if let Some(id) = self.database.query(&query)
                && self.face_supports_text(id, &plan.content)
            {
                return Some(id);
            }
        }

        let mut candidates = self
            .database
            .faces()
            .map(|face| face.id)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            let left = self
                .database
                .face(*left)
                .expect("font ID came from this database");
            let right = self
                .database
                .face(*right)
                .expect("font ID came from this database");
            face_match_score(left.style, left.weight, style, weight)
                .cmp(&face_match_score(right.style, right.weight, style, weight))
                .then_with(|| left.post_script_name.cmp(&right.post_script_name))
        });
        candidates
            .into_iter()
            .find(|id| self.face_supports_text(*id, &plan.content))
            .or(primary)
    }

    fn face_supports_text(&self, id: ID, text: &str) -> bool {
        self.database
            .with_face_data(id, |data, face_index| {
                Face::from_slice(data, face_index).is_some_and(|face| {
                    text.chars().all(|character| {
                        ignorable_for_font_matching(character)
                            || face.glyph_index(character).is_some()
                    })
                })
            })
            .unwrap_or(false)
    }
}

fn requested_family(name: &str) -> Family<'_> {
    if name.eq_ignore_ascii_case("serif") {
        Family::Serif
    } else if name.eq_ignore_ascii_case("sans-serif") || name.is_empty() {
        Family::SansSerif
    } else if name.eq_ignore_ascii_case("cursive") {
        Family::Cursive
    } else if name.eq_ignore_ascii_case("fantasy") {
        Family::Fantasy
    } else if name.eq_ignore_ascii_case("monospace") {
        Family::Monospace
    } else {
        Family::Name(name)
    }
}

fn face_match_score(
    actual_style: Style,
    actual_weight: Weight,
    requested_style: Style,
    requested_weight: Weight,
) -> (u16, u16) {
    let style_penalty = if actual_style == requested_style {
        0
    } else {
        1
    };
    (style_penalty, actual_weight.0.abs_diff(requested_weight.0))
}

fn ignorable_for_font_matching(character: char) -> bool {
    character.is_whitespace()
        || character.is_control()
        || character == '\u{200d}'
        || ('\u{fe00}'..='\u{fe0f}').contains(&character)
        || ('\u{e0100}'..='\u{e01ef}').contains(&character)
}

fn validate_plan(plan: &TextRenderPlan) -> Result<(), TextRasterError> {
    let finite = [
        plan.font_size,
        plan.letter_spacing,
        plan.line_spacing,
        plan.outline_width,
        plan.shadow_offset_x,
        plan.shadow_offset_y,
        plan.background_radius,
        plan.background_padding_x,
        plan.background_padding_y,
    ]
    .into_iter()
    .all(f32::is_finite);
    if !finite
        || plan.content.len() > MAX_TEXT_BYTES
        || plan.font_size <= 0.0
        || plan.line_spacing < 0.0
        || plan.outline_width < 0.0
        || plan.background_radius < 0.0
        || plan.background_padding_x < 0.0
        || plan.background_padding_y < 0.0
    {
        return Err(TextRasterError::InvalidParameters);
    }
    Ok(())
}

#[derive(Debug)]
struct GlyphPlacement {
    glyph_id: u16,
    x: f32,
    y_offset: f32,
}

#[derive(Debug)]
struct LineLayout {
    glyphs: Vec<GlyphPlacement>,
    width: f32,
}

fn rasterize_with_face(
    plan: &TextRenderPlan,
    timestamp_seconds: f64,
    data: &[u8],
    face_index: u32,
) -> Result<VideoFrame, TextRasterError> {
    let face = Face::from_slice(data, face_index).ok_or(TextRasterError::InvalidFont)?;
    let units_per_em = face.units_per_em().max(1) as f32;
    let scale = plan.font_size / units_per_em;
    let ascent = (f32::from(face.ascender()) * scale).max(plan.font_size * 0.5);
    let descent = (-f32::from(face.descender()) * scale).max(plan.font_size * 0.1);
    let glyph_line_height = (ascent + descent).max(plan.font_size);
    let natural_line_advance =
        (glyph_line_height + f32::from(face.line_gap()) * scale).max(glyph_line_height);
    let line_advance = if plan.line_spacing > 0.0 {
        plan.line_spacing
    } else {
        natural_line_advance
    };
    let lines = plan
        .content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .map(|line| shape_line(&face, line, scale, plan.letter_spacing))
        .collect::<Vec<_>>();
    let logical_width = lines.iter().map(|line| line.width).fold(0.0_f32, f32::max);
    let logical_height = glyph_line_height + line_advance * (lines.len().saturating_sub(1) as f32);
    if !logical_width.is_finite() || !logical_height.is_finite() {
        return Err(TextRasterError::InvalidDimensions);
    }

    let mut path_builder = PathBuilder::new();
    let mut outlined_glyphs = 0_usize;
    for (line_index, line) in lines.iter().enumerate() {
        let x = alignment_offset(plan.alignment, logical_width, line.width);
        let baseline = ascent + line_index as f32 * line_advance;
        for glyph in &line.glyphs {
            let mut outline = GlyphOutline {
                path: &mut path_builder,
                origin_x: x + glyph.x,
                baseline_y: baseline - glyph.y_offset,
                scale,
            };
            if face
                .outline_glyph(GlyphId(glyph.glyph_id), &mut outline)
                .is_some()
            {
                outlined_glyphs += 1;
            }
        }
    }
    let has_visible_text = plan
        .content
        .chars()
        .any(|character| !ignorable_for_font_matching(character));
    if has_visible_text && outlined_glyphs == 0 {
        return Err(TextRasterError::InvalidPath);
    }
    let path = path_builder.finish();
    let (visual_left, visual_top, visual_right, visual_bottom) =
        path.as_ref()
            .map_or((0.0, 0.0, logical_width, logical_height), |path| {
                let bounds = path.bounds();
                (
                    bounds.left().min(0.0),
                    bounds.top().min(0.0),
                    bounds.right().max(logical_width),
                    bounds.bottom().max(logical_height),
                )
            });
    let visual_width = (visual_right - visual_left).max(0.0);
    let visual_height = (visual_bottom - visual_top).max(0.0);
    let outline_padding = if plan.outline_enabled {
        plan.outline_width.ceil() + 2.0
    } else {
        2.0
    };
    let background_padding_x = if plan.background_enabled {
        plan.background_padding_x
    } else {
        0.0
    };
    let background_padding_y = if plan.background_enabled {
        plan.background_padding_y
    } else {
        0.0
    };
    let base_width = visual_width + (outline_padding + background_padding_x) * 2.0;
    let base_height = visual_height + (outline_padding + background_padding_y) * 2.0;
    let shadow_left = if plan.shadow_enabled {
        (-plan.shadow_offset_x).max(0.0)
    } else {
        0.0
    };
    let shadow_right = if plan.shadow_enabled {
        plan.shadow_offset_x.max(0.0)
    } else {
        0.0
    };
    let shadow_top = if plan.shadow_enabled {
        (-plan.shadow_offset_y).max(0.0)
    } else {
        0.0
    };
    let shadow_bottom = if plan.shadow_enabled {
        plan.shadow_offset_y.max(0.0)
    } else {
        0.0
    };
    let raster_width = (base_width + shadow_left + shadow_right).ceil().max(1.0);
    let raster_height = (base_height + shadow_top + shadow_bottom).ceil().max(1.0);
    if !raster_width.is_finite()
        || !raster_height.is_finite()
        || raster_width > MAX_RASTER_DIMENSION
        || raster_height > MAX_RASTER_DIMENSION
    {
        return Err(TextRasterError::InvalidDimensions);
    }

    let width = raster_width as u32;
    let height = raster_height as u32;
    let mut pixmap = Pixmap::new(width, height).ok_or(TextRasterError::AllocationFailed)?;
    if plan.background_enabled {
        let background = rounded_rect_path(
            shadow_left,
            shadow_top,
            base_width,
            base_height,
            plan.background_radius,
        )
        .ok_or(TextRasterError::InvalidPath)?;
        fill_path(
            &mut pixmap,
            &background,
            plan.background_color,
            Transform::identity(),
        );
    }

    if let Some(path) = path.as_ref() {
        let translate_x = shadow_left + outline_padding + background_padding_x - visual_left;
        let translate_y = shadow_top + outline_padding + background_padding_y - visual_top;
        if plan.shadow_enabled {
            let transform = Transform::from_translate(
                translate_x + plan.shadow_offset_x,
                translate_y + plan.shadow_offset_y,
            );
            if plan.outline_enabled && plan.outline_width > 0.0 {
                stroke_path(
                    &mut pixmap,
                    path,
                    plan.shadow_color,
                    plan.outline_width * 2.0,
                    transform,
                );
            }
            fill_path(&mut pixmap, path, plan.shadow_color, transform);
        }
        let transform = Transform::from_translate(translate_x, translate_y);
        if plan.outline_enabled && plan.outline_width > 0.0 {
            stroke_path(
                &mut pixmap,
                path,
                plan.outline_color,
                plan.outline_width * 2.0,
                transform,
            );
        }
        fill_path(&mut pixmap, path, plan.color, transform);
    }

    Ok(VideoFrame {
        width,
        height,
        rgba: straight_alpha_rgba(pixmap.data()),
        timestamp_seconds,
    })
}

fn shape_line(face: &Face<'_>, text: &str, scale: f32, letter_spacing: f32) -> LineLayout {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let glyphs = rustybuzz::shape(face, &[], buffer);
    let infos = glyphs.glyph_infos();
    let positions = glyphs.glyph_positions();
    let total_advance = positions
        .iter()
        .map(|position| position.x_advance as f32 * scale)
        .sum::<f32>();
    let spacing_direction = if total_advance < 0.0 { -1.0 } else { 1.0 };
    let mut pen_x = 0.0_f32;
    let mut minimum = 0.0_f32;
    let mut maximum = 0.0_f32;
    let mut placements = Vec::with_capacity(infos.len());
    for (index, (info, position)) in infos.iter().zip(positions).enumerate() {
        placements.push(GlyphPlacement {
            glyph_id: info.glyph_id as u16,
            x: pen_x + position.x_offset as f32 * scale,
            y_offset: position.y_offset as f32 * scale,
        });
        pen_x += position.x_advance as f32 * scale;
        if index + 1 < infos.len() {
            pen_x += letter_spacing * spacing_direction;
        }
        minimum = minimum.min(pen_x);
        maximum = maximum.max(pen_x);
    }
    for glyph in &mut placements {
        glyph.x -= minimum;
    }
    LineLayout {
        glyphs: placements,
        width: (maximum - minimum).max(0.0),
    }
}

fn alignment_offset(alignment: TextAlignment, width: f32, line_width: f32) -> f32 {
    match alignment {
        TextAlignment::Left | TextAlignment::Justify => 0.0,
        TextAlignment::Right => (width - line_width).max(0.0),
        TextAlignment::Center => ((width - line_width) / 2.0).max(0.0),
    }
}

struct GlyphOutline<'a> {
    path: &'a mut PathBuilder,
    origin_x: f32,
    baseline_y: f32,
    scale: f32,
}

impl GlyphOutline<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.origin_x + x * self.scale,
            self.baseline_y - y * self.scale,
        )
    }
}

impl OutlineBuilder for GlyphOutline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.path.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x, y) = self.point(x, y);
        self.path.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.point(x1, y1);
        let (x2, y2) = self.point(x2, y2);
        let (x, y) = self.point(x, y);
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

fn rounded_rect_path(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    let rect = Rect::from_xywh(x, y, width, height)?;
    let radius = radius.min(width / 2.0).min(height / 2.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let right = x + width;
    let bottom = y + height;
    let mut builder = PathBuilder::new();
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.quad_to(right, y, right, y + radius);
    builder.line_to(right, bottom - radius);
    builder.quad_to(right, bottom, right - radius, bottom);
    builder.line_to(x + radius, bottom);
    builder.quad_to(x, bottom, x, bottom - radius);
    builder.line_to(x, y + radius);
    builder.quad_to(x, y, x + radius, y);
    builder.close();
    builder.finish()
}

fn fill_path(pixmap: &mut Pixmap, path: &Path, color: RgbaColor, transform: Transform) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(to_skia_color(color));
    pixmap.fill_path(path, &paint, FillRule::Winding, transform, None);
}

fn stroke_path(
    pixmap: &mut Pixmap,
    path: &Path,
    color: RgbaColor,
    width: f32,
    transform: Transform,
) {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(to_skia_color(color));
    let stroke = Stroke {
        width,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, transform, None);
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

    fn plan() -> TextRenderPlan {
        TextRenderPlan {
            content: "\u{ea01}\n\u{ea02}".to_owned(),
            font_family: "remixicon".to_owned(),
            font_size: 64.0,
            bold: false,
            italic: false,
            letter_spacing: 2.0,
            line_spacing: 72.0,
            alignment: TextAlignment::Right,
            color: RgbaColor {
                red: 240,
                green: 180,
                blue: 80,
                alpha: 192,
            },
            outline_enabled: true,
            outline_color: RgbaColor {
                red: 20,
                green: 40,
                blue: 60,
                alpha: 255,
            },
            outline_width: 3.0,
            shadow_enabled: true,
            shadow_color: RgbaColor {
                red: 0,
                green: 0,
                blue: 0,
                alpha: 128,
            },
            shadow_offset_x: -6.0,
            shadow_offset_y: 5.0,
            background_enabled: true,
            background_color: RgbaColor {
                red: 30,
                green: 50,
                blue: 70,
                alpha: 160,
            },
            background_radius: 8.0,
            background_padding_x: 12.0,
            background_padding_y: 9.0,
            opacity: 1.0,
        }
    }

    fn test_rasterizer() -> TextRasterizer {
        let mut database = Database::new();
        database.load_font_data(include_bytes!("../../../ui/resources/remixicon.ttf").to_vec());
        TextRasterizer { database }
    }

    #[test]
    fn rasterizes_multiline_text_with_outline_shadow_and_background() {
        let frame = test_rasterizer()
            .rasterize(&plan(), 1.25)
            .expect("bundled outline font rasterizes");
        assert!(frame.width > 64);
        assert!(frame.height > 128);
        assert_eq!(frame.timestamp_seconds, 1.25);
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4
        );
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 255)
        );
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 160)
        );
    }

    #[test]
    fn rejects_unbounded_text_allocations() {
        let mut huge = plan();
        huge.font_size = 9_000.0;
        assert_eq!(
            test_rasterizer().rasterize(&huge, 0.0),
            Err(TextRasterError::InvalidDimensions)
        );
    }

    #[test]
    fn alignment_offsets_follow_qt_horizontal_flags() {
        assert_eq!(alignment_offset(TextAlignment::Left, 100.0, 40.0), 0.0);
        assert_eq!(alignment_offset(TextAlignment::Justify, 100.0, 40.0), 0.0);
        assert_eq!(alignment_offset(TextAlignment::Center, 100.0, 40.0), 30.0);
        assert_eq!(alignment_offset(TextAlignment::Right, 100.0, 40.0), 60.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_catalog_rasterizes_the_builtin_japanese_default() {
        let mut japanese = plan();
        japanese.content = "テキスト".to_owned();
        japanese.font_family = "sans-serif".to_owned();
        let frame = TextRasterizer::new()
            .rasterize(&japanese, 0.0)
            .expect("macOS system fonts cover the built-in Japanese text");
        assert!(
            frame
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
    }
}
