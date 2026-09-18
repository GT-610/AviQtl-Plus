//! Frontend behavior regression tests.

use crate::dialogs::{
    WindowGeometry, filtered_font_families, format_qt_color, parse_qt_color, qt_file_filters,
};
use crate::easing::{
    easing_catalog_rows, easing_label, easing_name_at, easing_options, easing_preview_path,
    easing_tangent_path, parameter_button_label,
};
use crate::export::{export_workspace_for_frame, selected_codec, selected_codec_index};
use crate::lifecycle::{RecentProject, merge_recent_project, recent_projects_from_value};
use crate::localization::{CURRENT_UI_LANGUAGE, UiLanguage};
use crate::localization::{localized_effect_metadata, ui_language_from_locale};
use crate::object_settings::{object_settings_rows, timeline_context_catalog_items};
use crate::playback::{
    audio_queue_lead_frames, samples_for_timeline_frame, scene_fps, stereo_levels,
};
use crate::settings::{
    SYSTEM_AUDIO_BLOCK_SIZES, SYSTEM_EXPORT_AUDIO_CODECS, SYSTEM_EXPORT_VIDEO_CODECS,
    SYSTEM_PLUGIN_FORMATS, SYSTEM_PREVIEW_RENDER_SCALES, SYSTEM_SHORTCUT_ROWS, SYSTEM_THEME_VALUES,
    SYSTEM_UI_LANGUAGE_VALUES, choice_f64, choice_i32, choice_str, plugin_paths_value,
};
use crate::shortcuts::{
    ShortcutInput, TimelineScrollInput, parse_shortcut, plan_timeline_scroll_with,
    scale_to_zoom_percent, shortcut_matches, zoom_percent_to_scale,
};
use aviqtl_app::audio_plugin::AudioPluginCatalog;
use aviqtl_app::easing::BezierCurve;
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::object_settings::{ObjectControl, ObjectSettings, keyframe_interpolation_names};
use aviqtl_app::{ApplicationModel, ProjectDefaults, ProjectSession};
use serde_json::json;
use slint::Model;
use slint::platform::Key;

use aviqtl_app::object_settings::{
    AudioPluginSettings, KeyframePoint, ObjectControlKind, ObjectControlOption, ObjectEffect,
};

#[test]
fn playback_audio_frame_sizes_follow_the_timeline_rate() {
    assert_eq!(samples_for_timeline_frame(0, 60.0, 48_000, 1.0), Some(800));
    assert_eq!(samples_for_timeline_frame(0, 60.0, 48_000, 2.0), Some(400));
    assert_eq!(
        samples_for_timeline_frame(0, 60.0, 48_000, 0.5),
        Some(1_600)
    );
    assert_eq!(samples_for_timeline_frame(-1, 60.0, 48_000, 1.0), None);
    assert_eq!(samples_for_timeline_frame(0, 0.0, 48_000, 1.0), None);
    assert_eq!(samples_for_timeline_frame(0, 60.0, 0, 1.0), None);
    assert_eq!(samples_for_timeline_frame(0, 60.0, 48_000, 0.0), None);
}

#[test]
fn audio_queue_lead_matches_the_buffered_transport_duration() {
    assert_eq!(audio_queue_lead_frames(60.0, 1.0, 6_000, 48_000), 8);
    assert_eq!(audio_queue_lead_frames(60.0, 2.0, 6_000, 48_000), 15);
    assert_eq!(audio_queue_lead_frames(240.0, 4.0, 6_000, 48_000), 120);
    assert_eq!(audio_queue_lead_frames(60.0, 1.0, 6_000, 0), 1);
}

#[test]
fn audio_timing_uses_scene_fps_with_a_project_fallback() {
    let mut project = ProjectSession::blank_with(ProjectDefaults {
        fps: 24.0,
        ..ProjectDefaults::default()
    });
    project.document.scenes[0].fps = 120.0;

    assert_eq!(scene_fps(&project.document, 1), 120.0);
    assert_eq!(scene_fps(&project.document, 999), 24.0);

    project.document.scenes[0].fps = 0.0;
    assert_eq!(scene_fps(&project.document, 1), 1.0);
}

