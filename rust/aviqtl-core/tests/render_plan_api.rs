#![recursion_limit = "256"]

use aviqtl_rust_core::api::{
    MediaKind, SceneRenderPlan, ShapeGradientKind, ShapeKind, TextAlignment, TimelineState,
};
use serde_json::json;

fn project_document() -> aviqtl_rust_core::api::ProjectDocument {
    let json = serde_json::to_vec(&json!({
        "version": 3,
        "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
        "scenes": [
            {"id": 1, "name": "Root", "width": 1280, "height": 720, "fps": 60.0, "duration": 300},
            {"id": 2, "name": "Nested", "width": 640, "height": 360, "fps": 30.0, "duration": 90}
        ],
        "clips": [{
            "id": 10,
            "sceneId": 1,
            "type": "video",
            "start": 10,
            "duration": 100,
            "layer": 3,
            "effects": [{
                "id": "video",
                "name": "Video",
                "enabled": true,
                "params": {
                    "path": "normal.mkv",
                    "opacity": 0.8,
                    "playMode": "normal",
                    "startFrame": 30.0,
                    "speed": 200.0,
                    "directFrame": 0.0
                }
            }, {
                "id": "transform",
                "name": "Transform",
                "enabled": true,
                "params": {
                    "x": 0.0,
                    "y": 4.0,
                    "rotationZ": 0.0,
                    "scale": 100.0,
                    "opacity": 1.0
                },
                "keyframes": {
                    "x": {
                        "start": {"frame": 0, "value": 0.0, "interp": "linear"},
                        "points": [{"frame": 100, "value": 100.0, "interp": "linear"}]
                    },
                    "rotationZ": {
                        "start": {"frame": 0, "value": 0.0, "interp": "linear"},
                        "points": [{"frame": 100, "value": 90.0, "interp": "linear"}]
                    },
                    "scale": {
                        "start": {"frame": 0, "value": 100.0, "interp": "linear"},
                        "points": [{"frame": 100, "value": 200.0, "interp": "linear"}]
                    },
                    "opacity": {
                        "start": {"frame": 0, "value": 1.0, "interp": "linear"},
                        "points": [{"frame": 100, "value": 0.5, "interp": "linear"}]
                    }
                }
            }]
        }, {
            "id": 11,
            "sceneId": 1,
            "type": "video",
            "start": 0,
            "duration": 100,
            "layer": 4,
            "effects": [{
                "id": "video",
                "name": "Video",
                "enabled": true,
                "params": {
                    "path": "direct.mkv",
                    "playMode": "direct",
                    "directFrame": 0.0
                },
                "keyframes": {
                    "directFrame": {
                        "start": {"frame": 0, "value": 0.0, "interp": "linear"},
                        "points": [{"frame": 100, "value": 60.0, "interp": "linear"}]
                    }
                }
            }]
        }, {
            "id": 12,
            "sceneId": 1,
            "type": "audio",
            "start": 0,
            "duration": 100,
            "layer": 5,
            "effects": [{
                "id": "audio",
                "name": "Audio",
                "enabled": true,
                "params": {
                    "source": "voice.wav",
                    "playMode": "direct",
                    "directTime": 2.5,
                    "volume": 0.5,
                    "pan": -0.25,
                    "limiter": true
                }
            }]
        }, {
            "id": 13,
            "sceneId": 1,
            "type": "rect",
            "start": 0,
            "duration": 100,
            "layer": 6,
            "effects": [{
                "id": "rect",
                "name": "Shape",
                "enabled": true,
                "params": {
                    "shapeType": "star",
                    "sizeW": 320.0,
                    "sizeH": 180.0,
                    "sides": 7,
                    "cornerRadius": 12.0,
                    "innerRadius": 37.5,
                    "rotation": 15.0,
                    "color": "#80402010",
                    "useGradient": true,
                    "gradientColor2": "#ffeeddcc",
                    "gradientType": 1,
                    "strokeColor": "#112233",
                    "strokeWidth": 6.0,
                    "dashLength": 8.0,
                    "dashSpace": 3.0,
                    "opacity": 0.6
                }
            }]
        }, {
            "id": 14,
            "sceneId": 1,
            "type": "text",
            "start": 0,
            "duration": 100,
            "layer": 7,
            "effects": [{
                "id": "text",
                "name": "Text",
                "enabled": true,
                "params": {
                    "text": "Rust\nテキスト",
                    "fontFamily": "sans-serif",
                    "fontSize": 64.0,
                    "fontBold": true,
                    "fontItalic": true,
                    "letterSpacing": 3.0,
                    "lineSpacing": 80.0,
                    "alignment": 2,
                    "color": "#80402010",
                    "outlineEnabled": true,
                    "outlineColor": "#ff112233",
                    "outlineWidth": 4.0,
                    "shadowEnabled": true,
                    "shadowColor": "#40102030",
                    "shadowOffsetX": -8.0,
                    "shadowOffsetY": 6.0,
                    "bgEnabled": true,
                    "backgroundColor": "#c0010203",
                    "backgroundRadius": 12.0,
                    "backgroundPaddingX": 24.0,
                    "backgroundPaddingY": 16.0
                }
            }]
        }, {
            "id": 15,
            "sceneId": 1,
            "type": "scene",
            "start": 0,
            "duration": 100,
            "layer": 8,
            "effects": [{
                "id": "scene",
                "name": "Scene",
                "enabled": true,
                "params": {
                    "targetSceneId": 2,
                    "speed": 1.5,
                    "offset": 10,
                    "opacity": 0.75
                }
            }]
        }]
    }))
    .expect("fixture serializes");
    TimelineState::from_json(&json)
        .expect("fixture is a valid project")
        .snapshot()
}

