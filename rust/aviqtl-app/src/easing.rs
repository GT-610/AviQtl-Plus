use aviqtl_rust_core::api::evaluate_keyframe_track;
use serde_json::Value;

const POINT_STRIDE: usize = 6;
const DEFAULT_POINTS: [f64; POINT_STRIDE] = [0.33, 0.0, 0.66, 1.0, 1.0, 1.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BezierHandle {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub anchor: bool,
    pub removable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BezierCurve {
    points: Vec<f64>,
}

impl Default for BezierCurve {
    fn default() -> Self {
        Self {
            points: DEFAULT_POINTS.to_vec(),
        }
    }
}

impl BezierCurve {
    pub fn from_points(points: &[f64]) -> Self {
        if points.len() >= POINT_STRIDE
            && points.len().is_multiple_of(POINT_STRIDE)
            && points.iter().all(|value| value.is_finite())
        {
            Self {
                points: points.to_vec(),
            }
        } else {
            Self::default()
        }
    }

    pub fn points(&self) -> &[f64] {
        &self.points
    }

    pub fn first_controls(&self) -> [f64; 4] {
        [
            self.points[0],
            self.points[1],
            self.points[2],
            self.points[3],
        ]
    }

    pub fn set_first_controls(&mut self, controls: [f64; 4]) -> bool {
        if controls.iter().any(|value| !value.is_finite()) {
            return false;
        }
        let controls = [
            controls[0].clamp(0.0, 1.0),
            controls[1],
            controls[2].clamp(0.0, 1.0),
            controls[3],
        ];
        let changed = self.points[..4] != controls;
        self.points[..4].copy_from_slice(&controls);
        changed
    }

    pub fn handles(&self) -> Vec<BezierHandle> {
        self.points
            .as_chunks::<POINT_STRIDE>()
            .0
            .iter()
            .enumerate()
            .flat_map(|(segment, points)| {
                let start = segment * POINT_STRIDE;
                let last_anchor = start + 4 == self.points.len() - 2;
                [
                    BezierHandle {
                        index: start,
                        x: points[0],
                        y: points[1],
                        anchor: false,
                        removable: false,
                    },
                    BezierHandle {
                        index: start + 2,
                        x: points[2],
                        y: points[3],
                        anchor: false,
                        removable: false,
                    },
                    BezierHandle {
                        index: start + 4,
                        x: points[4],
                        y: points[5],
                        anchor: true,
                        removable: !last_anchor,
                    },
                ]
            })
            .collect()
    }

    pub fn hit_test(&self, x: f64, y: f64, tolerance_x: f64, tolerance_y: f64) -> Option<usize> {
        if !x.is_finite()
            || !y.is_finite()
            || !tolerance_x.is_finite()
            || !tolerance_y.is_finite()
            || tolerance_x <= 0.0
            || tolerance_y <= 0.0
        {
            return None;
        }
        self.handles()
            .into_iter()
            .filter_map(|handle| {
                let dx = (x - handle.x) / tolerance_x;
                let dy = (y - handle.y) / tolerance_y;
                let distance = dx * dx + dy * dy;
                (distance < 1.0).then_some((handle.index, distance))
            })
            .min_by(|(_, first), (_, second)| first.total_cmp(second))
            .map(|(index, _)| index)
    }

    pub fn insert_anchor(&mut self, x: f64, y: f64) -> bool {
        if !x.is_finite() || !y.is_finite() || x <= 0.01 || x >= 0.99 {
            return false;
        }
        let insert_index = self
            .points
            .as_chunks::<POINT_STRIDE>()
            .0
            .iter()
            .position(|segment| x < segment[4])
            .map_or(self.points.len(), |segment| segment * POINT_STRIDE);
        let (previous_x, previous_y) = if insert_index == 0 {
            (0.0, 0.0)
        } else {
            (self.points[insert_index - 2], self.points[insert_index - 1])
        };
        let segment = [
            previous_x + (x - previous_x) * 0.33,
            previous_y + (y - previous_y) * 0.33,
            previous_x + (x - previous_x) * 0.66,
            previous_y + (y - previous_y) * 0.66,
            x,
            y,
        ];
        self.points.splice(insert_index..insert_index, segment);
        if insert_index + POINT_STRIDE < self.points.len() {
            self.points[insert_index + POINT_STRIDE] =
                x + (self.points[insert_index + 10] - x) * 0.33;
            self.points[insert_index + POINT_STRIDE + 1] =
                y + (self.points[insert_index + 11] - y) * 0.33;
        }
        true
    }

    pub fn move_point(&mut self, index: usize, x: f64, y: f64) -> bool {
        if !index.is_multiple_of(2)
            || index + 1 >= self.points.len()
            || !x.is_finite()
            || !y.is_finite()
        {
            return false;
        }
        let (x, y) = if index == self.points.len() - 2 {
            (1.0, 1.0)
        } else if (index + 2).is_multiple_of(POINT_STRIDE) {
            (x.clamp(0.001, 0.999), y)
        } else {
            (x.clamp(0.0, 1.0), y)
        };
        let changed = self.points[index] != x || self.points[index + 1] != y;
        self.points[index] = x;
        self.points[index + 1] = y;
        changed
    }

    pub fn remove_anchor(&mut self, index: usize) -> bool {
        if index < 4 || !(index + 2).is_multiple_of(POINT_STRIDE) || index >= self.points.len() - 2
        {
            return false;
        }
        self.points.drain(index - 4..index + 2);
        true
    }
}

pub fn sample_easing_curve(options: &Value, steps: i32) -> Vec<(f64, f64)> {
    let steps = steps.clamp(1, 4096);
    let mut start = options.as_object().cloned().unwrap_or_default();
    start.insert("frame".to_owned(), serde_json::json!(0));
    start.insert("value".to_owned(), serde_json::json!(0.0));
    let track = serde_json::json!({
        "start": start,
        "points": [{"frame": steps, "value": 1.0, "interp": "none"}]
    });
    let fallback = serde_json::json!(0.0);
    (0..=steps)
        .map(|frame| {
            let x = f64::from(frame) / f64::from(steps);
            let y = evaluate_keyframe_track(Some(&track), &fallback, steps, frame)
                .as_f64()
                .unwrap_or(0.0);
            (x, y)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_curves_use_the_qt_single_segment_default() {
        assert_eq!(BezierCurve::from_points(&[]), BezierCurve::default());
        assert_eq!(
            BezierCurve::from_points(&[0.0, 0.0, 1.0]).points(),
            DEFAULT_POINTS
        );
        assert_eq!(
            BezierCurve::from_points(&[0.0, 0.0, 0.0, 0.0, f64::NAN, 1.0]).points(),
            DEFAULT_POINTS
        );
    }

    #[test]
    fn insertion_matches_the_qt_segment_shape_and_preserves_the_tail() {
        let mut curve = BezierCurve::default();
        assert!(curve.insert_anchor(0.5, 0.25));
        assert_eq!(curve.points().len(), 12);
        assert_eq!(&curve.points()[4..6], &[0.5, 0.25]);
        assert_eq!(&curve.points()[10..12], &[1.0, 1.0]);
        assert!((curve.points()[6] - 0.665).abs() < f64::EPSILON);
        assert!((curve.points()[7] - 0.4975).abs() < f64::EPSILON);
        assert!(!curve.insert_anchor(0.005, 0.5));
        assert!(!curve.insert_anchor(0.995, 0.5));
    }

    #[test]
    fn dragging_and_removal_keep_the_qt_anchor_rules() {
        let mut curve = BezierCurve::default();
        assert!(curve.insert_anchor(0.5, 0.25));
        assert!(curve.move_point(0, -2.0, -1.0));
        assert_eq!(&curve.points()[..2], &[0.0, -1.0]);
        assert!(curve.move_point(4, 3.0, 2.0));
        assert_eq!(&curve.points()[4..6], &[0.999, 2.0]);
        assert!(!curve.move_point(10, 0.2, 0.3));
        assert_eq!(&curve.points()[10..12], &[1.0, 1.0]);
        assert!(!curve.remove_anchor(10));
        assert!(curve.remove_anchor(4));
        assert_eq!(curve.points().len(), POINT_STRIDE);
    }

    #[test]
    fn hit_testing_uses_screen_scaled_tolerances() {
        let curve = BezierCurve::default();
        assert_eq!(curve.hit_test(0.331, 0.001, 0.01, 0.01), Some(0));
        assert_eq!(curve.hit_test(0.5, 0.5, 0.01, 0.01), None);
        assert_eq!(curve.hit_test(0.33, 0.0, 0.0, 0.01), None);
    }

    #[test]
    fn preview_sampling_uses_the_production_keyframe_evaluator() {
        let points = sample_easing_curve(&serde_json::json!({"interp": "linear"}), 2);
        assert_eq!(points, [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]);
    }
}
