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
//! from other tools stay readable, and ignores a trailing field it does not know.

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
}

/// Parses a decimal field the way AviUtl's own reader does: take the longest numeric prefix
/// and ignore whatever follows.
///
/// This is not laxness for its own sake. ExEdit writes a trackbar value with full floating
/// point precision and then appends its scale suffix, so a real file can contain
/// `90.00000000000001.0`, which is not a number as a whole but is unambiguous to read. Its
/// reader stops at the first byte that cannot continue the number, and refusing the field
/// outright would silently drop the whole parameter.
fn parse_number(field: &str) -> Option<f64> {
    let trimmed = field.trim();
    if let Ok(value) = trimmed.parse::<f64>() {
        return value.is_finite().then_some(value);
    }
    let end = numeric_prefix_end(trimmed);
    if end == 0 {
        return None;
    }
    let value: f64 = trimmed[..end].parse().ok()?;
    value.is_finite().then_some(value)
}

/// Length of the leading substring that can still be a decimal number.
fn numeric_prefix_end(text: &str) -> usize {
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_exponent = false;
    for (index, character) in text.char_indices() {
        let accepted = match character {
            '0'..='9' => true,
            '+' | '-' => index == 0 || text[..index].ends_with(['e', 'E']),
            '.' => !seen_dot && !seen_exponent,
            'e' | 'E' => !seen_exponent && index > 0,
            _ => false,
        };
        if !accepted {
            break;
        }
        if character == '.' {
            seen_dot = true;
        } else if character == 'e' || character == 'E' {
            seen_exponent = true;
        }
        end = index + character.len_utf8();
    }
    // A trailing exponent marker or sign is not part of the number.
    while end > 0 && matches!(text.as_bytes()[end - 1], b'e' | b'E' | b'+' | b'-' | b'.') {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_constant_and_range_shapes() {
        let constant = TrackValue::parse("350.0").unwrap();
        assert_eq!(constant.start, 350.0);
        assert!(constant.end.is_none());

        let range = TrackValue::parse("23.00,-30.00,15@easing_普通@uf_easing,16").unwrap();
        assert_eq!(range.start, 23.0);
        assert_eq!(range.end, Some(-30.0));
        assert_eq!(range.motion.as_deref(), Some("15@easing_普通@uf_easing"));
        assert_eq!(range.motion_parameter, Some(16));

        let plain_range = TrackValue::parse("0,100").unwrap();
        assert_eq!(plain_range.start, 0.0);
        assert_eq!(plain_range.end, Some(100.0));
        assert!(plain_range.motion.is_none());
    }

    #[test]
    fn reads_a_value_with_a_precision_artifact() {
        // As written by ExEdit in `script/[ティム]/1280_720_6.exo`.
        let track = TrackValue::parse("180.0,90.00000000000001.0,1").unwrap();
        assert_eq!(track.start, 180.0);
        assert_eq!(track.end, Some(90.00000000000001));
        assert_eq!(track.motion.as_deref(), Some("1"));
        assert_eq!(
            TrackValue::parse("2.2043642384652358e-14.0").unwrap().start,
            2.2043642384652358e-14
        );
    }

    #[test]
    fn rejects_values_that_are_not_numbers() {
        assert!(TrackValue::parse("").is_none());
        assert!(TrackValue::parse("ffffff").is_none());
        assert!(TrackValue::parse("nan").is_none());
        assert!(TrackValue::parse("1,oops").is_none());
    }
}
