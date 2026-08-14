#![recursion_limit = "256"]

mod abi;
mod audio;
mod effect;
mod keyframe;
mod keyframe_document;
mod package;
mod policy;
mod preset;
mod project;
mod script;
mod settings;
mod timeline;
mod timeline_domain;
mod timeline_edit;

use abi::{AviQtlEasingParameters, slice_is_valid};
use std::f64::consts::PI;

const DEFAULT_ELASTIC_PERIOD: f64 = 0.3;

#[repr(u32)]
#[derive(Clone, Copy)]
pub(crate) enum EasingKind {
    Linear = 0,
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
    EaseOutInSine,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseOutInQuad,
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    EaseOutInCubic,
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,
    EaseOutInQuart,
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,
    EaseOutInQuint,
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    EaseOutInExpo,
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,
    EaseOutInCirc,
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    EaseOutInBack,
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,
    EaseOutInElastic,
    EaseOutBounce,
    EaseInBounce,
    EaseInOutBounce,
    EaseOutInBounce,
    Custom,
}

impl EasingKind {
    const ALL: [Self; 42] = [
        Self::Linear,
        Self::EaseInSine,
        Self::EaseOutSine,
        Self::EaseInOutSine,
        Self::EaseOutInSine,
        Self::EaseInQuad,
        Self::EaseOutQuad,
        Self::EaseInOutQuad,
        Self::EaseOutInQuad,
        Self::EaseInCubic,
        Self::EaseOutCubic,
        Self::EaseInOutCubic,
        Self::EaseOutInCubic,
        Self::EaseInQuart,
        Self::EaseOutQuart,
        Self::EaseInOutQuart,
        Self::EaseOutInQuart,
        Self::EaseInQuint,
        Self::EaseOutQuint,
        Self::EaseInOutQuint,
        Self::EaseOutInQuint,
        Self::EaseInExpo,
        Self::EaseOutExpo,
        Self::EaseInOutExpo,
        Self::EaseOutInExpo,
        Self::EaseInCirc,
        Self::EaseOutCirc,
        Self::EaseInOutCirc,
        Self::EaseOutInCirc,
        Self::EaseInBack,
        Self::EaseOutBack,
        Self::EaseInOutBack,
        Self::EaseOutInBack,
        Self::EaseInElastic,
        Self::EaseOutElastic,
        Self::EaseInOutElastic,
        Self::EaseOutInElastic,
        Self::EaseOutBounce,
        Self::EaseInBounce,
        Self::EaseInOutBounce,
        Self::EaseOutInBounce,
        Self::Custom,
    ];

    pub(crate) fn from_abi(value: u32) -> Option<Self> {
        Self::ALL.get(value as usize).copied()
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        const NAMES: [&str; 42] = [
            "linear",
            "ease_in_sine",
            "ease_out_sine",
            "ease_in_out_sine",
            "ease_out_in_sine",
            "ease_in_quad",
            "ease_out_quad",
            "ease_in_out_quad",
            "ease_out_in_quad",
            "ease_in_cubic",
            "ease_out_cubic",
            "ease_in_out_cubic",
            "ease_out_in_cubic",
            "ease_in_quart",
            "ease_out_quart",
            "ease_in_out_quart",
            "ease_out_in_quart",
            "ease_in_quint",
            "ease_out_quint",
            "ease_in_out_quint",
            "ease_out_in_quint",
            "ease_in_expo",
            "ease_out_expo",
            "ease_in_out_expo",
            "ease_out_in_expo",
            "ease_in_circ",
            "ease_out_circ",
            "ease_in_out_circ",
            "ease_out_in_circ",
            "ease_in_back",
            "ease_out_back",
            "ease_in_out_back",
            "ease_out_in_back",
            "ease_in_elastic",
            "ease_out_elastic",
            "ease_in_out_elastic",
            "ease_out_in_elastic",
            "ease_out_bounce",
            "ease_in_bounce",
            "ease_in_out_bounce",
            "ease_out_in_bounce",
            "custom",
        ];
        NAMES
            .iter()
            .position(|candidate| *candidate == name)
            .and_then(|index| Self::ALL.get(index).copied())
    }
}

