use aviqtl_rust_core::api::{
    ExportImageFormat, ImageSequenceExportRequest, PluginPermission, PluginPermissionState,
    ScriptPluginIdentity, ScriptPluginValidationStatus, SettingsState, TimelineCommand,
    TimelineError, TimelineState, VideoExportRequest, audio_plugin_categories,
    audio_plugins_in_category, build_effect_preset, deduplicate_audio_plugins,
    evaluate_keyframe_track, find_vacant_scene_frame, inspect_keyframe_track,
    inspect_script_metadata, keyframe_interpolation_names, parse_audio_plugin_discovery_output,
    parse_effect_metadata, parse_effect_preset, parse_script_plugin_manifest, plan_clip_delta_move,
    plan_clip_resize, plan_effect_reorder, plan_export_audio_frame, plan_export_progress,
    plan_image_sequence_export, plan_video_export, snap_scene_frame,
    validate_script_plugin_manifest, video_export_defaults,
};
use serde_json::{Map, json};
use std::path::PathBuf;

fn project() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": 3,
        "settings": {"width": 1920, "height": 1080, "fps": 60.0, "sampleRate": 48000},
        "scenes": [{"id": 1, "name": "Root"}],
        "clips": [{
            "id": 4,
            "sceneId": 1,
            "type": "video",
            "start": 0,
            "duration": 20,
            "layer": 0,
            "params": {"path": "media.mp4"}
        }]
    }))
    .expect("project fixture serializes")
}

#[test]
fn public_api_applies_and_undoes_a_planned_edit() {
    let mut state = TimelineState::from_json(&project()).expect("project loads");
    let before = state.snapshot();
    let transaction = state
        .plan(TimelineCommand::UpdateClipGeometry {
            clip_id: 4,
            layer: 3,
            start: 12,
            duration: 48,
        })
        .expect("edit plans");

    assert_eq!(state.snapshot(), before);
    state.apply(&transaction).expect("forward edit applies");
    let edited = state.snapshot();
    assert_eq!(edited.clips[0].layer, 3);
    assert_eq!(edited.clips[0].start, 12);
    assert_eq!(edited.clips[0].duration, 48);

    state.undo(&transaction).expect("inverse edit applies");
    assert_eq!(state.snapshot(), before);
}

#[test]
fn public_api_batches_edits_and_keeps_transactions_opaque() {
    let mut state = TimelineState::from_json(&project()).expect("project loads");
    let transaction = state
        .plan_batch(vec![
            TimelineCommand::UpdateClipGeometry {
                clip_id: 4,
                layer: 2,
                start: 8,
                duration: 30,
            },
            TimelineCommand::SetClipByUpperObject {
                clip_id: 4,
                enabled: true,
            },
        ])
        .expect("batch plans");

    state.apply(&transaction).expect("batch applies");
    let edited = state.snapshot();
    assert_eq!(edited.clips[0].layer, 2);
    assert_eq!(edited.clips[0].start, 8);
    assert!(edited.clips[0].clip_by_upper_object);

    state.undo(&transaction).expect("batch undoes");
    assert_eq!(state.snapshot().clips[0].start, 0);
}

#[test]
fn public_api_plans_stable_multi_effect_reordering() {
    let single = plan_effect_reorder(4, &[1], 3, 0).expect("single effect reorder plans");
    assert_eq!(single, [0, 2, 3, 1]);

    let permutation = plan_effect_reorder(6, &[4, 2, 4, 0], 5, 1).expect("effect reorder plans");
    assert_eq!(permutation, [0, 1, 3, 2, 4, 5]);
    assert!(plan_effect_reorder(3, &[0], 2, 1).is_err());
}

#[test]
fn public_api_owns_effect_metadata_and_preset_documents() {
    let metadata = parse_effect_metadata(include_bytes!("../../../ui/qml/effects/blur.json"))
        .expect("built-in effect metadata parses");
    assert_eq!(metadata.id, "blur");
    assert_eq!(metadata.kind, "effect");
    assert_eq!(metadata.params["size"], 5);

    let bytes = build_effect_preset(
        &metadata.id,
        "Warm blur",
        true,
        metadata.params.clone(),
        Map::new(),
    )
    .expect("preset builds");
    let preset =
        parse_effect_preset(&bytes, &metadata.id, "Warm blur").expect("preset identity validates");
    assert_eq!(preset.params["quality"], 1);
    assert!(parse_effect_preset(&bytes, "mosaic", "Warm blur").is_err());
}

#[test]
fn public_api_allocates_ids_and_reports_schema_errors() {
    let mut state = TimelineState::from_json_with_id_hints(&project(), 2, 1)
        .expect("project loads with stale hints");
    assert_eq!(
        state.reserve_clip_ids(3).expect("clip IDs reserve"),
        [5, 6, 7]
    );
    assert_eq!(
        state.reserve_scene_ids(2).expect("scene IDs reserve"),
        [2, 3]
    );
    assert!(
        state
            .plan(TimelineCommand::UpdateClipGeometry {
                clip_id: 999,
                layer: 0,
                start: 0,
                duration: 10,
            })
            .is_err()
    );
}

