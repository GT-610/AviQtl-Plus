use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use crate::keyframe::{NumericSegment, evaluate_numeric_segment};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Request {
    Inspect {
        track: Value,
        fallback: Value,
        duration: i32,
    },
    Normalize {
        track: Value,
        fallback: Value,
        duration: i32,
    },
    Set {
        track: Value,
        fallback: Value,
        duration: i32,
        frame: i32,
        value: Value,
        options: Value,
    },
    Remove {
        track: Value,
        fallback: Value,
        duration: i32,
        frame: i32,
    },
    Move {
        track: Value,
        fallback: Value,
        duration: i32,
        old_frame: i32,
        new_frame: i32,
    },
    Sync {
        track: Value,
        fallback: Value,
        old_duration: i32,
        new_duration: i32,
    },
    Split {
        track: Value,
        fallback: Value,
        first_half_duration: i32,
        original_duration: i32,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    track: Value,
    flat: Vec<Value>,
    inferred_duration: i32,
    accepted: bool,
    changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secondary_track: Option<Value>,
}

fn integer(value: Option<&Value>, fallback: i32) -> i32 {
    let Some(value) = value.map(payload) else {
        return fallback;
    };
    if let Some(number) = value.as_i64().and_then(|number| i32::try_from(number).ok()) {
        return number;
    }
    if let Some(number) = value.as_u64().and_then(|number| i32::try_from(number).ok()) {
        return number;
    }
    if let Some(number) = value.as_f64() {
        if number.is_finite() && number >= f64::from(i32::MIN) && number <= f64::from(i32::MAX) {
            return number as i32;
        }
    }
    value
        .as_str()
        .and_then(|text| text.parse::<i32>().ok())
        .unwrap_or(fallback)
}

fn payload(value: &Value) -> &Value {
    value
        .as_object()
        .filter(|object| object.contains_key("$aviqtlType"))
        .and_then(|object| object.get("value"))
        .unwrap_or(value)
}

fn point_frame(point: &Value) -> i32 {
    integer(point.as_object().and_then(|point| point.get("frame")), 0)
}

fn point_value(point: &Value) -> Option<&Value> {
    point.as_object().and_then(|point| point.get("value"))
}

fn sort_points(points: &mut [Value]) {
    points.sort_by_key(point_frame);
}

fn is_structured(track: &Value) -> bool {
    track
        .as_object()
        .is_some_and(|track| track.contains_key("start"))
}

fn track_points(track: &Value) -> Vec<Value> {
    if is_structured(track) {
        track
            .as_object()
            .and_then(|track| track.get("points"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        track.as_array().cloned().unwrap_or_default()
    }
}

fn inferred_duration(track: &Value) -> i32 {
    let maximum = track_points(track)
        .iter()
        .map(point_frame)
        .max()
        .unwrap_or(0);
    maximum.saturating_add(1).max(1)
}

fn mode_number(point: &Map<String, Value>, name: &str, fallback: f64) -> f64 {
    point
        .get("modeParams")
        .and_then(Value::as_object)
        .and_then(|params| params.get(name))
        .map(payload)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

fn mode_integer(point: &Map<String, Value>, name: &str, fallback: i32) -> i32 {
    integer(
        point
            .get("modeParams")
            .and_then(Value::as_object)
            .and_then(|params| params.get(name)),
        fallback,
    )
}

fn custom_points(point: &Map<String, Value>) -> Vec<f64> {
    if let Some(points) = point.get("points").and_then(Value::as_array) {
        let parsed: Vec<_> = points
            .iter()
            .map(payload)
            .filter_map(Value::as_f64)
            .collect();
        if parsed.len() >= 6 && parsed.len() % 6 == 0 {
            return parsed;
        }
    }
    let params = point.get("modeParams").and_then(Value::as_object);
    vec![
        params
            .and_then(|params| params.get("bzx1"))
            .or_else(|| point.get("bzx1"))
            .map(payload)
            .and_then(Value::as_f64)
            .unwrap_or(0.33),
        params
            .and_then(|params| params.get("bzy1"))
            .or_else(|| point.get("bzy1"))
            .map(payload)
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        params
            .and_then(|params| params.get("bzx2"))
            .or_else(|| point.get("bzx2"))
            .map(payload)
            .and_then(Value::as_f64)
            .unwrap_or(0.66),
        params
            .and_then(|params| params.get("bzy2"))
            .or_else(|| point.get("bzy2"))
            .map(payload)
            .and_then(Value::as_f64)
            .unwrap_or(1.0),
        1.0,
        1.0,
    ]
}

fn parse_hex_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.strip_prefix('#')?;
    let parsed = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some([
            255,
            ((parsed >> 16) & 0xff) as u8,
            ((parsed >> 8) & 0xff) as u8,
            (parsed & 0xff) as u8,
        ]),
        8 => Some([
            ((parsed >> 24) & 0xff) as u8,
            ((parsed >> 16) & 0xff) as u8,
            ((parsed >> 8) & 0xff) as u8,
            (parsed & 0xff) as u8,
        ]),
        _ => None,
    }
}

fn interpolate_color(first: &str, second: &str, progress: f64) -> Option<Value> {
    let first = parse_hex_color(first)?;
    let second = parse_hex_color(second)?;
    let mut result = [0_u8; 4];
    for index in 0..4 {
        let value = f64::from(first[index])
            + (f64::from(second[index]) - f64::from(first[index])) * progress;
        result[index] = value.clamp(0.0, 255.0) as u8;
    }
    Some(Value::String(format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        result[0], result[1], result[2], result[3]
    )))
}

fn evaluate_track(points: &[Value], frame: i32, fallback: &Value) -> Value {
    if points.is_empty() {
        return fallback.clone();
    }
    if frame <= point_frame(&points[0]) {
        return point_value(&points[0])
            .cloned()
            .unwrap_or_else(|| fallback.clone());
    }
    if frame >= point_frame(&points[points.len() - 1]) {
        return point_value(&points[points.len() - 1])
            .cloned()
            .unwrap_or_else(|| fallback.clone());
    }
    for pair in points.windows(2) {
        let first_frame = point_frame(&pair[0]);
        let second_frame = point_frame(&pair[1]);
        if frame < first_frame || frame > second_frame {
            continue;
        }
        let first_value = point_value(&pair[0])
            .cloned()
            .unwrap_or_else(|| fallback.clone());
        let second_value = point_value(&pair[1])
            .cloned()
            .unwrap_or_else(|| fallback.clone());
        if first_frame == second_frame {
            return first_value;
        }
        let first = pair[0].as_object().cloned().unwrap_or_default();
        let interpolation = first
            .get("interp")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if interpolation == "none" {
            return if frame < second_frame {
                first_value
            } else {
                second_value
            };
        }
        if let (Some(first_number), Some(second_number)) = (
            payload(&first_value).as_f64(),
            payload(&second_value).as_f64(),
        ) {
            let points = custom_points(&first);
            return serde_json::Number::from_f64(evaluate_numeric_segment(NumericSegment {
                interpolation,
                first_value: first_number,
                second_value: second_number,
                first_frame,
                second_frame,
                frame,
                custom_points: &points,
                amplitude: mode_number(&first, "amplitude", 1.0),
                period: mode_number(&first, "period", 0.3),
                step_frames: mode_integer(&first, "stepFrames", 1).max(1),
            }))
            .map(Value::Number)
            .unwrap_or(first_value);
        }
        if let (Some(first_color), Some(second_color)) = (
            payload(&first_value).as_str(),
            payload(&second_value).as_str(),
        ) {
            let points = custom_points(&first);
            let progress = evaluate_numeric_segment(NumericSegment {
                interpolation,
                first_value: 0.0,
                second_value: 1.0,
                first_frame,
                second_frame,
                frame,
                custom_points: &points,
                amplitude: mode_number(&first, "amplitude", 1.0),
                period: mode_number(&first, "period", 0.3),
                step_frames: mode_integer(&first, "stepFrames", 1).max(1),
            });
            if let Some(color) = interpolate_color(first_color, second_color, progress) {
                return color;
            }
        }
        return first_value;
    }
    point_value(&points[points.len() - 1])
        .cloned()
        .unwrap_or_else(|| fallback.clone())
}

fn normalize_track(track: &Value, fallback: &Value, duration: i32) -> Value {
    let duration = duration.max(1);
    if is_structured(track) {
        let mut normalized = track.as_object().cloned().unwrap_or_default();
        let mut start = normalized
            .get("start")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        start.insert("frame".to_owned(), Value::from(0));
        start
            .entry("value".to_owned())
            .or_insert_with(|| fallback.clone());
        let mut points = track_points(track);
        points.retain(|point| {
            let frame = point_frame(point);
            frame > 0 && frame <= duration
        });
        sort_points(&mut points);
        normalized.insert("start".to_owned(), Value::Object(start));
        normalized.insert("points".to_owned(), Value::Array(points));
        return Value::Object(normalized);
    }

    let mut legacy = track_points(track);
    sort_points(&mut legacy);
    let mut start = Map::new();
    start.insert("frame".to_owned(), Value::from(0));
    start.insert("value".to_owned(), evaluate_track(&legacy, 0, fallback));
    let start_interpolation = legacy
        .iter()
        .find(|point| point_frame(point) == 0)
        .and_then(Value::as_object)
        .and_then(|point| point.get("interp"))
        .and_then(Value::as_str)
        .unwrap_or("linear");
    start.insert(
        "interp".to_owned(),
        Value::String(start_interpolation.to_owned()),
    );
    legacy.retain(|point| {
        let frame = point_frame(point);
        frame > 0 && frame <= duration
    });
    let mut normalized = Map::new();
    normalized.insert("start".to_owned(), Value::Object(start));
    normalized.insert("points".to_owned(), Value::Array(legacy));
    Value::Object(normalized)
}

fn flatten(track: &Value) -> Vec<Value> {
    let Some(track) = track.as_object() else {
        return Vec::new();
    };
    let mut points = Vec::new();
    points.push(
        track
            .get("start")
            .cloned()
            .unwrap_or(Value::Object(Map::new())),
    );
    points.extend(
        track
            .get("points")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    );
    sort_points(&mut points);
    points
}

fn response(track: Value, accepted: bool, changed: bool) -> Response {
    let duration = inferred_duration(&track);
    Response {
        flat: flatten(&track),
        inferred_duration: duration,
        track,
        accepted,
        changed,
        base_value: None,
        secondary_track: None,
    }
}

fn apply(request: Request) -> Response {
    match request {
        Request::Inspect {
            track,
            fallback,
            duration,
        }
        | Request::Normalize {
            track,
            fallback,
            duration,
        } => {
            let duration = if duration > 0 {
                duration
            } else {
                inferred_duration(&track)
            };
            response(normalize_track(&track, &fallback, duration), true, false)
        }
        Request::Set {
            track,
            fallback,
            duration,
            frame,
            value,
            options,
        } => {
            let duration = if duration > 0 {
                duration
            } else {
                inferred_duration(&track).max(frame.saturating_add(1))
            };
            let mut normalized = normalize_track(&track, &fallback, duration);
            let object = normalized
                .as_object_mut()
                .expect("normalized track is an object");
            let options = options.as_object().cloned().unwrap_or_default();
            if frame <= 0 {
                let start = object
                    .get_mut("start")
                    .and_then(Value::as_object_mut)
                    .expect("normalized start is an object");
                start.insert("value".to_owned(), value.clone());
                let interpolation = options
                    .get("interp")
                    .cloned()
                    .or_else(|| start.get("interp").cloned())
                    .unwrap_or_else(|| Value::String("none".to_owned()));
                start.insert("interp".to_owned(), interpolation);
                let mut result = response(normalized, true, true);
                result.base_value = Some(value);
                return result;
            }
            let mut keyframe = Map::new();
            keyframe.insert("frame".to_owned(), Value::from(frame));
            keyframe.insert("value".to_owned(), value);
            keyframe.insert(
                "interp".to_owned(),
                options
                    .get("interp")
                    .cloned()
                    .unwrap_or_else(|| Value::String("none".to_owned())),
            );
            for name in ["points", "modeParams"] {
                if let Some(value) = options.get(name) {
                    keyframe.insert(name.to_owned(), value.clone());
                }
            }
            let points = object
                .get_mut("points")
                .and_then(Value::as_array_mut)
                .expect("normalized points is an array");
            if let Some(index) = points.iter().position(|point| point_frame(point) == frame) {
                points[index] = Value::Object(keyframe);
            } else {
                points.push(Value::Object(keyframe));
            }
            sort_points(points);
            response(normalized, true, true)
        }
        Request::Remove {
            track,
            fallback,
            duration,
            frame,
        } => {
            let duration = if duration > 0 {
                duration
            } else {
                inferred_duration(&track)
            };
            let mut normalized = normalize_track(&track, &fallback, duration);
            if frame <= 0 {
                return response(normalized, false, false);
            }
            let points = normalized
                .as_object_mut()
                .and_then(|track| track.get_mut("points"))
                .and_then(Value::as_array_mut)
                .expect("normalized points is an array");
            let previous_length = points.len();
            points.retain(|point| point_frame(point) != frame);
            let changed = previous_length != points.len();
            response(normalized, changed, changed)
        }
        Request::Move {
            track,
            fallback,
            duration,
            old_frame,
            new_frame,
        } => {
            let duration = if duration > 0 {
                duration.max(new_frame.saturating_add(1))
            } else {
                inferred_duration(&track).max(new_frame.saturating_add(1))
            };
            let mut normalized = normalize_track(&track, &fallback, duration);
            if old_frame == new_frame {
                return response(normalized, true, false);
            }
            if old_frame <= 0 || new_frame <= 0 {
                return response(normalized, false, false);
            }
            let points = normalized
                .as_object_mut()
                .and_then(|track| track.get_mut("points"))
                .and_then(Value::as_array_mut)
                .expect("normalized points is an array");
            if points.iter().any(|point| point_frame(point) == new_frame) {
                return response(normalized, false, false);
            }
            let Some(source) = points
                .iter()
                .position(|point| point_frame(point) == old_frame)
            else {
                return response(normalized, false, false);
            };
            points[source]
                .as_object_mut()
                .expect("keyframe point is an object")
                .insert("frame".to_owned(), Value::from(new_frame));
            sort_points(points);
            response(normalized, true, true)
        }
        Request::Sync {
            track,
            fallback,
            old_duration,
            new_duration,
        } => {
            let was_structured = is_structured(&track);
            let mut normalized = if was_structured {
                let mut track = track.as_object().cloned().unwrap_or_default();
                let mut start = track
                    .get("start")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                start.insert("frame".to_owned(), Value::from(0));
                start.entry("value".to_owned()).or_insert(fallback);
                track.insert("start".to_owned(), Value::Object(start));
                track
                    .entry("points".to_owned())
                    .or_insert_with(|| Value::Array(Vec::new()));
                Value::Object(track)
            } else {
                let mut start = Map::new();
                start.insert("frame".to_owned(), Value::from(0));
                start.insert("value".to_owned(), fallback);
                start.insert("interp".to_owned(), Value::String("none".to_owned()));
                Value::Object(Map::from_iter([
                    ("start".to_owned(), Value::Object(start)),
                    ("points".to_owned(), Value::Array(Vec::new())),
                ]))
            };
            let mut changed = !was_structured;
            if old_duration > 0 && old_duration != new_duration {
                let points = normalized
                    .as_object_mut()
                    .and_then(|track| track.get_mut("points"))
                    .and_then(Value::as_array_mut)
                    .expect("normalized points is an array");
                if let Some(point) = points
                    .iter_mut()
                    .find(|point| point_frame(point) == old_duration)
                {
                    point
                        .as_object_mut()
                        .expect("keyframe point is an object")
                        .insert("frame".to_owned(), Value::from(new_duration));
                    sort_points(points);
                    changed = true;
                }
            }
            response(normalized, true, changed)
        }
        Request::Split {
            track,
            fallback,
            first_half_duration,
            original_duration,
        } => {
            if original_duration < 1 {
                let normalized = normalize_track(&track, &fallback, 1);
                let mut result = response(normalized, false, false);
                result.secondary_track = Some(Value::Object(Map::new()));
                return result;
            }
            let normalized = normalize_track(&track, &fallback, original_duration);
            let flat = flatten(&normalized);
            let original = normalized.as_object().cloned().unwrap_or_default();
            let start = original
                .get("start")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let points = original
                .get("points")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let first_end = first_half_duration.saturating_sub(1).max(0);
            let second_duration = original_duration.saturating_sub(first_half_duration).max(1);
            let second_end = second_duration.saturating_sub(1).max(0);

            let first_points: Vec<_> = points
                .iter()
                .filter(|point| {
                    let frame = point_frame(point);
                    frame > 0 && frame <= first_end
                })
                .cloned()
                .collect();
            let first_track = Value::Object(Map::from_iter([
                ("start".to_owned(), Value::Object(start.clone())),
                ("points".to_owned(), Value::Array(first_points)),
            ]));

            let mut second_start = Map::new();
            second_start.insert("frame".to_owned(), Value::from(0));
            second_start.insert(
                "value".to_owned(),
                evaluate_track(&flat, first_half_duration, &fallback),
            );
            second_start.insert(
                "interp".to_owned(),
                start
                    .get("interp")
                    .cloned()
                    .unwrap_or_else(|| Value::String("none".to_owned())),
            );
            let mut second_points = Vec::new();
            for mut point in points {
                let frame = point_frame(&point);
                if frame > first_half_duration && frame < original_duration.saturating_sub(1).max(0)
                {
                    let next_frame = frame - first_half_duration;
                    if next_frame > 0 && next_frame < second_end {
                        point
                            .as_object_mut()
                            .expect("keyframe point is an object")
                            .insert("frame".to_owned(), Value::from(next_frame));
                        second_points.push(point);
                    }
                }
            }
            let second_track = Value::Object(Map::from_iter([
                ("start".to_owned(), Value::Object(second_start)),
                ("points".to_owned(), Value::Array(second_points)),
            ]));
            let mut result = response(first_track, true, true);
            result.secondary_track = Some(second_track);
            result
        }
    }
}

/// Applies a typed keyframe-document operation encoded as JSON.
///
/// # Safety
///
/// The input must be readable, output writable, and all ranges disjoint. A null output with zero
/// capacity is a size query. Insufficient capacity never writes partial JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_keyframe_document_apply_json(
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(input, input_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(input, input_length, output, output_capacity),
        ranges_overlap(input, input_length, output_length, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let input = if input_length == 0 {
        &[]
    } else {
        // SAFETY: The input range was validated and checked against both outputs.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let Ok(request) = serde_json::from_slice::<Request>(input) else {
        return STATUS_INVALID_JSON;
    };
    let Ok(encoded) = serde_json::to_vec(&apply(request)) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The length output was validated and checked for overlap.
    unsafe { output_length.write(encoded.len()) };
    if output_capacity < encoded.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !encoded.is_empty() {
        // SAFETY: Capacity was checked, and the output is valid and disjoint.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..encoded.len()].copy_from_slice(&encoded);
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_and_mutates_legacy_tracks() {
        let inspected = apply(Request::Inspect {
            track: json!([
                {"frame": 20, "value": 20.0, "interp": "none"},
                {"frame": 0, "value": 0.0, "interp": "linear"},
                {"frame": 10, "value": 10.0, "interp": "linear"}
            ]),
            fallback: json!(5.0),
            duration: 21,
        });
        assert_eq!(inspected.flat.len(), 3);
        assert_eq!(point_frame(&inspected.flat[0]), 0);
        assert_eq!(point_frame(&inspected.flat[2]), 20);

        let endpoint = apply(Request::Inspect {
            track: json!([
                {"frame": 0, "value": 0.0, "interp": "linear"},
                {"frame": 20, "value": 20.0, "interp": "none"}
            ]),
            fallback: json!(0.0),
            duration: 20,
        });
        assert_eq!(
            endpoint.flat.iter().map(point_frame).collect::<Vec<_>>(),
            [0, 20]
        );

        let set = apply(Request::Set {
            track: inspected.track,
            fallback: json!(0.0),
            duration: 21,
            frame: 15,
            value: json!(15.0),
            options: json!({"interp": "custom", "points": [0.33, 0.0, 0.66, 1.0, 1.0, 1.0]}),
        });
        assert!(set.accepted && set.changed);
        assert_eq!(
            set.flat.iter().map(point_frame).collect::<Vec<_>>(),
            [0, 10, 15, 20]
        );

        let moved = apply(Request::Move {
            track: set.track,
            fallback: json!(0.0),
            duration: 21,
            old_frame: 15,
            new_frame: 8,
        });
        assert!(moved.accepted && moved.changed);
        assert_eq!(
            moved.flat.iter().map(point_frame).collect::<Vec<_>>(),
            [0, 8, 10, 20]
        );
    }

    #[test]
    fn sync_and_split_preserve_domain_values() {
        let track = json!({
            "start": {"frame": 0, "value": 0.0, "interp": "linear"},
            "points": [
                {"frame": 9, "value": 90.0, "interp": "linear"},
                {"frame": 10, "value": 100.0, "interp": "linear"},
                {"frame": 20, "value": 200.0, "interp": "none"}
            ]
        });
        let synced = apply(Request::Sync {
            track: track.clone(),
            fallback: json!(0.0),
            old_duration: 20,
            new_duration: 30,
        });
        assert_eq!(
            synced.flat.iter().map(point_frame).collect::<Vec<_>>(),
            [0, 9, 10, 30]
        );

        let split = apply(Request::Split {
            track,
            fallback: json!(0.0),
            first_half_duration: 10,
            original_duration: 21,
        });
        assert_eq!(
            split.flat.iter().map(point_frame).collect::<Vec<_>>(),
            [0, 9]
        );
        let second = split.secondary_track.unwrap();
        assert_eq!(second["start"]["value"].as_f64(), Some(100.0));
    }

    #[test]
    fn ffi_size_query_and_small_buffer_do_not_write_partial_json() {
        let request = serde_json::to_vec(&json!({
            "operation": "normalize",
            "track": [],
            "fallback": 1.0,
            "duration": 10
        }))
        .unwrap();
        let mut required = 0_usize;
        // SAFETY: The request and length output are valid and disjoint.
        let status = unsafe {
            aviqtl_keyframe_document_apply_json(
                request.as_ptr(),
                request.len(),
                std::ptr::null_mut(),
                0,
                &mut required,
            )
        };
        assert_eq!(status, STATUS_BUFFER_TOO_SMALL);
        assert!(required > 1);

        let mut output = vec![0xa5_u8; required - 1];
        let original = output.clone();
        let mut reported = 0_usize;
        // SAFETY: Every range is valid and disjoint; the output is intentionally undersized.
        let status = unsafe {
            aviqtl_keyframe_document_apply_json(
                request.as_ptr(),
                request.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut reported,
            )
        };
        assert_eq!(status, STATUS_BUFFER_TOO_SMALL);
        assert_eq!(reported, required);
        assert_eq!(output, original);
    }
}