#[test]
fn stereo_meter_reports_independent_peak_and_rms_levels() {
    let levels = stereo_levels(&[0.5, -1.0, -0.5, 0.0, 0.75]);
    assert_eq!(levels[0], 0.5);
    assert_eq!(levels[1], 1.0);
    assert!((levels[2] - 0.5).abs() < f32::EPSILON);
    assert!((levels[3] - (0.5_f32).sqrt()).abs() < f32::EPSILON);
    assert_eq!(stereo_levels(&[]), [0.0; 4]);
}

#[test]
fn recent_projects_parse_qt_entries_and_respect_the_configured_limit() {
    let value = json!([
        {"name":"One","path":"C:/projects/one.aviqtl","width":1920,"height":1080,"fps":60.0},
        {"name":"","path":"C:/projects/two.aviqtl","width":1280,"height":720,"fps":30.0},
        {"name":"Invalid","path":""}
    ]);

    let recent = recent_projects_from_value(Some(&value), 2);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].name, "One");
    assert_eq!(recent[1].name, "two.aviqtl");
    assert_eq!(recent[1].fps, 30.0);
}

#[test]
fn recent_project_paths_preserve_surrounding_whitespace() {
    let value = json!([
        {"path":"  C:/projects/with-space.aviqtl  "},
        {"path":" \t "}
    ]);

    let recent = recent_projects_from_value(Some(&value), 10);

    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].path, "  C:/projects/with-space.aviqtl  ");
}

#[test]
fn recent_project_updates_move_duplicates_to_the_front_atomically() {
    let one = RecentProject {
        name: "One".to_owned(),
        path: "one.aviqtl".to_owned(),
        width: 1920,
        height: 1080,
        fps: 60.0,
    };
    let two = RecentProject {
        name: "Two".to_owned(),
        path: "two.aviqtl".to_owned(),
        width: 1280,
        height: 720,
        fps: 30.0,
    };
    let updated = merge_recent_project(vec![one.clone(), two.clone()], two.clone(), 2);

    assert_eq!(updated, [two, one]);
}

#[test]
fn window_geometry_uses_qt_keys_and_clamps_invalid_sizes() {
    let fallback = WindowGeometry::new(100, 200, 640, 480);
    let value = json!({
        "x": -120,
        "y": 45,
        "width": 1280,
        "height": 720,
        "maximized": true
    });
    assert_eq!(
        WindowGeometry::from_value(Some(&value), fallback),
        WindowGeometry {
            x: -120,
            y: 45,
            width: 1280,
            height: 720,
            maximized: true,
        }
    );

    let invalid = json!({"width": 0, "height": -5});
    assert_eq!(
        WindowGeometry::from_value(Some(&invalid), fallback),
        WindowGeometry {
            width: 1,
            height: 1,
            ..fallback
        }
    );
    assert_eq!(WindowGeometry::from_value(None, fallback), fallback);
}

#[test]
fn system_settings_choices_and_plugin_paths_match_the_qt_schema() {
    assert_eq!(choice_str(&SYSTEM_THEME_VALUES, 2, 0), "System");
    assert_eq!(
        choice_str(&SYSTEM_UI_LANGUAGE_VALUES, 2, 0),
        "SimplifiedChinese"
    );
    assert_eq!(choice_str(&SYSTEM_EXPORT_VIDEO_CODECS, 1, 2), "hevc_vaapi");
    assert_eq!(choice_str(&SYSTEM_EXPORT_AUDIO_CODECS, 1, 0), "opus");
    assert_eq!(choice_i32(&SYSTEM_AUDIO_BLOCK_SIZES, 5, 4), 8192);
    assert_eq!(choice_f64(&SYSTEM_PREVIEW_RENDER_SCALES, 3, 0), 0.25);
    assert_eq!(choice_str(&SYSTEM_THEME_VALUES, -1, 0), "Dark");
    assert_eq!(choice_i32(&SYSTEM_AUDIO_BLOCK_SIZES, 99, 4), 4096);
    assert_eq!(
        plugin_paths_value(" /plugins/one \n\n/plugins/two "),
        json!(["/plugins/one", "/plugins/two"])
    );
    assert_eq!(SYSTEM_PLUGIN_FORMATS.len(), 11);
    assert_eq!(SYSTEM_SHORTCUT_ROWS.len(), 34);
}