#[test]
fn evaluates_keyframed_layers_and_normal_and_direct_video_positions() {
    let document = project_document();
    let mut plan = SceneRenderPlan::from_document(&document, 1).expect("scene plan builds");
    assert_eq!(plan.effect_count(), 7);

    let frame = plan.evaluate(60);
    assert_eq!(frame.frame, 60);
    assert_eq!(frame.layers.len(), 6);
    assert_eq!(frame.audio.len(), 3);

    let normal = frame
        .layers
        .iter()
        .find(|layer| layer.clip_id == 10)
        .expect("normal video layer");
    assert_eq!(normal.relative_frame, 50);
    assert_eq!(normal.transform.x, 50.0);
    assert_eq!(normal.transform.y, 4.0);
    assert_eq!(normal.transform.rotation_z, 45.0);
    assert_eq!(normal.transform.scale_percent, 150.0);
    assert_eq!(normal.transform.opacity, 0.75);
    let normal_media = normal.media.as_ref().expect("normal media plan");
    assert_eq!(normal_media.kind, MediaKind::Video);
    assert_eq!(normal_media.path, "normal.mkv");
    assert!(!normal_media.direct_mode);
    assert!((normal_media.timestamp_seconds(30.0) - (13.0 / 3.0)).abs() < 1e-9);
    assert_eq!(
        normal
            .effects
            .iter()
            .find(|effect| effect.id == "transform")
            .expect("transform effect")
            .params["x"],
        json!(50.0)
    );

    let direct = frame
        .layers
        .iter()
        .find(|layer| layer.clip_id == 11)
        .expect("direct video layer")
        .media
        .as_ref()
        .expect("direct media plan");
    assert!(direct.direct_mode);
    assert!((direct.direct_frame - 36.0).abs() < 1e-9);
    assert!((direct.timestamp_seconds(30.0) - 1.2).abs() < 1e-9);

    let audio = frame
        .audio
        .iter()
        .find(|audio| audio.clip_id == 12)
        .expect("audio layer plan");
    assert!(audio.direct_mode);
    assert_eq!(audio.source_path.as_deref(), Some("voice.wav"));
    assert_eq!(audio.direct_time, 2.5);
    assert_eq!(audio.volume, 0.5);
    assert_eq!(audio.pan, -0.25);
    assert!(audio.limiter);

    let video_audio = frame
        .audio
        .iter()
        .find(|audio| audio.clip_id == 10)
        .expect("video audio layer plan");
    assert_eq!(video_audio.source_path.as_deref(), Some("normal.mkv"));

    let shape = frame
        .layers
        .iter()
        .find(|layer| layer.clip_id == 13)
        .and_then(|layer| layer.shape.as_ref())
        .expect("shape plan");
    assert_eq!(shape.kind, ShapeKind::Star);
    assert_eq!(shape.width, 320.0);
    assert_eq!(shape.height, 180.0);
    assert_eq!(shape.sides, 7);
    assert_eq!(shape.inner_radius_percent, 37.5);
    assert_eq!(shape.fill_color.alpha, 0x80);
    assert_eq!(shape.fill_color.red, 0x40);
    assert!(shape.use_gradient);
    assert_eq!(shape.gradient_kind, ShapeGradientKind::Radial);
    assert_eq!(shape.stroke_color.blue, 0x33);
    assert_eq!(shape.dash_length, 8.0);
    assert_eq!(shape.opacity, 0.6);

    let text = frame
        .layers
        .iter()
        .find(|layer| layer.clip_id == 14)
        .and_then(|layer| layer.text.as_ref())
        .expect("text plan");
    assert_eq!(text.content, "Rust\nテキスト");
    assert_eq!(text.font_family, "sans-serif");
    assert_eq!(text.font_size, 64.0);
    assert!(text.bold);
    assert!(text.italic);
    assert_eq!(text.letter_spacing, 3.0);
    assert_eq!(text.line_spacing, 80.0);
    assert_eq!(text.alignment, TextAlignment::Right);
    assert_eq!(text.color.alpha, 0x80);
    assert_eq!(text.color.red, 0x40);
    assert!(text.outline_enabled);
    assert_eq!(text.outline_color.blue, 0x33);
    assert_eq!(text.outline_width, 4.0);
    assert!(text.shadow_enabled);
    assert_eq!(text.shadow_offset_x, -8.0);
    assert_eq!(text.shadow_offset_y, 6.0);
    assert!(text.background_enabled);
    assert_eq!(text.background_color.alpha, 0xc0);
    assert_eq!(text.background_radius, 12.0);
    assert_eq!(text.background_padding_x, 24.0);
    assert_eq!(text.background_padding_y, 16.0);

    let nested = frame
        .layers
        .iter()
        .find(|layer| layer.clip_id == 15)
        .and_then(|layer| layer.nested_scene)
        .expect("nested scene plan");
    assert_eq!(nested.target_scene_id, 2);
    assert_eq!(nested.target_frame, 89);
    assert_eq!(nested.opacity, 0.75);
}

#[test]
fn excludes_inactive_layers_and_reports_a_missing_scene() {
    let document = project_document();
    assert!(SceneRenderPlan::from_document(&document, 99).is_err());

    let mut plan = SceneRenderPlan::from_document(&document, 1).expect("scene plan builds");
    let frame = plan.evaluate(110);
    assert!(frame.layers.is_empty());
    assert!(frame.audio.is_empty());
}
