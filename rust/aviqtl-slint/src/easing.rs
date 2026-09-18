//! Easing editor models, labels, and curve presentation.

use crate::localization::localized;
use crate::object_settings::finite_f32;
use crate::projection::update_vec_model;
use crate::{CurveHandleData, EasingCatalogRowData, EasingConfigWindow};
use aviqtl_app::easing::{BezierCurve, sample_easing_curve};
use aviqtl_app::object_settings::{KeyframePoint, keyframe_interpolation_names};
use slint::{Model, SharedString};

pub(super) const EASING_CATEGORIES: [(&str, &[&str]); 5] = [
    ("Basic", &["none", "linear"]),
    (
        "Standard curves",
        &[
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
        ],
    ),
    (
        "Strong curves",
        &[
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
        ],
    ),
    (
        "Bounce and elasticity",
        &[
            "ease_in_back",
            "ease_out_back",
            "ease_in_out_back",
            "ease_out_in_back",
            "ease_in_elastic",
            "ease_out_elastic",
            "ease_in_out_elastic",
            "ease_out_in_elastic",
            "ease_in_bounce",
            "ease_out_bounce",
            "ease_in_out_bounce",
            "ease_out_in_bounce",
        ],
    ),
    ("Special", &["random", "alternate", "custom"]),
];

pub(super) fn easing_name_at(index: i32) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|index| keyframe_interpolation_names().get(index).copied())
        .unwrap_or("none")
}

pub(super) fn parameter_button_label(label: &str, keyframed: bool, interpolation: &str) -> String {
    if keyframed && !matches!(interpolation, "" | "constant" | "none") {
        format!("{label} ({})", easing_label(interpolation))
    } else {
        label.to_owned()
    }
}

pub(super) fn easing_label(name: &str) -> String {
    match name {
        "none" => return localized("Instant", "瞬间", "瞬間移動").to_owned(),
        "linear" => return localized("Linear", "线性", "直線").to_owned(),
        "custom" => return localized("Custom", "自定义", "カスタム").to_owned(),
        "random" => return localized("Random", "随机", "ランダム移動").to_owned(),
        "alternate" => return localized("Alternate", "往复", "反復移動").to_owned(),
        _ => {}
    }
    let (direction, family) = [
        (
            "ease_in_out_",
            localized("Ease in/out", "缓入缓出", "加減速"),
        ),
        (
            "ease_out_in_",
            localized("Ease out/in", "缓出缓入", "減加速"),
        ),
        ("ease_in_", localized("Ease in", "缓入", "加速")),
        ("ease_out_", localized("Ease out", "缓出", "減速")),
    ]
    .into_iter()
    .find_map(|(prefix, direction)| name.strip_prefix(prefix).map(|family| (direction, family)))
    .unwrap_or(("", name));
    let family = match family {
        "sine" => localized("Sine", "正弦", "サイン"),
        "quad" => localized("Quadratic", "二次", "2次"),
        "cubic" => localized("Cubic", "三次", "3次"),
        "quart" => localized("Quartic", "四次", "4次"),
        "quint" => localized("Quintic", "五次", "5次"),
        "expo" => localized("Exponential", "指数", "指数"),
        "circ" => localized("Circular", "圆形", "円"),
        "back" => localized("Back", "回退", "戻る"),
        "elastic" => localized("Elastic", "弹性", "弾性"),
        "bounce" => localized("Bounce", "回弹", "跳ね返り"),
        other => other,
    };
    if direction.is_empty() {
        family.to_owned()
    } else {
        format!("{family} {direction}")
    }
}