#[test]
fn ui_locale_selection_uses_english_as_the_fallback() {
    assert_eq!(
        ui_language_from_locale("zh-CN"),
        UiLanguage::SimplifiedChinese
    );
    assert_eq!(
        ui_language_from_locale("zh_Hans_CN.UTF-8"),
        UiLanguage::SimplifiedChinese
    );
    assert_eq!(ui_language_from_locale("ja-JP"), UiLanguage::Japanese);
    assert_eq!(ui_language_from_locale("en-US"), UiLanguage::English);
    assert_eq!(ui_language_from_locale("fr-FR"), UiLanguage::English);
    assert_eq!(ui_language_from_locale(""), UiLanguage::English);
}

#[test]
fn effect_metadata_follows_the_selected_ui_language() {
    CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::English));
    assert_eq!(localized_effect_metadata("テキスト"), "Text");
    assert_eq!(localized_effect_metadata("変形/クロップ"), "Transform/Crop");
    assert_eq!(localized_effect_metadata("標準描画"), "Standard Drawing");
    assert_eq!(localized_effect_metadata("拡大率"), "Scale");
    assert_eq!(localized_effect_metadata("X軸回転"), "X Rotation");
    assert_eq!(localized_effect_metadata("中心X"), "Center X");
    assert_eq!(localized_effect_metadata("不透明度"), "Opacity");
    assert_eq!(localized_effect_metadata("合成モード"), "Blend Mode");
    assert_eq!(localized_effect_metadata("通常"), "Normal");

    CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::SimplifiedChinese));
    assert_eq!(localized_effect_metadata("テキスト"), "文本");
    assert_eq!(localized_effect_metadata("変形/クロップ"), "变形/裁剪");
    assert_eq!(localized_effect_metadata("標準描画"), "标准绘制");
    assert_eq!(localized_effect_metadata("拡大率"), "缩放率");
    assert_eq!(localized_effect_metadata("X軸回転"), "X 轴旋转");
    assert_eq!(localized_effect_metadata("中心X"), "中心 X");
    assert_eq!(localized_effect_metadata("不透明度"), "不透明度");
    assert_eq!(localized_effect_metadata("合成モード"), "混合模式");
    assert_eq!(localized_effect_metadata("通常"), "正常");

    CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::Japanese));
    assert_eq!(localized_effect_metadata("テキスト"), "テキスト");
    assert_eq!(localized_effect_metadata("変形/クロップ"), "変形/クロップ");
    assert_eq!(localized_effect_metadata("標準描画"), "標準描画");
}

#[test]
fn dual_slider_button_uses_the_localized_property_and_easing_names() {
    CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::English));
    assert_eq!(
        parameter_button_label(&localized_effect_metadata("拡大率"), true, "linear"),
        "Scale (Linear)"
    );

    CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::SimplifiedChinese));
    assert_eq!(
        parameter_button_label(&localized_effect_metadata("拡大率"), true, "linear"),
        "缩放率 (线性)"
    );

    CURRENT_UI_LANGUAGE.with(|language| language.set(UiLanguage::Japanese));
    assert_eq!(
        parameter_button_label(&localized_effect_metadata("拡大率"), true, "linear"),
        "拡大率 (直線)"
    );
}