fn solve_bezier_t(x: f64, x1: f64, x2: f64) -> f64 {
    if x1 == x2 && x1 == x {
        return x;
    }

    let mut t = x;
    for _ in 0..8 {
        let one_minus_t = 1.0 - t;
        let current_x =
            3.0 * one_minus_t * one_minus_t * t * x1 + 3.0 * one_minus_t * t * t * x2 + t * t * t;
        let error = current_x - x;
        if error.abs() < 1e-5 {
            return t;
        }
        let dx_dt = 3.0 * one_minus_t * one_minus_t * x1
            + 6.0 * one_minus_t * t * (x2 - x1)
            + 3.0 * t * t * (1.0 - x2);
        if dx_dt.abs() < 1e-6 {
            break;
        }
        t -= error / dx_dt;
    }
    t.clamp(0.0, 1.0)
}

fn ease_out_bounce(mut x: f64) -> f64 {
    const N1: f64 = 7.5625;
    const D1: f64 = 2.75;
    if x < 1.0 / D1 {
        return N1 * x * x;
    }
    if x < 2.0 / D1 {
        x -= 1.5 / D1;
        return N1 * x * x + 0.75;
    }
    if x < 2.5 / D1 {
        x -= 2.25 / D1;
        return N1 * x * x + 0.9375;
    }
    x -= 2.625 / D1;
    N1 * x * x + 0.984375
}

fn custom_easing(x: f64, points: &[f64]) -> f64 {
    let mut previous_x = 0.0;
    let mut previous_y = 0.0;
    for (index, segment) in points.chunks_exact(6).enumerate() {
        let cp1x = segment[0];
        let cp1y = segment[1];
        let cp2x = segment[2];
        let cp2y = segment[3];
        let end_x = segment[4];
        let end_y = segment[5];
        if x <= end_x || index * 6 + 6 >= points.len() {
            let range = end_x - previous_x;
            if range < 1e-6 {
                return end_y;
            }
            let normalized_cp1x = (cp1x - previous_x) / range;
            let normalized_cp2x = (cp2x - previous_x) / range;
            let normalized_x = (x - previous_x) / range;
            let t = solve_bezier_t(normalized_x, normalized_cp1x, normalized_cp2x);
            let one_minus_t = 1.0 - t;
            return one_minus_t * one_minus_t * one_minus_t * previous_y
                + 3.0 * one_minus_t * one_minus_t * t * cp1y
                + 3.0 * one_minus_t * t * t * cp2y
                + t * t * t * end_y;
        }
        previous_x = end_x;
        previous_y = end_y;
    }
    x
}