fn normalized_easing_filter(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

pub(super) fn easing_catalog_rows(
    query: &str,
    step_frames: i32,
    amplitude: f32,
    period: f32,
    points: &[f64],
) -> Vec<EasingCatalogRowData> {
    let names = keyframe_interpolation_names();
    let query = normalized_easing_filter(query);
    let mut rows = Vec::with_capacity(names.len() + EASING_CATEGORIES.len());
    for (category, category_names) in EASING_CATEGORIES {
        let matching = category_names
            .iter()
            .filter_map(|name| {
                let index = names.iter().position(|candidate| candidate == name)?;
                (query.is_empty() || normalized_easing_filter(name).contains(&query))
                    .then_some((index, *name))
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            continue;
        }
        rows.push(EasingCatalogRowData {
            header: true,
            easing_index: -1,
            name: SharedString::from(category),
            label: SharedString::from(easing_category_label(category)),
            preview_path: SharedString::new(),
        });
        rows.extend(matching.into_iter().map(|(index, name)| {
            let options = easing_options(name, step_frames, amplitude, period, points);
            EasingCatalogRowData {
                header: false,
                easing_index: index as i32,
                name: SharedString::from(name),
                label: SharedString::from(easing_label(name)),
                preview_path: SharedString::from(easing_preview_path_with_steps(&options, 48)),
            }
        }));
    }
    rows
}

pub(super) fn sync_easing_catalog(window: &EasingConfigWindow, query: &str, curve: &BezierCurve) {
    update_vec_model(
        &window.get_easing_catalog_rows(),
        easing_catalog_rows(
            query,
            window.get_step_frames(),
            window.get_elastic_amplitude(),
            window.get_elastic_period(),
            curve.points(),
        ),
    );
}

fn easing_category_label(category: &str) -> &'static str {
    match category {
        "Basic" => localized("Basic", "基础", "基本"),
        "Standard curves" => localized("Standard curves", "标准曲线", "標準カーブ"),
        "Strong curves" => localized("Strong curves", "强曲线", "強いカーブ"),
        "Bounce and elasticity" => localized("Bounce and elasticity", "回弹与弹性", "反動と弾性"),
        "Special" => localized("Special", "特殊", "特殊"),
        _ => "",
    }
}

pub(super) fn refresh_easing_translations(window: &EasingConfigWindow) {
    let names = window.get_easing_names();
    let labels = (0..names.row_count())
        .filter_map(|index| names.row_data(index))
        .map(|name| SharedString::from(easing_label(name.as_str())))
        .collect();
    update_vec_model(&window.get_easing_labels(), labels);
    let rows = window.get_easing_catalog_rows();
    let translated_rows = (0..rows.row_count())
        .filter_map(|index| rows.row_data(index))
        .map(|mut row| {
            row.label = if row.header {
                SharedString::from(easing_category_label(row.name.as_str()))
            } else {
                SharedString::from(easing_label(row.name.as_str()))
            };
            row
        })
        .collect();
    update_vec_model(&rows, translated_rows);
}

pub(super) fn sync_easing_window(
    window: &EasingConfigWindow,
    effect_index: usize,
    param_name: &str,
    point: &KeyframePoint,
) -> BezierCurve {
    let interpolation = if point.interpolation == "bezier" {
        "custom"
    } else {
        point.interpolation.as_str()
    };
    let names = keyframe_interpolation_names();
    let selected_index = names
        .iter()
        .position(|name| *name == interpolation)
        .map_or(0, |index| index as i32);
    let mode_params = point
        .options
        .get("modeParams")
        .and_then(serde_json::Value::as_object);
    let step_frames = mode_params
        .and_then(|params| params.get("stepFrames"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(1)
        .max(1);
    let amplitude = mode_params
        .and_then(|params| params.get("amplitude"))
        .and_then(serde_json::Value::as_f64)
        .map_or(1.0, |value| finite_f32(value, 1.0));
    let period = mode_params
        .and_then(|params| params.get("period"))
        .and_then(serde_json::Value::as_f64)
        .map_or(0.3, |value| finite_f32(value, 0.3));
    let custom_points = point
        .options
        .get("points")
        .and_then(serde_json::Value::as_array)
        .and_then(|points| {
            points
                .iter()
                .map(serde_json::Value::as_f64)
                .collect::<Option<Vec<_>>>()
        })
        .unwrap_or_default();
    let curve = BezierCurve::from_points(&custom_points);
    let controls = curve.first_controls();

    window.set_initializing(true);
    window.set_effect_index(effect_index as i32);
    window.set_param_name(SharedString::from(param_name));
    window.set_keyframe_frame(point.frame);
    window.set_selected_easing_index(selected_index);
    window.set_step_frames(step_frames);
    window.set_elastic_amplitude(amplitude);
    window.set_elastic_period(period);
    window.set_preview_scale(1.0);
    window.set_preview_offset_x(0.0);
    window.set_preview_offset_y(0.0);
    window.set_custom_x1(finite_f32(controls[0], 0.33));
    window.set_custom_y1(finite_f32(controls[1], 0.0));
    window.set_custom_x2(finite_f32(controls[2], 0.66));
    window.set_custom_y2(finite_f32(controls[3], 1.0));
    window.set_initializing(false);
    curve
}

pub(super) fn sync_easing_curve(window: &EasingConfigWindow, curve: &BezierCurve) {
    let controls = curve.first_controls();
    let initializing = window.get_initializing();
    window.set_initializing(true);
    window.set_custom_x1(finite_f32(controls[0], 0.33));
    window.set_custom_y1(finite_f32(controls[1], 0.0));
    window.set_custom_x2(finite_f32(controls[2], 0.66));
    window.set_custom_y2(finite_f32(controls[3], 1.0));
    window.set_initializing(initializing);
    sync_easing_preview(window, curve);
}

pub(super) fn invoke_current_easing(window: &EasingConfigWindow) {
    let interpolation = easing_name_at(window.get_selected_easing_index());
    window.invoke_apply_easing(
        SharedString::from(interpolation),
        window.get_step_frames(),
        window.get_elastic_amplitude(),
        window.get_elastic_period(),
        window.get_custom_x1(),
        window.get_custom_y1(),
        window.get_custom_x2(),
        window.get_custom_y2(),
    );
}

pub(super) fn sync_easing_preview(window: &EasingConfigWindow, curve: &BezierCurve) {
    let interpolation = easing_name_at(window.get_selected_easing_index());
    let options = easing_options(
        interpolation,
        window.get_step_frames(),
        window.get_elastic_amplitude(),
        window.get_elastic_period(),
        curve.points(),
    );
    window.set_preview_path(SharedString::from(easing_preview_path(&options)));
    window.set_tangent_path(SharedString::from(easing_tangent_path(curve)));
    update_vec_model(
        &window.get_curve_handles(),
        curve
            .handles()
            .into_iter()
            .map(|handle| CurveHandleData {
                index: handle.index as i32,
                x: finite_f32(handle.x, 0.0),
                y: finite_f32(handle.y, 0.0),
                anchor: handle.anchor,
                removable: handle.removable,
            })
            .collect(),
    );
}

pub(super) fn easing_preview_path(options: &serde_json::Value) -> String {
    easing_preview_path_with_steps(options, 128)
}

fn easing_preview_path_with_steps(options: &serde_json::Value, steps: i32) -> String {
    let mut samples = sample_easing_curve(options, steps).into_iter();
    let Some((first_x, first_y)) = samples.next() else {
        return String::new();
    };
    let mut path = format!("M {first_x} {}", 1.0 - first_y);
    for (x, y) in samples {
        path.push_str(&format!(" L {x} {}", 1.0 - y));
    }
    path
}

pub(super) fn easing_tangent_path(curve: &BezierCurve) -> String {
    let mut path = String::new();
    let mut previous = (0.0, 0.0);
    for points in curve.points().as_chunks::<6>().0 {
        path.push_str(&format!(
            " M {} {} L {} {} M {} {} L {} {}",
            previous.0,
            1.0 - previous.1,
            points[0],
            1.0 - points[1],
            points[4],
            1.0 - points[5],
            points[2],
            1.0 - points[3]
        ));
        previous = (points[4], points[5]);
    }
    path
}

pub(super) fn easing_options(
    interpolation: &str,
    step_frames: i32,
    amplitude: f32,
    period: f32,
    points: &[f64],
) -> serde_json::Value {
    let interpolation = keyframe_interpolation_names()
        .into_iter()
        .find(|name| *name == interpolation)
        .unwrap_or("none");
    let mut options = serde_json::Map::new();
    options.insert(
        "interp".to_owned(),
        serde_json::Value::String(interpolation.to_owned()),
    );
    if interpolation == "custom" {
        let mut points = if points.len() >= 6 && points.len().is_multiple_of(6) {
            points.to_vec()
        } else {
            vec![0.33, 0.0, 0.66, 1.0, 1.0, 1.0]
        };
        points[0] = points[0].clamp(0.0, 1.0);
        points[2] = points[2].clamp(0.0, 1.0);
        options.insert("points".to_owned(), serde_json::json!(points));
    } else if interpolation == "random" || interpolation == "alternate" {
        options.insert(
            "modeParams".to_owned(),
            serde_json::json!({"stepFrames": step_frames.max(1)}),
        );
    } else if interpolation.contains("elastic") {
        let amplitude = if amplitude.is_finite() {
            amplitude.clamp(0.1, 5.0)
        } else {
            1.0
        };
        let period = if period.is_finite() {
            period.clamp(0.05, 1.0)
        } else {
            0.3
        };
        options.insert(
            "modeParams".to_owned(),
            serde_json::json!({"amplitude": amplitude, "period": period}),
        );
    }
    serde_json::Value::Object(options)
}