#[test]
fn searchable_context_catalog_preserves_qt_category_paths() {
    let (catalog, _) = EffectCatalog::load();
    let items =
        timeline_context_catalog_items(&catalog, &AudioPluginCatalog::default(), "clipping", 1);
    let clipping = items
        .iter()
        .find(|item| item.id.as_str() == "clipping")
        .expect("clipping effect remains searchable by technical id");
    assert!(clipping.categories.as_str().contains("Transform/Crop"));
}

#[test]
fn codec_selection_matches_the_qt_fallback_contract() {
    let codecs = vec!["libx264".to_owned(), "libx265".to_owned()];
    assert_eq!(selected_codec_index(&codecs, "libx265"), 1);
    assert_eq!(selected_codec_index(&codecs, "missing"), 0);
    assert_eq!(selected_codec_index(&[], "libx264"), -1);
    assert_eq!(selected_codec(&codecs, 1, "fallback"), "libx265");
    assert_eq!(selected_codec(&codecs, -1, "fallback"), "fallback");
    assert_eq!(selected_codec(&codecs, 9, "fallback"), "fallback");
}

#[test]
fn export_frame_gate_rejects_a_project_tab_change() {
    let mut model = ApplicationModel::default();
    model.create_project(ProjectDefaults::default());
    let export_project = model
        .current_project_instance_id()
        .expect("first project exists");
    model.create_project(ProjectDefaults::default());

    let error = match export_workspace_for_frame(&mut model, export_project) {
        Ok(_) => panic!("another project must not render the active export frame"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        "Frame render error: project tab changed during export"
    );
    assert!(model.select_project(0));
    assert!(export_workspace_for_frame(&mut model, export_project).is_ok());
}

#[test]
fn shortcut_parser_preserves_qt_cross_platform_names_and_punctuation() {
    let undo = parse_shortcut("Ctrl+Shift+Z").expect("undo shortcut parses");
    assert!(undo.control);
    assert!(undo.shift);
    assert_eq!(undo.text, "z");

    let settings = parse_shortcut("Alt+Enter").expect("settings shortcut parses");
    assert!(settings.alt);
    assert_eq!(settings.text, char::from(Key::Return).to_string());

    let zoom = parse_shortcut("Ctrl++").expect("plus shortcut parses");
    assert!(zoom.control);
    assert!(zoom.ignore_shift);
    assert_eq!(zoom.text, "+");
    assert!(parse_shortcut("Ctrl+UnknownKey").is_none());
}

#[test]
fn shortcut_matching_uses_slint_command_and_physical_control_semantics() {
    let command_save = ShortcutInput {
        text: "s".to_owned(),
        alt: false,
        control: true,
        shift: false,
        meta: false,
    };
    assert!(shortcut_matches("Ctrl+S", &command_save));

    let physical_control_save = ShortcutInput {
        control: false,
        meta: true,
        ..command_save.clone()
    };
    assert!(shortcut_matches("Meta+S", &physical_control_save));
    if cfg!(target_os = "macos") {
        assert!(shortcut_matches("Command+S", &command_save));
        assert!(!shortcut_matches("Control+S", &command_save));
        assert!(shortcut_matches("Control+S", &physical_control_save));
    } else {
        assert!(shortcut_matches("Control+S", &command_save));
        assert!(!shortcut_matches("Command+S", &command_save));
        assert!(shortcut_matches("Command+S", &physical_control_save));
    }
}

#[test]
fn easing_options_match_the_qt_parameter_contracts() {
    assert_eq!(easing_label("ease_in_out_elastic"), "Elastic Ease in/out");
    assert_eq!(easing_label("ease_out_sine"), "Sine Ease out");
    assert_eq!(easing_label("ease_in_quad"), "Quadratic Ease in");
    assert_eq!(easing_label("ease_out_in_circ"), "Circular Ease out/in");
    assert_eq!(easing_label("ease_in_bounce"), "Bounce Ease in");
    assert_eq!(easing_name_at(0), "none");
    assert_eq!(easing_name_at(-1), "none");

    let random = easing_options("random", 0, 1.0, 0.3, &[0.33, 0.0, 0.66, 1.0, 1.0, 1.0]);
    assert_eq!(random["interp"], "random");
    assert_eq!(random["modeParams"]["stepFrames"], 1);

    let elastic = easing_options(
        "ease_out_elastic",
        1,
        f32::INFINITY,
        0.0,
        &[0.33, 0.0, 0.66, 1.0, 1.0, 1.0],
    );
    assert_eq!(elastic["modeParams"]["amplitude"], 1.0);
    let period = elastic["modeParams"]["period"]
        .as_f64()
        .expect("elastic period is numeric");
    assert!((period - 0.05).abs() < f64::from(f32::EPSILON));

    let custom = easing_options(
        "custom",
        1,
        1.0,
        0.3,
        &[-2.0, -1.0, 3.0, 2.0, 0.5, 0.5, 0.6, 0.6, 0.8, 0.8, 1.0, 1.0],
    );
    assert_eq!(
        custom["points"],
        json!([0.0, -1.0, 1.0, 2.0, 0.5, 0.5, 0.6, 0.6, 0.8, 0.8, 1.0, 1.0])
    );
    assert_eq!(
        easing_options("unknown", 1, 1.0, 0.3, &[0.0; 6])["interp"],
        "none"
    );

    let linear = easing_preview_path(&easing_options(
        "linear",
        1,
        1.0,
        0.3,
        BezierCurve::default().points(),
    ));
    assert!(linear.starts_with("M 0 1"));
    assert!(linear.contains(" L 0.5 0.5"));
    assert!(linear.ends_with(" L 1 0"));

    let mut curve = BezierCurve::default();
    assert!(curve.insert_anchor(0.5, 0.25));
    let tangents = easing_tangent_path(&curve);
    assert!(tangents.contains("M 0 1 L 0.165 0.9175"));
    assert!(tangents.contains("M 0.5 0.75 L 0.33 0.835"));
}

#[test]
fn easing_catalog_matches_the_qt_categories_and_filtering() {
    let curve = BezierCurve::default();
    let rows = easing_catalog_rows("", 1, 1.0, 0.3, curve.points());
    assert_eq!(
        rows.iter()
            .filter(|row| row.header)
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Basic",
            "Standard curves",
            "Strong curves",
            "Bounce and elasticity",
            "Special"
        ]
    );

    let mut actual_names = rows
        .iter()
        .filter(|row| !row.header)
        .map(|row| row.name.as_str().to_owned())
        .collect::<Vec<_>>();
    let mut expected_names = keyframe_interpolation_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    actual_names.sort();
    expected_names.sort();
    assert_eq!(actual_names, expected_names);
    assert!(rows.iter().filter(|row| !row.header).all(|row| {
        easing_name_at(row.easing_index) == row.name.as_str()
            && row.preview_path.as_str().starts_with("M 0 1")
    }));

    let bounce = easing_catalog_rows("Bo_Un_Ce", 1, 1.0, 0.3, curve.points());
    assert_eq!(bounce.len(), 5);
    assert!(bounce[0].header);
    assert_eq!(bounce[0].label.as_str(), "Bounce and elasticity");
    assert!(
        bounce[1..]
            .iter()
            .all(|row| row.name.as_str().contains("bounce"))
    );
    assert!(easing_catalog_rows("跳ね返り", 1, 1.0, 0.3, curve.points()).is_empty());
}