#[test]
fn public_api_plans_snapped_collision_safe_timeline_interactions() {
    let state = TimelineState::from_json(&project()).expect("project loads");
    let mut document = state.snapshot();
    document.scenes[0].grid_mode = "Frame".to_owned();
    document.scenes[0].grid_interval = 10;
    assert_eq!(snap_scene_frame(16.0, false, &document.scenes[0], 3.0), 20);
    assert_eq!(snap_scene_frame(16.0, true, &document.scenes[0], 3.0), 16);

    let mut blocking = document.clips[0].clone();
    blocking.id = 5;
    blocking.start = 25;
    document.clips.push(blocking);
    assert_eq!(
        find_vacant_scene_frame(&document, 1, &[], 0, 10, 10),
        Ok(45)
    );
    assert_eq!(
        find_vacant_scene_frame(&document, 1, &[4], 0, 10, 10),
        Ok(10)
    );
    assert_eq!(
        find_vacant_scene_frame(&document, 1, &[], 512, 10, 10),
        Err(TimelineError::InvalidArgument)
    );
    let moved = plan_clip_delta_move(&document, 1, &[4], 0, 10).expect("move plans");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].start, 45);

    let resized = plan_clip_resize(&document, 1, &[4], 5, -5).expect("resize plans");
    assert_eq!((resized[0].start, resized[0].duration), (5, 15));
    document.scenes[0].locked_layers.push(0);
    assert_eq!(
        plan_clip_resize(&document, 1, &[4], 0, 1),
        Err(TimelineError::InvalidArgument)
    );
}

#[test]
fn public_api_inspects_and_evaluates_keyframe_tracks() {
    let fallback = json!(0.0);
    let track = json!([
        {"frame": 10, "value": 10.0, "interp": "none"},
        {"frame": 0, "value": 0.0, "interp": "linear"},
        {
            "frame": 20,
            "value": 20.0,
            "interp": "custom",
            "bzx1": 0.25,
            "bzy1": 0.0,
            "bzx2": 0.75,
            "bzy2": 1.0
        }
    ]);

    let points = inspect_keyframe_track(Some(&track), &fallback, 20);
    assert_eq!(
        points.iter().map(|point| point.frame).collect::<Vec<_>>(),
        [0, 10, 20]
    );
    assert_eq!(points[0].interpolation, "linear");
    assert_eq!(
        points[2].options["points"].as_array().map(Vec::len),
        Some(6)
    );
    assert_eq!(
        evaluate_keyframe_track(Some(&track), &fallback, 20, 5),
        json!(5.0)
    );
    assert_eq!(evaluate_keyframe_track(None, &json!(7), 20, 5), json!(7));

    let names = keyframe_interpolation_names();
    assert!(names.contains(&"none"));
    assert!(names.contains(&"ease_out_bounce"));
    assert!(names.contains(&"custom"));
    assert!(names.contains(&"random"));
    assert!(names.contains(&"alternate"));
}

#[test]
fn public_api_owns_settings_defaults_merge_mutation_and_persistence() {
    let mut platform = Map::new();
    platform.insert("pluginPathsVST3".to_owned(), json!(["/platform/vst3"]));
    let mut settings = SettingsState::defaults(platform);
    assert_eq!(settings.value("theme"), Some(&json!("Dark")));
    assert_eq!(
        settings.value("pluginPathsVST3"),
        Some(&json!(["/platform/vst3"]))
    );

    let migrated = settings
        .merge_json(
            br#"{
                "theme": "Light",
                "packageRepositoryUrls": ["https://example.invalid/repo.json"],
                "_session": 1
            }"#,
        )
        .expect("settings merge");
    assert!(migrated);
    assert_eq!(settings.value("theme"), Some(&json!("Light")));
    assert_eq!(settings.i32_value("_session", 0), 1);

    let mutation = settings.set_value("backupInterval", json!(12));
    assert!(mutation.changed && mutation.persistent);
    assert_eq!(settings.i32_value("backupInterval", 0), 12);
    assert!(settings.bool_value("showConfirmOnClose", false));
    assert_eq!(settings.f64_value("previewRenderScale", 0.0), 1.0);
    assert!(!settings.set_value("backupInterval", json!(12)).changed);

    let persistent = settings.persistent_snapshot();
    assert!(!persistent.contains_key("_session"));
    assert!(
        serde_json::from_slice::<Map<String, serde_json::Value>>(
            &settings.persistent_json().expect("settings serialize")
        )
        .is_ok()
    );
}

