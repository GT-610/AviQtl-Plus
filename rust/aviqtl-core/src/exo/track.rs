//! ExEdit track values.
//!
//! A trackbar value is written in one of two shapes:
//!
//! * `12.50` — one value for the whole object.
//! * `12.50,80.00,15@easing_普通@uf_easing,3` — a start value, an end value, the motion
//!   ("変化方法") that joins them, and its script parameter.
//!
//! ExEdit abbreviates the first shape when the motion identifier is zero, which is why most
//! values in a real file have only one field. The parser accepts one to four fields so files
//! from other tools, and our own output, stay readable; the writer reproduces the shape the
//! value was parsed from.

use std::fmt::Write as _;

/// One trackbar (or check/colour) parameter as stored in an object file.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackValue {
    /// Value at the first frame of the object.
    pub start: f64,
    /// Value at the last frame, when the file stores a range.
    pub end: Option<f64>,
    /// Motion identifier, for example `15@easing_普通@uf_easing`.
    pub motion: Option<String>,
    /// Script parameter that accompanies [`Self::motion`].
    pub motion_parameter: Option<i32>,
}

impl TrackValue {
    /// A constant value with no motion.
    pub fn constant(value: f64) -> Self {
        Self {
            start: value,
            end: None,
            motion: None,
            motion_parameter: None,
        }
    }

    /// A value that changes between the first and last frame.
    pub fn range(start: f64, end: f64, motion: Option<String>) -> Self {
        Self {
            start,
            end: Some(end),
            motion,
            motion_parameter: None,
        }
    }

    /// Whether the value changes over the object.
    pub fn is_animated(&self) -> bool {
        self.end
            .is_some_and(|end| (end - self.start).abs() > f64::EPSILON)
    }

    /// Parses one `key=value` right-hand side. Returns `None` when the first field is not a
    /// finite number, which is what keeps a stray key from being mistaken for a trackbar.
    pub fn parse(text: &str) -> Option<Self> {
        let mut fields = text.split(',');
        let start = parse_number(fields.next()?)?;
        let Some(end_field) = fields.next() else {
            return Some(Self::constant(start));
        };
        let end = parse_number(end_field)?;
        let motion = fields
            .next()
            .map(str::trim)
            .filter(|motion| !motion.is_empty())
            .map(str::to_owned);
        let motion_parameter = fields.next().and_then(|value| value.trim().parse().ok());
        Some(Self {
            start,
            end: Some(end),
            motion,
            motion_parameter,
        })
    }

    /// Writes the value back in ExEdit's shape.
    pub fn to_text(&self) -> String {
        let mut text = format_number(self.start);
        if let Some(end) = self.end {
            let _ = write!(text, ",{}", format_number(end));
            if let Some(motion) = &self.motion {
                let _ = write!(text, ",{motion}");
                if let Some(parameter) = self.motion_parameter {
                    let _ = write!(text, ",{parameter}");
                }
            }
        }
        text
    }
}

/// Parses a decimal field, tolerating the surrounding spaces some tools leave behind.
fn parse_number(field: &str) -> Option<f64> {
    let trimmed = field.trim();
    let value: f64 = trimmed.parse().ok()?;
    value.is_finite().then_some(value)
}

/// Writes a number the way the source field was written: one decimal place for whole values,
/// and the shortest exact form otherwise.
fn format_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        let mut text = format!("{value}");
        if text.contains('e') {
            text = format!("{value:.6}");
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_constant_and_range_shapes() {
        let constant = TrackValue::parse("350.0").unwrap();
        assert_eq!(constant.start, 350.0);
        assert!(constant.end.is_none());
        assert!(!constant.is_animated());

        let range = TrackValue::parse("23.00,-30.00,15@easing_普通@uf_easing,16").unwrap();
        assert_eq!(range.start, 23.0);
        assert_eq!(range.end, Some(-30.0));
        assert_eq!(range.motion.as_deref(), Some("15@easing_普通@uf_easing"));
        assert_eq!(range.motion_parameter, Some(16));
        assert!(range.is_animated());

        let plain_range = TrackValue::parse("0,100").unwrap();
        assert_eq!(plain_range.start, 0.0);
        assert_eq!(plain_range.end, Some(100.0));
        assert!(plain_range.motion.is_none());
    }

    #[test]
    fn rejects_values_that_are_not_numbers() {
        assert!(TrackValue::parse("").is_none());
        assert!(TrackValue::parse("ffffff").is_none());
        assert!(TrackValue::parse("nan").is_none());
        assert!(TrackValue::parse("1,oops").is_none());
    }

    #[test]
    fn writes_the_shape_it_parsed() {
        for text in [
            "350.0",
            "-30.0",
            "23.0,-30.0",
            "23.0,-30.0,15@easing_普通@uf_easing,16",
        ] {
            assert_eq!(TrackValue::parse(text).unwrap().to_text(), text);
        }
        assert_eq!(TrackValue::constant(12.0).to_text(), "12.0");
        assert_eq!(TrackValue::constant(12.5).to_text(), "12.5");
    }
}