#[test]
fn easing_catalog_previews_follow_live_mode_parameters() {
    let curve = BezierCurve::default();
    let random_preview = |step_frames| {
        easing_catalog_rows("random", step_frames, 1.0, 0.3, curve.points())[1]
            .preview_path
            .to_string()
    };
    assert_ne!(random_preview(1), random_preview(4));

    let custom_default = easing_catalog_rows("custom", 1, 1.0, 0.3, curve.points())[1]
        .preview_path
        .to_string();
    let mut edited = curve.clone();
    assert!(edited.set_first_controls([0.1, 0.8, 0.9, 0.2]));
    let custom_edited = easing_catalog_rows("custom", 1, 1.0, 0.3, edited.points())[1]
        .preview_path
        .to_string();
    assert_ne!(custom_default, custom_edited);
}

#[test]
fn parameter_picker_helpers_preserve_qt_value_contracts() {
    assert_eq!(
        qt_file_filters("Video Files (*.mp4 *.mov);;Images (*.png *.jpg)"),
        vec![
            (
                "Video Files".to_owned(),
                vec!["mp4".to_owned(), "mov".to_owned()]
            ),
            (
                "Images".to_owned(),
                vec!["png".to_owned(), "jpg".to_owned()]
            )
        ]
    );
    assert!(qt_file_filters("").is_empty());
    assert_eq!(parse_qt_color("#abc"), [255, 0xaa, 0xbb, 0xcc]);
    assert_eq!(parse_qt_color("#80ff0000"), [0x80, 0xff, 0, 0]);
    assert_eq!(parse_qt_color("invalid"), [255; 4]);
    assert_eq!(format_qt_color(255, 0, 16, 255), "#ff0010");
    assert_eq!(format_qt_color(300, -1, 16, 128), "#80ff0010");
}