#[test]
fn public_api_parses_and_queries_audio_plugin_discovery() {
    let transcript = "carla-discovery::init\n\
carla-discovery::name::Example Filter\n\
carla-discovery::label::example\n\
carla-discovery::maker::AviQtl\n\
carla-discovery::uniqueId::42\n\
carla-discovery::category::4\n\
carla-discovery::audio.ins::2\n\
carla-discovery::audio.outs::2\n\
carla-discovery::end";
    let plugin =
        parse_audio_plugin_discovery_output(transcript, "VST3", "/plugins/example.vst3", "example")
            .pop()
            .expect("discovery record");
    assert_eq!(plugin.id, "VST3:example:42");
    assert_eq!(plugin.category, "Filter");
    assert_eq!((plugin.audio_ins, plugin.audio_outs), (2, 2));

    let plugins = deduplicate_audio_plugins(vec![plugin.clone(), plugin]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(audio_plugin_categories(&plugins), vec!["Filter"]);
    assert_eq!(audio_plugins_in_category(&plugins, "filter").len(), 1);
}

#[test]
fn public_api_parses_and_validates_declarative_script_plugins() {
    let manifest = parse_script_plugin_manifest(
        r#"
            -- data-only plugin manifest
            return {
                id = "com.aviqtl.example",
                name = 'Example',
                version = "1.0.0",
                author = "AviQtl",
                description = "Safe manifest",
                min_app_version = "0.6.0",
            }
        "#,
    )
    .expect("manifest parses");
    let (manifest, status) = validate_script_plugin_manifest(
        manifest,
        false,
        "",
        env!("CARGO_PKG_VERSION"),
        "/plugins/example/main.lua",
        &[],
    );
    assert_eq!(status, ScriptPluginValidationStatus::Ok);
    assert_eq!(manifest.id, "com.aviqtl.example");
    assert!(
        parse_script_plugin_manifest("return os.execute('unsafe')").is_none(),
        "manifest parsing must never execute or accept imperative Lua"
    );

    let metadata = inspect_script_metadata(
        "--group:Playback,false\n--track@rate:Rate,1,120,60\n--check@enabled:Enabled,true\nlocal active = true",
    );
    assert_eq!(metadata.parameters.len(), 2);
    assert_eq!(metadata.groups.len(), 1);
    assert_eq!(metadata.parameters[0].var_name, "rate");

    let (_, duplicate) = validate_script_plugin_manifest(
        manifest,
        false,
        "",
        env!("CARGO_PKG_VERSION"),
        "/plugins/example/main.lua",
        &[ScriptPluginIdentity {
            id: "com.aviqtl.example".to_owned(),
            path: "/plugins/other/main.lua".to_owned(),
        }],
    );
    assert_eq!(duplicate, ScriptPluginValidationStatus::Duplicate);
}

#[test]
fn public_api_manages_plugin_permissions_without_legacy_handles() {
    let mut permissions = PluginPermissionState::from_value(&json!({
        "plugin": ["clip.read", "unknown", "clip.read"]
    }))
    .expect("permission object");
    assert!(permissions.has("plugin", PluginPermission::ClipRead));
    assert!(!permissions.has("plugin", PluginPermission::ClipModify));
    assert_eq!(
        PluginPermission::for_api("clip_copy"),
        Some(PluginPermission::ClipboardAccess)
    );
    assert_eq!(
        PluginPermission::from_name("project.save"),
        Some(PluginPermission::ProjectSave)
    );

    permissions.set("plugin", PluginPermission::ClipModify, true);
    permissions.set("plugin", PluginPermission::ClipRead, false);
    assert_eq!(
        permissions.snapshot(),
        json!({"plugin": ["clip.modify"]})
            .as_object()
            .cloned()
            .expect("snapshot object")
    );

    permissions.grant_all("plugin");
    assert_eq!(permissions.granted("plugin"), PluginPermission::ALL);
    permissions.revoke_all("plugin");
    assert!(!permissions.is_authorized("plugin"));
}

#[test]
fn public_api_plans_video_image_audio_and_progress_exports() {
    let defaults = video_export_defaults();
    assert_eq!(defaults.video_codec, "libx264");
    assert_eq!(defaults.audio_codec, "aac");
    let video = plan_video_export(&VideoExportRequest {
        width: 1920,
        height: 1080,
        fps_num: 60_000,
        fps_den: 1_001,
        start_frame: 10,
        end_frame: -1,
        timeline_duration: 110,
        output_path: PathBuf::from("output.mp4"),
        project_fps: 60_000.0 / 1_001.0,
    })
    .expect("video range plans");
    assert_eq!(video.total_frames, 100);
    let images = plan_image_sequence_export(&ImageSequenceExportRequest {
        start_frame: 3,
        end_frame: -1,
        timeline_duration: 12_345,
        configured_padding: 4,
        output_directory: PathBuf::from("frames"),
        format: ExportImageFormat::Jpeg,
    })
    .expect("image sequence plans");
    assert_eq!(images.pad_digits, 5);
    assert_eq!(images.format.extension(), "jpg");
    assert_eq!(
        plan_export_audio_frame(1, 48_000, 60_000, 1_001)
            .expect("audio frame plans")
            .samples_for_frame,
        801
    );
    let progress = plan_export_progress(5, 10, 5, 2_500).expect("progress plans");
    assert!(progress.should_emit);
    assert_eq!(progress.progress, 50);
}