pub(crate) fn evaluate(
    kind: EasingKind,
    t: f64,
    points: &[f64],
    parameters: AviQtlEasingParameters,
) -> f64 {
    let elastic_period = if parameters.period > 0.0 {
        parameters.period
    } else {
        DEFAULT_ELASTIC_PERIOD
    };

    match kind {
        EasingKind::Linear => t,
        EasingKind::EaseInSine => 1.0 - (t * PI / 2.0).cos(),
        EasingKind::EaseOutSine => (t * PI / 2.0).sin(),
        EasingKind::EaseInOutSine => -((PI * t).cos() - 1.0) / 2.0,
        EasingKind::EaseOutInSine => {
            if t < 0.5 {
                (t * PI).sin() / 2.0
            } else {
                (1.0 - ((t * 2.0 - 1.0) * PI / 2.0).cos()) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInQuad => t * t,
        EasingKind::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
        EasingKind::EaseInOutQuad => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
        EasingKind::EaseOutInQuad => {
            if t < 0.5 {
                (1.0 - (1.0 - 2.0 * t).powi(2)) / 2.0
            } else {
                (2.0 * t - 1.0).powi(2) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInCubic => t.powi(3),
        EasingKind::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
        EasingKind::EaseInOutCubic => {
            if t < 0.5 {
                4.0 * t.powi(3)
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        EasingKind::EaseOutInCubic => {
            if t < 0.5 {
                (1.0 - (1.0 - 2.0 * t).powi(3)) / 2.0
            } else {
                (2.0 * t - 1.0).powi(3) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInQuart => t.powi(4),
        EasingKind::EaseOutQuart => 1.0 - (1.0 - t).powi(4),
        EasingKind::EaseInOutQuart => {
            if t < 0.5 {
                8.0 * t.powi(4)
            } else {
                1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
            }
        }
        EasingKind::EaseOutInQuart => {
            if t < 0.5 {
                (1.0 - (1.0 - 2.0 * t).powi(4)) / 2.0
            } else {
                (2.0 * t - 1.0).powi(4) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInQuint => t.powi(5),
        EasingKind::EaseOutQuint => 1.0 - (1.0 - t).powi(5),
        EasingKind::EaseInOutQuint => {
            if t < 0.5 {
                16.0 * t.powi(5)
            } else {
                1.0 - (-2.0 * t + 2.0).powi(5) / 2.0
            }
        }
        EasingKind::EaseOutInQuint => {
            if t < 0.5 {
                (1.0 - (1.0 - 2.0 * t).powi(5)) / 2.0
            } else {
                (2.0 * t - 1.0).powi(5) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInExpo => {
            if t == 0.0 {
                0.0
            } else {
                2.0_f64.powf(10.0 * t - 10.0)
            }
        }
        EasingKind::EaseOutExpo => {
            if t == 1.0 {
                1.0
            } else {
                1.0 - 2.0_f64.powf(-10.0 * t)
            }
        }
        EasingKind::EaseInOutExpo => {
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else if t < 0.5 {
                2.0_f64.powf(20.0 * t - 10.0) / 2.0
            } else {
                (2.0 - 2.0_f64.powf(-20.0 * t + 10.0)) / 2.0
            }
        }
        EasingKind::EaseOutInExpo => {
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else if t < 0.5 {
                (1.0 - 2.0_f64.powf(-20.0 * t)) / 2.0
            } else {
                2.0_f64.powf(20.0 * t - 20.0) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInCirc => 1.0 - (1.0 - t * t).sqrt(),
        EasingKind::EaseOutCirc => (1.0 - (t - 1.0).powi(2)).sqrt(),
        EasingKind::EaseInOutCirc => {
            if t < 0.5 {
                (1.0 - (1.0 - 4.0 * t * t).sqrt()) / 2.0
            } else {
                ((1.0 - (-2.0 * t + 2.0).powi(2)).sqrt() + 1.0) / 2.0
            }
        }
        EasingKind::EaseOutInCirc => {
            if t < 0.5 {
                (1.0 - (2.0 * t - 1.0).powi(2)).sqrt() / 2.0
            } else {
                (1.0 - (1.0 - (2.0 * t - 1.0).powi(2)).sqrt()) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInBack => {
            const C1: f64 = 1.70158;
            (C1 + 1.0) * t.powi(3) - C1 * t * t
        }
        EasingKind::EaseOutBack => {
            const C1: f64 = 1.70158;
            1.0 + (C1 + 1.0) * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
        }
        EasingKind::EaseInOutBack => {
            const C2: f64 = 1.70158 * 1.525;
            if t < 0.5 {
                (2.0 * t).powi(2) * ((C2 + 1.0) * 2.0 * t - C2) / 2.0
            } else {
                ((2.0 * t - 2.0).powi(2) * ((C2 + 1.0) * (2.0 * t - 2.0) + C2) + 2.0) / 2.0
            }
        }
        EasingKind::EaseOutInBack => {
            const C1: f64 = 1.70158;
            let ease_out = |u: f64| 1.0 + (C1 + 1.0) * (u - 1.0).powi(3) + C1 * (u - 1.0).powi(2);
            let ease_in = |u: f64| (C1 + 1.0) * u.powi(3) - C1 * u.powi(2);
            if t < 0.5 {
                ease_out(2.0 * t) / 2.0
            } else {
                ease_in(2.0 * t - 1.0) / 2.0 + 0.5
            }
        }
        EasingKind::EaseInElastic => {
            let c4 = 2.0 * PI / elastic_period;
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else {
                -parameters.amplitude
                    * 2.0_f64.powf(10.0 * t - 10.0)
                    * ((t - 1.0 - elastic_period / 4.0) * c4).sin()
            }
        }
        EasingKind::EaseOutElastic => {
            let c4 = 2.0 * PI / elastic_period;
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else {
                parameters.amplitude
                    * 2.0_f64.powf(-10.0 * t)
                    * ((t - elastic_period / 4.0) * c4).sin()
                    + 1.0
            }
        }
        EasingKind::EaseInOutElastic => {
            let period = elastic_period * 1.5;
            let c5 = 2.0 * PI / period;
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else if t < 0.5 {
                -(parameters.amplitude
                    * 2.0_f64.powf(20.0 * t - 10.0)
                    * ((20.0 * t - 11.125) * c5).sin())
                    / 2.0
            } else {
                parameters.amplitude
                    * 2.0_f64.powf(-20.0 * t + 10.0)
                    * ((20.0 * t - 11.125) * c5).sin()
                    / 2.0
                    + 1.0
            }
        }
        EasingKind::EaseOutInElastic => {
            let c4 = 2.0 * PI / elastic_period;
            let ease_out = |u: f64| {
                parameters.amplitude
                    * 2.0_f64.powf(-10.0 * u)
                    * ((u - elastic_period / 4.0) * c4).sin()
                    + 1.0
            };
            let ease_in = |u: f64| {
                -parameters.amplitude
                    * 2.0_f64.powf(10.0 * u - 10.0)
                    * ((u - 1.0 - elastic_period / 4.0) * c4).sin()
            };
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else if t < 0.5 {
                ease_out(2.0 * t) / 2.0
            } else {
                ease_in(2.0 * t - 1.0) / 2.0 + 0.5
            }
        }
        EasingKind::EaseOutBounce => ease_out_bounce(t),
        EasingKind::EaseInBounce => 1.0 - ease_out_bounce(1.0 - t),
        EasingKind::EaseInOutBounce => {
            if t < 0.5 {
                (1.0 - ease_out_bounce(1.0 - 2.0 * t)) / 2.0
            } else {
                (1.0 + ease_out_bounce(2.0 * t - 1.0)) / 2.0
            }
        }
        EasingKind::EaseOutInBounce => {
            if t < 0.5 {
                ease_out_bounce(2.0 * t) / 2.0
            } else {
                (1.0 - ease_out_bounce(1.0 - 2.0 * (t - 0.5))) / 2.0 + 0.5
            }
        }
        EasingKind::Custom => custom_easing(t, points),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aviqtl_solve_bezier_t(x: f64, x1: f64, x2: f64) -> f64 {
    solve_bezier_t(x, x1, x2)
}

#[unsafe(no_mangle)]
/// Evaluates one easing curve through the stable C ABI.
///
/// # Safety
///
/// When `points_len` is non-zero, `points` must address at least
/// `points_len` contiguous, initialized `f64` values and remain valid for the
/// duration of the call. A zero length permits a null pointer.
pub unsafe extern "C" fn aviqtl_easing_evaluate(
    kind: u32,
    t: f64,
    points: *const f64,
    points_len: usize,
    parameters: AviQtlEasingParameters,
) -> f64 {
    let Some(kind) = EasingKind::from_abi(kind) else {
        return t;
    };
    let points = if !slice_is_valid(points, points_len) {
        return t;
    } else if points_len == 0 {
        &[]
    } else {
        // SAFETY: The C++ adapter supplies a pointer to `points_len` contiguous
        // doubles and keeps the vector alive for the duration of this call.
        unsafe { std::slice::from_raw_parts(points, points_len) }
    };
    evaluate(kind, t, points, parameters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "actual={actual:.17e} expected={expected:.17e}"
        );
    }

    #[test]
    fn matches_cpp_golden_values() {
        let parameters = AviQtlEasingParameters {
            amplitude: 0.8,
            period: 0.42,
        };
        let samples = [0.1, 0.25, 0.5, 0.75, 0.9];
        let custom_points = [0.1, 0.2, 0.4, 0.5, 0.6, 0.7, 0.7, 0.8, 0.9, 0.95, 1.0, 1.0];
        let golden = include_str!("../../../tests/data/keyframe_easing_cpp_golden.txt");
        let mut case_count = 0;
        for line in golden
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let mut fields = line.split_whitespace();
            let kind = fields
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(EasingKind::from_abi)
                .expect("valid easing kind in C++ golden data");
            let expected: Vec<f64> = fields
                .map(|value| {
                    value
                        .parse::<f64>()
                        .expect("valid number in C++ golden data")
                })
                .collect();
            assert_eq!(expected.len(), samples.len());
            let points = if matches!(kind, EasingKind::Custom) {
                custom_points.as_slice()
            } else {
                &[]
            };
            for (index, sample) in samples.iter().enumerate() {
                assert_close(evaluate(kind, *sample, points, parameters), expected[index]);
            }
            case_count += 1;
        }
        assert_eq!(case_count, EasingKind::Custom as usize + 1);
    }

    #[test]
    fn ffi_rejects_invalid_inputs_without_panicking() {
        let parameters = AviQtlEasingParameters {
            amplitude: 1.0,
            period: 0.3,
        };
        // SAFETY: Both calls use null only with either a zero length or an
        // intentionally invalid pair that the boundary must reject before use.
        unsafe {
            assert_eq!(
                aviqtl_easing_evaluate(999, 0.25, std::ptr::null(), 0, parameters),
                0.25
            );
            assert_eq!(
                aviqtl_easing_evaluate(
                    EasingKind::Custom as u32,
                    0.25,
                    std::ptr::null(),
                    6,
                    parameters,
                ),
                0.25
            );
            let bytes = [0_u8; size_of::<f64>() + align_of::<f64>()];
            let misaligned_offset = (0..align_of::<f64>())
                .find(|offset| {
                    !(bytes.as_ptr() as usize + offset).is_multiple_of(align_of::<f64>())
                })
                .expect("a misaligned offset");
            assert_eq!(
                aviqtl_easing_evaluate(
                    EasingKind::Custom as u32,
                    0.25,
                    bytes.as_ptr().add(misaligned_offset).cast(),
                    1,
                    parameters,
                ),
                0.25
            );
            assert_eq!(
                aviqtl_easing_evaluate(
                    EasingKind::Custom as u32,
                    0.25,
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                    isize::MAX as usize / size_of::<f64>() + 1,
                    parameters,
                ),
                0.25
            );
        }
    }

    #[test]
    fn elastic_easings_use_default_for_non_positive_periods() {
        let kinds = [
            EasingKind::EaseInElastic,
            EasingKind::EaseOutElastic,
            EasingKind::EaseInOutElastic,
            EasingKind::EaseOutInElastic,
        ];
        let fallback_parameters = AviQtlEasingParameters {
            amplitude: 0.8,
            period: DEFAULT_ELASTIC_PERIOD,
        };

        for kind in kinds {
            let expected = evaluate(kind, 0.37, &[], fallback_parameters);
            for period in [0.0, -0.42] {
                let actual = evaluate(
                    kind,
                    0.37,
                    &[],
                    AviQtlEasingParameters {
                        amplitude: 0.8,
                        period,
                    },
                );
                assert!(actual.is_finite());
                assert_close(actual, expected);
            }
        }
    }
}