#[test]
fn font_picker_filter_is_case_insensitive_and_stable() {
    let families = vec![
        "Arial".to_owned(),
        "Noto Sans CJK JP".to_owned(),
        "Noto Serif".to_owned(),
    ];
    assert_eq!(
        filtered_font_families(&families, "  NOTO ")
            .into_iter()
            .map(|family| family.to_string())
            .collect::<Vec<_>>(),
        vec!["Noto Sans CJK JP", "Noto Serif"]
    );
    assert_eq!(filtered_font_families(&families, "").len(), 3);
}

#[test]
fn zoom_mapping_matches_the_qt_piecewise_scale() {
    assert_eq!(zoom_percent_to_scale(10.0), 0.1);
    assert_eq!(zoom_percent_to_scale(100.0), 1.0);
    assert_eq!(zoom_percent_to_scale(200.0), 4.0);
    assert_eq!(zoom_percent_to_scale(400.0), 10.0);
    assert!((scale_to_zoom_percent(4.0) - 200.0).abs() < f32::EPSILON);
}

fn timeline_scroll_input() -> TimelineScrollInput {
    TimelineScrollInput {
        delta_x: 20.0,
        delta_y: 120.0,
        content_x: 500.0,
        viewport_x: -300.0,
        viewport_y: -90.0,
        visible_width: 800.0,
        viewport_width: 3_600.0,
        visible_height: 300.0,
        viewport_height: 3_840.0,
        duration_frames: 3_600.0,
        alt: false,
        control: false,
        shift: false,
        ruler_zoom: false,
        pixels_per_frame: 1.0,
    }
}

#[test]
fn timeline_wheel_matches_qt_axis_and_modifier_routing() {
    let zoom = (10.0, 400.0, 10.0);
    let horizontal = plan_timeline_scroll_with(timeline_scroll_input(), zoom);
    assert_eq!(horizontal.viewport_x, -180.0);
    assert_eq!(horizontal.viewport_y, -90.0);

    let vertical = plan_timeline_scroll_with(
        TimelineScrollInput {
            shift: true,
            ..timeline_scroll_input()
        },
        zoom,
    );
    assert_eq!(vertical.viewport_x, -300.0);
    assert_eq!(vertical.viewport_y, 0.0);
}

#[test]
fn timeline_wheel_zoom_preserves_the_pointer_anchor() {
    let zoom = (10.0, 400.0, 10.0);
    let stepped = plan_timeline_scroll_with(
        TimelineScrollInput {
            control: true,
            ..timeline_scroll_input()
        },
        zoom,
    );
    assert!((stepped.pixels_per_frame - 1.3).abs() < f32::EPSILON);
    assert!((stepped.viewport_x + 450.0).abs() < f32::EPSILON);

    let ruler = plan_timeline_scroll_with(
        TimelineScrollInput {
            ruler_zoom: true,
            ..timeline_scroll_input()
        },
        zoom,
    );
    assert!((ruler.pixels_per_frame - 1.1).abs() < f32::EPSILON);
    assert!((ruler.viewport_x + 350.0).abs() < f32::EPSILON);
}

#[test]
fn object_setting_rows_keep_effect_control_and_option_order() {
    let settings = ObjectSettings {
        clip_id: 7,
        clip_label: "Rectangle".to_owned(),
        audio_plugin_mode: false,
        effects: vec![ObjectEffect {
            index: 1,
            id: "blur".to_owned(),
            name: "Blur".to_owned(),
            enabled: false,
            selected: true,
            removable: true,
            controls: vec![
                ObjectControl {
                    kind: ObjectControlKind::Header,
                    source_kind: "header".to_owned(),
                    param: None,
                    label: "Quality".to_owned(),
                    minimum: None,
                    maximum: None,
                    step: None,
                    decimals: None,
                    unit: String::new(),
                    filter: String::new(),
                    disabled: false,
                    keyframed: false,
                    value: serde_json::Value::Null,
                    relative_frame: 0,
                    clip_duration: 100,
                    interval_start: 0,
                    interval_end: 100,
                    start_value: serde_json::Value::Null,
                    end_value: serde_json::Value::Null,
                    start_interpolation: "constant".to_owned(),
                    keyframes: Vec::new(),
                    options: Vec::new(),
                },
                ObjectControl {
                    kind: ObjectControlKind::Choice,
                    source_kind: "enum".to_owned(),
                    param: Some("mode".to_owned()),
                    label: "Mode".to_owned(),
                    minimum: None,
                    maximum: None,
                    step: None,
                    decimals: None,
                    unit: String::new(),
                    filter: String::new(),
                    disabled: false,
                    keyframed: true,
                    value: json!("high"),
                    relative_frame: 10,
                    clip_duration: 100,
                    interval_start: 0,
                    interval_end: 100,
                    start_value: json!("high"),
                    end_value: json!("high"),
                    start_interpolation: "linear".to_owned(),
                    keyframes: Vec::new(),
                    options: vec![
                        ObjectControlOption {
                            value: json!("low"),
                            label: "Low".to_owned(),
                        },
                        ObjectControlOption {
                            value: json!("high"),
                            label: "High".to_owned(),
                        },
                    ],
                },
                ObjectControl {
                    kind: ObjectControlKind::Number,
                    source_kind: "slider".to_owned(),
                    param: Some("size".to_owned()),
                    label: "Size".to_owned(),
                    minimum: Some(0.0),
                    maximum: Some(100.0),
                    step: Some(1.0),
                    decimals: Some(1),
                    unit: "px".to_owned(),
                    filter: String::new(),
                    disabled: false,
                    keyframed: true,
                    value: json!(10.0),
                    relative_frame: 10,
                    clip_duration: 100,
                    interval_start: 0,
                    interval_end: 20,
                    start_value: json!(0.0),
                    end_value: json!(20.0),
                    start_interpolation: "linear".to_owned(),
                    keyframes: vec![
                        KeyframePoint {
                            frame: 0,
                            value: json!(0.0),
                            interpolation: "linear".to_owned(),
                            options: json!({"interp":"linear"}),
                        },
                        KeyframePoint {
                            frame: 20,
                            value: json!(20.0),
                            interpolation: "ease_in".to_owned(),
                            options: json!({"interp":"ease_in"}),
                        },
                    ],
                    options: Vec::new(),
                },
            ],
        }],
        audio_plugins: Vec::new(),
    };

    let rows = object_settings_rows(&settings);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].row_kind.as_str(), "effect");
    assert_eq!(rows[1].row_kind.as_str(), "header");
    assert_eq!(rows[2].row_kind.as_str(), "choice");
    assert_eq!(rows[2].selected_option, 1);
    assert!(rows[2].keyframed);
    assert_eq!(rows[2].option_labels.row_count(), 2);
    assert_eq!(rows[2].option_labels.row_data(0).as_deref(), Some("Low"));
    assert_eq!(rows[2].option_labels.row_data(1).as_deref(), Some("High"));
    assert_eq!(rows[3].row_kind.as_str(), "number");
    assert!(rows[3].range_mode);
    assert_eq!((rows[3].start_frame, rows[3].end_frame), (0, 20));
    assert_eq!(rows[3].text_value.as_str(), "0.0");
    assert_eq!(rows[3].end_text_value.as_str(), "20.0");
    assert_eq!(rows[4].row_kind.as_str(), "keyframes");
    assert_eq!(rows[4].keyframe_markers.row_count(), 3);
    assert!(!rows[4].keyframe_markers.row_data(0).unwrap().draggable);
    assert!(rows[4].keyframe_markers.row_data(1).unwrap().draggable);
    let virtual_end = rows[4].keyframe_markers.row_data(2).unwrap();
    assert_eq!(virtual_end.frame, 100);
    assert!(virtual_end.virtual_end);
}

#[test]
fn audio_plugin_rows_use_current_values_and_protect_qt_endpoints() {
    let control = ObjectControl {
        kind: ObjectControlKind::Number,
        source_kind: "slider".to_owned(),
        param: Some("0".to_owned()),
        label: "Gain".to_owned(),
        minimum: Some(0.0),
        maximum: Some(1.0),
        step: Some(0.01),
        decimals: Some(3),
        unit: "dB".to_owned(),
        filter: String::new(),
        disabled: false,
        keyframed: true,
        value: json!(0.5),
        relative_frame: 50,
        clip_duration: 100,
        interval_start: 20,
        interval_end: 80,
        start_value: json!(0.2),
        end_value: json!(0.8),
        start_interpolation: "linear".to_owned(),
        keyframes: vec![
            KeyframePoint {
                frame: 0,
                value: json!(0.0),
                interpolation: "linear".to_owned(),
                options: json!({"interp":"linear"}),
            },
            KeyframePoint {
                frame: 50,
                value: json!(0.5),
                interpolation: "linear".to_owned(),
                options: json!({"interp":"linear"}),
            },
            KeyframePoint {
                frame: 100,
                value: json!(1.0),
                interpolation: "linear".to_owned(),
                options: json!({"interp":"linear"}),
            },
        ],
        options: Vec::new(),
    };
    let rows = object_settings_rows(&ObjectSettings {
        clip_id: 9,
        clip_label: "Audio".to_owned(),
        audio_plugin_mode: true,
        effects: Vec::new(),
        audio_plugins: vec![AudioPluginSettings {
            index: 0,
            id: "gain".to_owned(),
            name: "Gain".to_owned(),
            format: "CLAP".to_owned(),
            enabled: true,
            selected: true,
            controls: vec![control],
        }],
    });

    assert_eq!(rows.len(), 3);
    assert!(rows[0].audio_plugin);
    assert!(!rows[0].header_toggle_visible);
    assert_eq!(rows[1].start_frame, 50);
    assert_eq!(rows[1].text_value.as_str(), "0.500");
    assert!(!rows[1].range_mode);
    assert!(rows[1].interactive);
    let markers = &rows[2].keyframe_markers;
    assert_eq!(markers.row_count(), 3);
    assert!(!markers.row_data(0).unwrap().removable);
    assert!(markers.row_data(1).unwrap().removable);
    assert!(!markers.row_data(1).unwrap().draggable);
    assert!(!markers.row_data(2).unwrap().removable);
}
