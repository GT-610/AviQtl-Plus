//! Editor callback wiring; domain mutations stay in aviqtl-app.

use crate::dialogs::{
    choose_missing_media_replacement, format_qt_color, parse_qt_color, persist_window_geometry,
    pick_parameter_file, show_and_redraw, show_centered_and_redraw, show_error_dialog,
    sync_font_families,
};
use crate::easing::{
    easing_options, invoke_current_easing, sync_easing_catalog, sync_easing_curve,
    sync_easing_preview,
};
use crate::lifecycle::{LifecycleUi, WindowRefs, sync_launcher, sync_recovery_window};
use crate::localization::localized;
use crate::object_settings::{
    ObjectSettingsUi, sync_object_catalog, sync_object_settings, sync_timeline_context_catalog,
    sync_timeline_object_catalog,
};
use crate::packages::{
    PackageOperationRuntime, start_package_operation, sync_package_manager, sync_plugin_permissions,
};
use crate::playback::AudioPlaybackRuntime;
use crate::projection::{sync_transport_weak, sync_weak_windows, update_vec_model};
use crate::settings::{
    apply_system_settings, parse_finite_f64, parse_i32_unbounded, parse_required_f64,
    parse_required_i32, project_defaults, sync_project_settings, sync_scene_settings,
    sync_system_settings, timeline_last_layer,
};
use crate::shortcuts::sync_timeline_zoom_settings;
use crate::{MainWindow, TimelineMediaDropResult, TimelineWindow};
use aviqtl_app::easing::BezierCurve;
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::package_manager::{
    PackageManagerModel, PackageOperation, save_plugin_permission_grants,
};
use aviqtl_app::preset_store::PresetStore;
use aviqtl_app::selection::SelectionBox;
use aviqtl_app::settings::SettingsStore;
use aviqtl_app::timeline_interaction::{TimelineDragKind, TimelineDragRequest};
use aviqtl_app::{
    ApplicationModel, LifecycleStep, MAX_TIMELINE_LAYER, MAX_TIMELINE_LAYERS, ProjectSettingsInput,
    SaveDecision, SceneSettingsInput, WorkspaceModel,
};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

pub(super) fn install_timeline_file_drop(
    main: &MainWindow,
    timeline: &TimelineWindow,
    model: Rc<RefCell<ApplicationModel>>,
    settings: Rc<RefCell<SettingsStore>>,
    catalog: Rc<RefCell<EffectCatalog>>,
) {
    let dropped_model = model.clone();
    let dropped_settings = settings;
    let dropped_catalog = catalog;
    let dropped_main = main.as_weak();
    let dropped_timeline = timeline.as_weak();
    timeline.on_file_dropped(move |path, frame, layer, ignore_snap| {
        let Some(window) = dropped_timeline.upgrade() else {
            return TimelineMediaDropResult {
                imported: false,
                next_frame: 0,
                layer: 0,
            };
        };
        let default_duration = dropped_settings
            .borrow()
            .i32_value("defaultClipDuration", 100)
            .max(1);
        let path = PathBuf::from(path.as_str());
        let result = {
            let catalog = dropped_catalog.borrow();
            let mut application = dropped_model.borrow_mut();
            application.current_workspace_mut().and_then(|workspace| {
                let frame = workspace.snap_timeline_frame(
                    f64::from(frame),
                    ignore_snap,
                    f64::from(window.get_pixels_per_frame()),
                );
                workspace.import_media_files(
                    std::slice::from_ref(&path),
                    frame,
                    layer,
                    default_duration,
                    window.get_maximum_layers(),
                    &catalog,
                )
            })
        };
        let drop_result = result.map_or(
            TimelineMediaDropResult {
                imported: false,
                next_frame: 0,
                layer: layer.clamp(0, timeline_last_layer(&window)),
            },
            |(next_frame, next_layer)| {
                window.set_skimmer_frame(next_frame.max(0));
                window.set_skimmer_layer(next_layer.clamp(0, timeline_last_layer(&window)));
                window.set_skimmer_visible(true);
                TimelineMediaDropResult {
                    imported: true,
                    next_frame,
                    layer: next_layer,
                }
            },
        );
        sync_weak_windows(&dropped_main, &dropped_timeline, &dropped_model);
        drop_result
    });

    let timeline_weak = timeline.as_weak();
    let mut cursor_position = None::<(f32, f32)>;
    let mut shift_pressed = false;
    let mut hovered_file_count = 0usize;
    let mut next_target = None::<(i32, i32)>;
    timeline
        .window()
        .on_winit_window_event(move |window, event| {
            let Some(timeline) = timeline_weak.upgrade() else {
                return EventResult::Propagate;
            };
            match event {
                winit::event::WindowEvent::CursorMoved { position, .. } => {
                    let logical = position.to_logical::<f32>(f64::from(window.scale_factor()));
                    cursor_position = Some((logical.x, logical.y));
                    if hovered_file_count > 0 {
                        timeline.invoke_native_file_hovered(logical.x, logical.y, shift_pressed);
                    }
                }
                winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                    shift_pressed = modifiers.state().shift_key();
                    if hovered_file_count > 0
                        && let Some((x, y)) = cursor_position
                    {
                        timeline.invoke_native_file_hovered(x, y, shift_pressed);
                    }
                }
                winit::event::WindowEvent::HoveredFile(_) => {
                    if hovered_file_count == 0 {
                        next_target = None;
                    }
                    hovered_file_count = hovered_file_count.saturating_add(1);
                    if let Some((x, y)) = cursor_position {
                        timeline.invoke_native_file_hovered(x, y, shift_pressed);
                    }
                }
                winit::event::WindowEvent::HoveredFileCancelled => {
                    hovered_file_count = 0;
                    next_target = None;
                    timeline.invoke_native_file_left();
                }
                winit::event::WindowEvent::DroppedFile(path) => {
                    hovered_file_count = hovered_file_count.saturating_sub(1);
                    let path = SharedString::from(path.to_string_lossy().as_ref());
                    let result = if let Some((frame, layer)) = next_target {
                        timeline.invoke_file_dropped(path, frame as f32, layer, true)
                    } else if let Some((x, y)) = cursor_position {
                        timeline.invoke_native_file_dropped(path, x, y, shift_pressed)
                    } else {
                        TimelineMediaDropResult {
                            imported: false,
                            next_frame: 0,
                            layer: 0,
                        }
                    };
                    if result.imported {
                        next_target = Some((result.next_frame, result.layer));
                    }
                    if hovered_file_count == 0 {
                        timeline.set_file_drop_active(false);
                    }
                }
                _ => {}
            }
            EventResult::Propagate
        });
}

pub(super) fn install_callbacks(
    windows: WindowRefs<'_>,
    package_manager_model: Rc<RefCell<PackageManagerModel>>,
    package_operation: Rc<RefCell<Option<PackageOperationRuntime>>>,
    preset_store: Rc<PresetStore>,
    font_families: Rc<Vec<String>>,
    lifecycle_ui: Rc<LifecycleUi>,
    audio_playback: Rc<RefCell<AudioPlaybackRuntime>>,
) {
    let system_apply_ui = lifecycle_ui.clone();
    let model = lifecycle_ui.model.clone();
    let settings = lifecycle_ui.settings.clone();
    let effect_catalog = lifecycle_ui.effect_catalog.clone();
    let WindowRefs {
        launcher,
        recovery,
        main,
        timeline,
        object_settings,
        easing,
        project_settings,
        scene_settings,
        system_settings,
        package_manager,
        plugin_permissions,
        about,
    } = windows;
    let object_settings_ui = ObjectSettingsUi {
        main: main.as_weak(),
        timeline: timeline.as_weak(),
        window: object_settings.as_weak(),
        easing: easing.as_weak(),
        easing_curve: Rc::new(RefCell::new(BezierCurve::default())),
        model: model.clone(),
        catalog: effect_catalog.clone(),
        audio_catalog: lifecycle_ui.audio_plugin_catalog.clone(),
        presets: preset_store.clone(),
        font_families,
    };
    let object_select_ui = object_settings_ui.clone();
    object_settings.on_select_effect(move |index, control, shift| {
        let _ = object_select_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.select_effect(index.max(0) as usize, control, shift)
            });
        object_select_ui.sync();
    });
    let object_context_ui = object_settings_ui.clone();
    object_settings.on_context_select_effect(move |index| {
        let _ = object_context_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.context_select_effect(index.max(0) as usize));
        object_context_ui.sync();
    });
    let object_enabled_ui = object_settings_ui.clone();
    object_settings.on_set_effect_enabled(move |index, enabled| {
        let _ = object_enabled_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.set_audio_plugin_enabled(index.max(0) as usize, enabled)
                } else {
                    workspace.set_effect_enabled(index.max(0) as usize, enabled)
                }
            });
        object_enabled_ui.sync();
    });
    let object_remove_ui = object_settings_ui.clone();
    object_settings.on_remove_effect(move |index| {
        let _ = object_remove_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.remove_audio_plugin(index.max(0) as usize)
                } else {
                    workspace.remove_effect(index.max(0) as usize)
                }
            });
        object_remove_ui.sync();
    });
    let object_remove_selection_ui = object_settings_ui.clone();
    object_settings.on_remove_effect_selection(move |index| {
        let _ = object_remove_selection_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.remove_audio_plugin_group(index.max(0) as usize)
                } else {
                    workspace.remove_effect_group(index.max(0) as usize)
                }
            });
        object_remove_selection_ui.sync();
    });
    let object_delete_selection_ui = object_settings_ui.clone();
    object_settings.on_delete_selected_effects(move || {
        let _ = object_delete_selection_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.remove_selected_audio_plugins()
                } else {
                    workspace.remove_selected_effects()
                }
            });
        object_delete_selection_ui.sync();
    });
    let object_text_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_text(move |audio_plugin, index, param, frame, value| {
        object_text_ui.set_text(
            audio_plugin,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            value.as_str(),
        );
    });
    let object_number_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_number(move |audio_plugin, index, param, frame, value| {
        object_number_ui.set_number(
            audio_plugin,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            value,
        );
    });
    let object_start_number_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_start_number(
        move |index, param, start_frame, end_frame, value| {
            object_start_number_ui.set_start_number(
                index.max(0) as usize,
                param.as_str(),
                start_frame.max(0),
                end_frame.max(0),
                value,
            );
        },
    );
    let object_bool_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_bool(move |audio_plugin, index, param, frame, value| {
        object_bool_ui.set_value(
            audio_plugin,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            serde_json::Value::Bool(value),
        );
    });
    let object_option_ui = object_settings_ui.clone();
    object_settings.on_set_parameter_option(move |audio_plugin, index, param, frame, option| {
        if option >= 0 {
            object_option_ui.set_option(
                audio_plugin,
                index.max(0) as usize,
                param.as_str(),
                frame.max(0),
                option as usize,
            );
        }
    });
    let object_path_ui = object_settings_ui.clone();
    object_settings.on_choose_parameter_path(
        move |audio_plugin, index, param, frame, current, filter, label| {
            let Some(path) = pick_parameter_file(current.as_str(), filter.as_str(), label.as_str())
            else {
                return;
            };
            object_path_ui.set_text(
                audio_plugin,
                index.max(0) as usize,
                param.as_str(),
                frame.max(0),
                &path,
            );
        },
    );
    let object_color_window = object_settings.as_weak();
    object_settings.on_show_color_picker(move |_audio_plugin, index, param, frame, value| {
        let Some(window) = object_color_window.upgrade() else {
            return;
        };
        let [alpha, red, green, blue] = parse_qt_color(value.as_str());
        window.set_picker_effect_index(index);
        window.set_picker_param_name(param);
        window.set_picker_frame(frame.max(0));
        window.set_picker_red(i32::from(red));
        window.set_picker_green(i32::from(green));
        window.set_picker_blue(i32::from(blue));
        window.set_picker_alpha(i32::from(alpha));
        window.set_font_picker_visible(false);
        window.set_color_picker_visible(true);
    });
    let object_color_ui = object_settings_ui.clone();
    object_settings.on_apply_picked_color(move |index, param, frame, red, green, blue, alpha| {
        object_color_ui.set_text(
            false,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            &format_qt_color(red, green, blue, alpha),
        );
    });
    let object_font_window = object_settings.as_weak();
    let object_font_catalog = object_settings_ui.font_families.clone();
    object_settings.on_show_font_picker(move |_audio_plugin, index, param, frame, value| {
        let Some(window) = object_font_window.upgrade() else {
            return;
        };
        window.set_picker_effect_index(index);
        window.set_picker_param_name(param);
        window.set_picker_frame(frame.max(0));
        window.set_current_font_family(value);
        window.set_font_filter(SharedString::new());
        sync_font_families(&window, &object_font_catalog, "");
        window.set_color_picker_visible(false);
        window.set_font_picker_visible(true);
    });
    let object_font_filter_window = object_settings.as_weak();
    let object_font_filter_catalog = object_settings_ui.font_families.clone();
    object_settings.on_filter_fonts(move |query| {
        if let Some(window) = object_font_filter_window.upgrade() {
            sync_font_families(&window, &object_font_filter_catalog, query.as_str());
        }
    });
    let object_font_ui = object_settings_ui.clone();
    object_settings.on_apply_picked_font(move |index, param, frame, family| {
        object_font_ui.set_text(
            false,
            index.max(0) as usize,
            param.as_str(),
            frame.max(0),
            family.as_str(),
        );
    });
    let object_seek_keyframe_ui = object_settings_ui.clone();
    let object_seek_audio = audio_playback.clone();
    object_settings.on_seek_effect_frame(move |frame| {
        if let Some(workspace) = object_seek_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
        {
            workspace.seek_effect_frame(frame.max(0));
            object_seek_audio.borrow_mut().reset_queue();
        }
        object_seek_keyframe_ui.sync();
    });
    let object_snap_model = model.clone();
    let object_snap_timeline = timeline.as_weak();
    let object_snap_settings = settings.clone();
    object_settings.on_snap_effect_frame(move |frame| {
        let timeline_scale = object_snap_timeline
            .upgrade()
            .map_or(1.0, |window| f64::from(window.get_pixels_per_frame()));
        let enable_snap = object_snap_settings.borrow().bool_value("enableSnap", true);
        object_snap_model.borrow().current_workspace().map_or_else(
            || frame.round().max(0.0) as i32,
            |workspace| {
                workspace.snap_effect_keyframe_frame(f64::from(frame), timeline_scale, enable_snap)
            },
        )
    });
    let object_add_keyframe_ui = object_settings_ui.clone();
    object_settings.on_add_effect_keyframe(move |audio_plugin, index, param, frame| {
        let _ = object_add_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if audio_plugin {
                    workspace.add_audio_plugin_keyframe(
                        index.max(0) as usize,
                        param.as_str(),
                        frame.max(0),
                    )
                } else {
                    workspace.add_effect_keyframe(
                        index.max(0) as usize,
                        param.as_str(),
                        frame.max(0),
                    )
                }
            });
        object_add_keyframe_ui.sync();
    });
    let object_seed_keyframe_ui = object_settings_ui.clone();
    object_settings.on_seed_audio_plugin_keyframes(move |index, param| {
        let _ = object_seed_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.seed_audio_plugin_keyframes(index.max(0) as usize, param.as_str())
            });
        object_seed_keyframe_ui.sync();
    });
    let object_remove_keyframe_ui = object_settings_ui.clone();
    object_settings.on_remove_effect_keyframe(move |audio_plugin, index, param, frame| {
        let _ = object_remove_keyframe_ui
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if audio_plugin {
                    workspace.remove_audio_plugin_keyframe(
                        index.max(0) as usize,
                        param.as_str(),
                        frame,
                    )
                } else {
                    workspace.remove_effect_keyframe(index.max(0) as usize, param.as_str(), frame)
                }
            });
        object_remove_keyframe_ui.sync();
    });
    let object_move_keyframe_ui = object_settings_ui.clone();
    object_settings.on_move_effect_keyframe(
        move |audio_plugin, index, param, old_frame, new_frame| {
            let _ = object_move_keyframe_ui
                .model
                .borrow_mut()
                .current_workspace_mut()
                .is_some_and(|workspace| {
                    if audio_plugin {
                        workspace.move_audio_plugin_keyframe(
                            index.max(0) as usize,
                            param.as_str(),
                            old_frame,
                            new_frame,
                        )
                    } else {
                        workspace.move_effect_keyframe(
                            index.max(0) as usize,
                            param.as_str(),
                            old_frame,
                            new_frame,
                        )
                    }
                });
            object_move_keyframe_ui.sync();
        },
    );
    let object_easing_ui = object_settings_ui.clone();
    object_settings.on_open_effect_easing(move |index, param, start_frame, end_frame| {
        object_easing_ui.open_easing(
            index.max(0) as usize,
            param.as_str(),
            start_frame.max(0),
            end_frame.max(0),
        );
    });
    let easing_apply_ui = object_settings_ui.clone();
    let easing_apply_window = easing.as_weak();
    easing.on_apply_easing(
        move |interpolation, step_frames, amplitude, period, x1, y1, x2, y2| {
            let Some(window) = easing_apply_window.upgrade() else {
                return;
            };
            let custom_points = easing_apply_ui.update_easing_custom_points([x1, y1, x2, y2]);
            let options = easing_options(
                interpolation.as_str(),
                step_frames,
                amplitude,
                period,
                &custom_points,
            );
            {
                let curve = easing_apply_ui.easing_curve.borrow();
                sync_easing_preview(&window, &curve);
                sync_easing_catalog(&window, window.get_easing_filter().as_str(), &curve);
            }
            easing_apply_ui.apply_easing(
                window.get_effect_index().max(0) as usize,
                window.get_param_name().as_str(),
                window.get_keyframe_frame().max(0),
                options,
            );
        },
    );
    let easing_custom_window = easing.as_weak();
    easing.on_custom_point_edited(move |index, text| {
        let Some(window) = easing_custom_window.upgrade() else {
            return;
        };
        let mut value = text.as_str().trim().parse::<f32>().unwrap_or(0.0);
        if index == 0 || index == 2 {
            value = value.clamp(0.0, 1.0);
        }
        match index {
            0 => window.set_custom_x1(value),
            1 => window.set_custom_y1(value),
            2 => window.set_custom_x2(value),
            3 => window.set_custom_y2(value),
            _ => return,
        }
        invoke_current_easing(&window);
    });
    let easing_hit_ui = object_settings_ui.clone();
    easing.on_hit_custom_point(move |x, y, tolerance_x, tolerance_y| {
        easing_hit_ui
            .easing_curve
            .borrow()
            .hit_test(
                f64::from(x),
                f64::from(y),
                f64::from(tolerance_x),
                f64::from(tolerance_y),
            )
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1)
    });
    let easing_insert_ui = object_settings_ui.clone();
    let easing_insert_window = easing.as_weak();
    easing.on_insert_custom_anchor(move |x, y| {
        let Some(window) = easing_insert_window.upgrade() else {
            return;
        };
        if easing_insert_ui
            .easing_curve
            .borrow_mut()
            .insert_anchor(f64::from(x), f64::from(y))
        {
            sync_easing_curve(&window, &easing_insert_ui.easing_curve.borrow());
            invoke_current_easing(&window);
        }
    });
    let easing_move_ui = object_settings_ui.clone();
    let easing_move_window = easing.as_weak();
    easing.on_move_custom_point(move |index, x, y| {
        let Some(window) = easing_move_window.upgrade() else {
            return;
        };
        if usize::try_from(index).ok().is_some_and(|index| {
            easing_move_ui
                .easing_curve
                .borrow_mut()
                .move_point(index, f64::from(x), f64::from(y))
        }) {
            sync_easing_curve(&window, &easing_move_ui.easing_curve.borrow());
            invoke_current_easing(&window);
        }
    });
    let easing_remove_ui = object_settings_ui.clone();
    let easing_remove_window = easing.as_weak();
    easing.on_remove_custom_anchor(move |index| {
        let Some(window) = easing_remove_window.upgrade() else {
            return;
        };
        if usize::try_from(index).ok().is_some_and(|index| {
            easing_remove_ui
                .easing_curve
                .borrow_mut()
                .remove_anchor(index)
        }) {
            sync_easing_curve(&window, &easing_remove_ui.easing_curve.borrow());
            invoke_current_easing(&window);
        }
    });
    let easing_filter_window = easing.as_weak();
    let easing_filter_ui = object_settings_ui.clone();
    easing.on_filter_easings(move |query| {
        if let Some(window) = easing_filter_window.upgrade() {
            sync_easing_catalog(
                &window,
                query.as_str(),
                &easing_filter_ui.easing_curve.borrow(),
            );
        }
    });
    let easing_close = easing.as_weak();
    let easing_close_settings = settings.clone();
    easing.on_close_window(move || {
        if let Some(window) = easing_close.upgrade() {
            persist_window_geometry(&easing_close_settings, "easingConfig", window.window());
            let _ = window.hide();
        }
    });
    let object_reorder_ui = object_settings_ui.clone();
    object_settings.on_reorder_effect(move |index, delta_y| {
        object_reorder_ui.reorder_effect(index.max(0) as usize, delta_y);
    });
    let object_filter_window = object_settings.as_weak();
    let object_filter_model = model.clone();
    let object_filter_catalog = effect_catalog.clone();
    let object_filter_audio_catalog = object_settings_ui.audio_catalog.clone();
    object_settings.on_filter_effects(move |query| {
        if let Some(window) = object_filter_window.upgrade() {
            let effect_catalog = object_filter_catalog.borrow();
            sync_object_catalog(
                &window,
                &object_filter_model.borrow(),
                &effect_catalog,
                &object_filter_audio_catalog.borrow(),
                query.as_str(),
            );
        }
    });
    let object_add_ui = object_settings_ui.clone();
    object_settings.on_add_effect(move |effect_id| {
        object_add_ui.add_effect(effect_id.as_str());
    });
    let preset_names_model = model.clone();
    let preset_names_store = preset_store.clone();
    object_settings.on_preset_names(move |index| {
        let effect_id = preset_names_model
            .borrow()
            .current_workspace()
            .and_then(WorkspaceModel::selected_clip_document)
            .and_then(|clip| {
                if clip.clip_type == "audio" {
                    clip.audio_plugins
                        .get(index.max(0) as usize)
                        .map(|plugin| plugin.id.clone())
                } else {
                    clip.effects
                        .get(index.max(0) as usize)
                        .map(|effect| effect.id.clone())
                }
            });
        let names = effect_id.map_or_else(Vec::new, |effect_id| {
            preset_names_store
                .names(&effect_id)
                .into_iter()
                .map(SharedString::from)
                .collect()
        });
        ModelRc::new(VecModel::from(names))
    });
    let preset_save_ui = object_settings_ui.clone();
    object_settings.on_save_effect_preset(move |index, name| {
        preset_save_ui.save_preset(index.max(0) as usize, name.as_str());
    });
    let preset_load_ui = object_settings_ui.clone();
    object_settings.on_load_effect_preset(move |index, name| {
        preset_load_ui.load_preset(index.max(0) as usize, name.as_str());
    });
    let preset_delete_ui = object_settings_ui.clone();
    object_settings.on_delete_effect_preset(move |index, name| {
        preset_delete_ui.delete_preset(index.max(0) as usize, name.as_str());
    });
    let create_model = model.clone();
    let create_main = main.as_weak();
    let create_timeline = timeline.as_weak();
    let create_launcher = launcher.as_weak();
    let create_recovery = recovery.as_weak();
    let create_settings = settings.clone();
    launcher.on_create_project(move |width, height, fps, sample_rate| {
        if create_model.borrow().lifecycle_pending() {
            return;
        }
        let mut defaults = project_defaults(&create_settings.borrow());
        defaults.width =
            match parse_required_i32(&width, localized("Width", "宽度", "幅"), 1, 8_000) {
                Ok(value) => value,
                Err(message) => {
                    show_error_dialog(&message);
                    return;
                }
            };
        defaults.height =
            match parse_required_i32(&height, localized("Height", "高度", "高さ"), 1, 8_000) {
                Ok(value) => value,
                Err(message) => {
                    show_error_dialog(&message);
                    return;
                }
            };
        defaults.fps = match parse_required_f64(&fps, "FPS", 1.0, 240.0) {
            Ok(value) => value,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        defaults.sample_rate = match parse_required_i32(
            &sample_rate,
            localized("Sample rate", "采样率", "サンプリングレート"),
            8_000,
            192_000,
        ) {
            Ok(value) => value,
            Err(message) => {
                show_error_dialog(&message);
                return;
            }
        };
        create_model.borrow_mut().create_project(defaults);
        sync_weak_windows(&create_main, &create_timeline, &create_model);
        if let Some(main) = create_main.upgrade() {
            let _ = show_and_redraw(&main);
        }
        if let Some(timeline) = create_timeline.upgrade() {
            let _ = show_and_redraw(&timeline);
        }
        if let Some(window) = create_launcher.upgrade() {
            let _ = window.hide();
        }
        if let Some(window) = create_recovery.upgrade() {
            let _ = window.hide();
        }
    });

    let launcher_open_ui = lifecycle_ui.clone();
    launcher.on_open_project(move || launcher_open_ui.open_project_dialog(true));
    let launcher_recent_ui = lifecycle_ui.clone();
    launcher.on_open_recent_project(move |path| {
        launcher_recent_ui.open_project_path(Path::new(path.as_str()), true);
    });

    let new_launcher = launcher.as_weak();
    let new_ui = lifecycle_ui.clone();
    let new_model = model.clone();
    main.on_new_project(move || {
        if new_model.borrow().lifecycle_pending() {
            return;
        }
        if let Some(window) = new_launcher.upgrade() {
            let _ = show_and_redraw(&window);
        }
        new_ui.show_recoveries_if_available();
    });

    let main_open_ui = lifecycle_ui.clone();
    main.on_open_project(move || main_open_ui.open_project_dialog(false));

    let save_ui = lifecycle_ui.clone();
    main.on_save_project(move || {
        let step = save_ui.model.borrow_mut().request_save_current_project();
        save_ui.handle(step);
    });

    let save_as_ui = lifecycle_ui.clone();
    main.on_save_project_as(move || {
        let step = save_as_ui
            .model
            .borrow_mut()
            .request_save_current_project_as();
        save_as_ui.handle(step);
    });

    let quit_ui = lifecycle_ui.clone();
    main.on_quit_requested(move || {
        let confirm_unsaved = quit_ui
            .settings
            .borrow()
            .bool_value("showConfirmOnClose", true);
        let step = quit_ui.model.borrow_mut().request_quit(confirm_unsaved);
        quit_ui.handle(step);
    });

    let close_window_ui = lifecycle_ui.clone();
    main.window().on_close_requested(move || {
        if close_window_ui.quit_confirmed.get() {
            return CloseRequestResponse::HideWindow;
        }
        let confirm_unsaved = close_window_ui
            .settings
            .borrow()
            .bool_value("showConfirmOnClose", true);
        let step = close_window_ui
            .model
            .borrow_mut()
            .request_quit(confirm_unsaved);
        let quit_ready = matches!(step, LifecycleStep::QuitReady);
        close_window_ui.handle(step);
        if quit_ready {
            CloseRequestResponse::HideWindow
        } else {
            CloseRequestResponse::KeepWindowShown
        }
    });

    let confirm_save_ui = lifecycle_ui.clone();
    main.on_confirm_save(move || {
        let step = confirm_save_ui
            .model
            .borrow_mut()
            .answer_save_confirmation(SaveDecision::Save);
        confirm_save_ui.handle(step);
    });
    let confirm_discard_ui = lifecycle_ui.clone();
    main.on_confirm_discard(move || {
        let step = confirm_discard_ui
            .model
            .borrow_mut()
            .answer_save_confirmation(SaveDecision::Discard);
        confirm_discard_ui.handle(step);
    });
    let confirm_cancel_ui = lifecycle_ui.clone();
    main.on_confirm_cancel(move || {
        let step = confirm_cancel_ui
            .model
            .borrow_mut()
            .answer_save_confirmation(SaveDecision::Cancel);
        confirm_cancel_ui.handle(step);
    });

    let select_model = model.clone();
    let select_main = main.as_weak();
    let select_timeline = timeline.as_weak();
    main.on_select_project(move |index| {
        if select_model
            .borrow_mut()
            .select_project(index.max(0) as usize)
        {
            if let Some(window) = select_timeline.upgrade() {
                window.set_skimmer_visible(false);
            }
            sync_weak_windows(&select_main, &select_timeline, &select_model);
        }
    });

    let close_ui = lifecycle_ui.clone();
    main.on_close_project(move |index| {
        let step = close_ui
            .model
            .borrow_mut()
            .request_close_project(index.max(0) as usize);
        close_ui.handle(step);
    });

    let relink_model = model.clone();
    let relink_main = main.as_weak();
    let relink_timeline = timeline.as_weak();
    main.on_relink_missing_media(move |clip_id| {
        let target = relink_model
            .borrow()
            .current_workspace()
            .and_then(|workspace| {
                workspace
                    .missing_media()
                    .into_iter()
                    .find(|entry| entry.clip_id == clip_id)
                    .map(|entry| {
                        let path = PathBuf::from(&entry.path);
                        let suggested_path = if path.is_absolute() {
                            path
                        } else {
                            workspace
                                .project()
                                .path
                                .as_deref()
                                .and_then(Path::parent)
                                .map_or(path.clone(), |directory| directory.join(path))
                        };
                        (entry.clip_type, suggested_path)
                    })
            });
        let Some((clip_type, suggested_path)) = target else {
            return;
        };
        let Some(path) = choose_missing_media_replacement(&clip_type, &suggested_path) else {
            return;
        };
        let error = {
            let mut application = relink_model.borrow_mut();
            application.current_workspace_mut().and_then(|workspace| {
                (!workspace.relink_media(clip_id, &path)).then(|| workspace.status().to_owned())
            })
        };
        if let Some(message) = error {
            show_error_dialog(&message);
        }
        sync_weak_windows(&relink_main, &relink_timeline, &relink_model);
    });

    let recover_ui = lifecycle_ui.clone();
    recovery.on_recover_project(move |id| {
        let recover_ui = recover_ui.clone();
        let id = id.to_string();
        Timer::single_shot(Duration::ZERO, move || {
            let result = recover_ui.model.borrow_mut().recover_project(&id);
            match result {
                Ok(_) => {
                    recover_ui.hydrate_audio_plugins();
                    if let Some(window) = recover_ui.recovery.upgrade() {
                        let _ = window.hide();
                    }
                    recover_ui.sync();
                    if let Some(window) = recover_ui.main.upgrade() {
                        let _ = show_and_redraw(&window);
                    }
                    if let Some(window) = recover_ui.timeline.upgrade() {
                        let _ = show_and_redraw(&window);
                    }
                    if let Some(window) = recover_ui.launcher.upgrade() {
                        let _ = window.hide();
                    }
                }
                Err(message) => {
                    if let Some(window) = recover_ui.recovery.upgrade() {
                        window.set_error_message(SharedString::from(message));
                    }
                }
            }
        });
    });

    let discard_ui = lifecycle_ui;
    recovery.on_discard_recovery(move |id| {
        let discard_ui = discard_ui.clone();
        let id = id.to_string();
        Timer::single_shot(Duration::ZERO, move || {
            let result = discard_ui.model.borrow_mut().discard_recovery(&id);
            if let Some(window) = discard_ui.recovery.upgrade() {
                match result {
                    Ok(()) => {
                        sync_recovery_window(&window, &discard_ui.model.borrow());
                        if discard_ui.model.borrow().recovery_entries().is_empty() {
                            let _ = window.hide();
                        }
                    }
                    Err(message) => window.set_error_message(SharedString::from(message)),
                }
            }
        });
    });

    let recovery_window = recovery.as_weak();
    recovery.on_close_window(move || {
        if let Some(window) = recovery_window.upgrade() {
            window.set_error_message(SharedString::new());
            window.set_discard_confirmation_visible(false);
            let _ = window.hide();
        }
    });

    let timeline_window = timeline.as_weak();
    main.on_show_timeline(move || {
        if let Some(window) = timeline_window.upgrade() {
            let _ = show_and_redraw(&window);
        }
    });
    let settings_window = object_settings.as_weak();
    let settings_model = model.clone();
    let settings_catalog = effect_catalog.clone();
    main.on_show_object_settings(move || {
        if let Some(window) = settings_window.upgrade() {
            sync_object_settings(
                &window,
                &settings_model.borrow(),
                &settings_catalog.borrow(),
            );
            let _ = show_and_redraw(&window);
        }
    });

    let project_settings_model = model.clone();
    let project_settings_window = project_settings.as_weak();
    main.on_show_project_settings(move || {
        let input = project_settings_model
            .borrow()
            .current_workspace()
            .map(|workspace| workspace.project_settings());
        if let (Some(window), Some(input)) = (project_settings_window.upgrade(), input) {
            sync_project_settings(&window, &input);
            let _ = show_and_redraw(&window);
        }
    });

    let system_settings_store = settings.clone();
    let system_settings_window = system_settings.as_weak();
    main.on_show_system_settings(move || {
        if let Some(window) = system_settings_window.upgrade() {
            sync_system_settings(&window, &system_settings_store.borrow());
            let _ = show_and_redraw(&window);
        }
    });

    let package_window = package_manager.as_weak();
    let package_open_model = package_manager_model.clone();
    main.on_show_package_manager(move || {
        if let Some(window) = package_window.upgrade() {
            sync_package_manager(&window, &package_open_model.borrow());
            let _ = show_and_redraw(&window);
        }
    });
    let package_tab_window = package_manager.as_weak();
    let package_tab_model = package_manager_model.clone();
    package_manager.on_select_tab(move |_| {
        if let Some(window) = package_tab_window.upgrade() {
            sync_package_manager(&window, &package_tab_model.borrow());
        }
    });
    let package_filter_window = package_manager.as_weak();
    let package_filter_model = package_manager_model.clone();
    package_manager.on_filter_packages(move |_| {
        if let Some(window) = package_filter_window.upgrade() {
            sync_package_manager(&window, &package_filter_model.borrow());
        }
    });
    let package_sync_window = package_manager.as_weak();
    let package_sync_model = package_manager_model.clone();
    let package_sync_runtime = package_operation.clone();
    package_manager.on_sync_repositories(move || {
        if let Some(window) = package_sync_window.upgrade() {
            start_package_operation(
                &window,
                &package_sync_runtime,
                &package_sync_model,
                PackageOperation::Sync,
            );
        }
    });
    let package_install_window = package_manager.as_weak();
    let package_install_model = package_manager_model.clone();
    let package_install_runtime = package_operation.clone();
    package_manager.on_install_package(move |package_id, source_repository| {
        if let Some(window) = package_install_window.upgrade() {
            start_package_operation(
                &window,
                &package_install_runtime,
                &package_install_model,
                PackageOperation::Install {
                    package_id: package_id.to_string(),
                    source_repository: source_repository.to_string(),
                    version: String::new(),
                },
            );
        }
    });
    let package_remove_window = package_manager.as_weak();
    let package_remove_model = package_manager_model.clone();
    let package_remove_runtime = package_operation.clone();
    package_manager.on_remove_package(move |package_id| {
        if let Some(window) = package_remove_window.upgrade() {
            start_package_operation(
                &window,
                &package_remove_runtime,
                &package_remove_model,
                PackageOperation::Remove {
                    package_id: package_id.to_string(),
                },
            );
        }
    });
    let package_upgrade_window = package_manager.as_weak();
    let package_upgrade_model = package_manager_model.clone();
    let package_upgrade_runtime = package_operation.clone();
    package_manager.on_upgrade_all(move || {
        if let Some(window) = package_upgrade_window.upgrade() {
            start_package_operation(
                &window,
                &package_upgrade_runtime,
                &package_upgrade_model,
                PackageOperation::UpgradeAll,
            );
        }
    });
    let permission_window = plugin_permissions.as_weak();
    let permission_parent = package_manager.as_weak();
    let permission_settings = settings.clone();
    package_manager.on_show_permissions(move |plugin_id, plugin_name| {
        if let (Some(window), Some(parent)) =
            (permission_window.upgrade(), permission_parent.upgrade())
        {
            window.set_plugin_id(plugin_id.clone());
            window.set_plugin_name(if plugin_name.is_empty() {
                plugin_id
            } else {
                plugin_name
            });
            sync_plugin_permissions(&window, &permission_settings.borrow());
            let _ = show_centered_and_redraw(&window, &parent);
        }
    });
    let permission_toggle_window = plugin_permissions.as_weak();
    plugin_permissions.on_toggle_permission(move |index, granted| {
        if let Some(window) = permission_toggle_window.upgrade() {
            let mut rows = window.get_permissions().iter().collect::<Vec<_>>();
            if let Some(row) = rows.get_mut(index.max(0) as usize) {
                row.granted = granted;
                update_vec_model(&window.get_permissions(), rows);
            }
        }
    });
    let permission_all_window = plugin_permissions.as_weak();
    plugin_permissions.on_set_all(move |granted| {
        if let Some(window) = permission_all_window.upgrade() {
            let rows = window
                .get_permissions()
                .iter()
                .map(|mut row| {
                    row.granted = granted;
                    row
                })
                .collect();
            update_vec_model(&window.get_permissions(), rows);
        }
    });
    let permission_accept_window = plugin_permissions.as_weak();
    let permission_accept_settings = settings.clone();
    plugin_permissions.on_accept(move || {
        let Some(window) = permission_accept_window.upgrade() else {
            return;
        };
        let granted = window
            .get_permissions()
            .iter()
            .filter(|row| row.granted)
            .map(|row| row.name.to_string())
            .collect::<Vec<_>>();
        match save_plugin_permission_grants(
            &mut permission_accept_settings.borrow_mut(),
            window.get_plugin_id().as_str(),
            &granted,
        ) {
            Ok(()) => {
                let _ = window.hide();
            }
            Err(error) => show_error_dialog(&error),
        }
    });
    let permission_close_window = plugin_permissions.as_weak();
    plugin_permissions.on_close_window(move || {
        if let Some(window) = permission_close_window.upgrade() {
            let _ = window.hide();
        }
    });
    let package_add_window = package_manager.as_weak();
    let package_add_model = package_manager_model.clone();
    let package_add_settings = settings.clone();
    package_manager.on_add_repository(move |url| {
        let result = package_add_model
            .borrow_mut()
            .add_repository(&mut package_add_settings.borrow_mut(), url.as_str());
        if let Some(window) = package_add_window.upgrade() {
            match result {
                Ok(true) => window.set_repository_url(SharedString::new()),
                Ok(false) => {}
                Err(message) => window.set_error_message(SharedString::from(message)),
            }
            sync_package_manager(&window, &package_add_model.borrow());
        }
    });
    let package_enabled_window = package_manager.as_weak();
    let package_enabled_model = package_manager_model.clone();
    let package_enabled_settings = settings.clone();
    package_manager.on_set_repository_enabled(move |url, enabled| {
        let result = package_enabled_model.borrow_mut().set_repository_enabled(
            &mut package_enabled_settings.borrow_mut(),
            url.as_str(),
            enabled,
        );
        if let Some(window) = package_enabled_window.upgrade() {
            if let Err(message) = result {
                window.set_error_message(SharedString::from(message));
            }
            sync_package_manager(&window, &package_enabled_model.borrow());
        }
    });
    let package_repository_remove_window = package_manager.as_weak();
    let package_repository_remove_model = package_manager_model.clone();
    let package_repository_remove_settings = settings.clone();
    package_manager.on_remove_repository(move |url| {
        let result = package_repository_remove_model
            .borrow_mut()
            .remove_repository(
                &mut package_repository_remove_settings.borrow_mut(),
                url.as_str(),
            );
        if let Some(window) = package_repository_remove_window.upgrade() {
            if let Err(message) = result {
                window.set_error_message(SharedString::from(message));
            }
            sync_package_manager(&window, &package_repository_remove_model.borrow());
        }
    });
    let package_error_window = package_manager.as_weak();
    package_manager.on_dismiss_error(move || {
        if let Some(window) = package_error_window.upgrade() {
            window.set_error_message(SharedString::new());
        }
    });
    let package_update_window = package_manager.as_weak();
    package_manager.on_dismiss_update(move || {
        if let Some(window) = package_update_window.upgrade() {
            window.set_update_message(SharedString::new());
        }
    });

    let about_window = about.as_weak();
    main.on_show_about(move || {
        if let Some(window) = about_window.upgrade() {
            let _ = show_and_redraw(&window);
        }
    });
    about.on_open_project_page(move || {
        if let Err(error) = webbrowser::open("https://codeberg.org/taisho-guy/AviQtl") {
            show_error_dialog(&format!("Failed to open the project page: {error}"));
        }
    });

    let project_apply_model = model.clone();
    let project_apply_main = main.as_weak();
    let project_apply_timeline = timeline.as_weak();
    let project_apply_window = project_settings.as_weak();
    project_settings.on_apply_settings(move || {
        let Some(window) = project_apply_window.upgrade() else {
            return false;
        };
        let fps = match parse_required_f64(&window.get_project_fps(), "FPS", 1.0, 240.0) {
            Ok(fps) => fps,
            Err(message) => {
                show_error_dialog(&message);
                return false;
            }
        };
        let input = ProjectSettingsInput {
            width: window.get_project_width(),
            height: window.get_project_height(),
            fps,
            sample_rate: window.get_project_sample_rate(),
        };
        let applied = project_apply_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.update_project_settings(input));
        if applied {
            sync_weak_windows(
                &project_apply_main,
                &project_apply_timeline,
                &project_apply_model,
            );
        }
        applied
    });
    let project_close_window = project_settings.as_weak();
    let project_close_settings = settings.clone();
    project_settings.on_close_window(move || {
        if let Some(window) = project_close_window.upgrade() {
            persist_window_geometry(&project_close_settings, "projectSettings", window.window());
            let _ = window.hide();
        }
    });

    let scene_create_model = model.clone();
    let scene_create_settings = settings.clone();
    let scene_create_window = scene_settings.as_weak();
    timeline.on_show_scene_settings(move || {
        let input = {
            let application = scene_create_model.borrow();
            application.current_workspace().map(|workspace| {
                let project = workspace.project_settings();
                SceneSettingsInput {
                    name: format!(
                        "{} {}",
                        localized("Scene", "场景", "シーン"),
                        workspace.document().scenes.len() + 1
                    ),
                    width: project.width,
                    height: project.height,
                    fps: project.fps,
                    duration: scene_create_settings
                        .borrow()
                        .i32_value("defaultProjectFrames", 3_600)
                        .max(1),
                    grid_mode: "Auto".to_owned(),
                    grid_bpm: 120.0,
                    grid_offset: 0.0,
                    grid_interval: 10,
                    grid_subdivision: 4,
                    enable_snap: true,
                    magnetic_snap_range: 10,
                }
            })
        };
        if let (Some(window), Some(input)) = (scene_create_window.upgrade(), input) {
            sync_scene_settings(&window, true, -1, &input);
            let _ = show_and_redraw(&window);
        }
    });

    let timeline_settings = object_settings.as_weak();
    let timeline_settings_model = model.clone();
    let timeline_settings_catalog = effect_catalog.clone();
    timeline.on_show_object_settings(move || {
        if let Some(window) = timeline_settings.upgrade() {
            sync_object_settings(
                &window,
                &timeline_settings_model.borrow(),
                &timeline_settings_catalog.borrow(),
            );
            let _ = show_and_redraw(&window);
        }
    });

    let timeline_action_model = model.clone();
    let timeline_action_main = main.as_weak();
    let timeline_action_window = timeline.as_weak();
    let timeline_action_project_settings = project_settings.as_weak();
    let timeline_action_scene_settings = scene_settings.as_weak();
    let timeline_action_system_settings = system_settings.as_weak();
    let timeline_action_settings = settings.clone();
    timeline.on_timeline_action(move |action, frame, layer| {
        match action.as_str() {
            "undo" | "redo" | "paste" => {
                let (pixels_per_frame, last_layer) = timeline_action_window
                    .upgrade()
                    .map_or((1.0, MAX_TIMELINE_LAYER), |window| {
                        (window.get_pixels_per_frame(), timeline_last_layer(&window))
                    });
                if let Some(workspace) = timeline_action_model.borrow_mut().current_workspace_mut()
                {
                    match action.as_str() {
                        "undo" => {
                            workspace.undo();
                        }
                        "redo" => {
                            workspace.redo();
                        }
                        "paste" => {
                            let frame = workspace.snap_timeline_frame(
                                f64::from(frame),
                                false,
                                f64::from(pixels_per_frame),
                            );
                            workspace.paste_clips_at(
                                frame,
                                layer.clamp(0, last_layer),
                                last_layer + 1,
                            );
                        }
                        _ => unreachable!(),
                    }
                }
            }
            "scene-settings" => {
                let input =
                    timeline_action_model
                        .borrow()
                        .current_workspace()
                        .and_then(|workspace| {
                            let scene_id = workspace.selected_scene_document()?.id;
                            workspace
                                .scene_settings(scene_id)
                                .map(|settings| (scene_id, settings))
                        });
                if let (Some(window), Some((scene_id, input))) =
                    (timeline_action_scene_settings.upgrade(), input)
                {
                    sync_scene_settings(&window, false, scene_id, &input);
                    let _ = show_and_redraw(&window);
                }
            }
            "project-settings" => {
                let input = timeline_action_model
                    .borrow()
                    .current_workspace()
                    .map(WorkspaceModel::project_settings);
                if let (Some(window), Some(input)) =
                    (timeline_action_project_settings.upgrade(), input)
                {
                    sync_project_settings(&window, &input);
                    let _ = show_and_redraw(&window);
                }
            }
            "system-settings" => {
                if let Some(window) = timeline_action_system_settings.upgrade() {
                    sync_system_settings(&window, &timeline_action_settings.borrow());
                    let _ = show_and_redraw(&window);
                }
            }
            _ => {}
        }
        sync_weak_windows(
            &timeline_action_main,
            &timeline_action_window,
            &timeline_action_model,
        );
    });

    let seek_audio_model = model.clone();
    let seek_audio_main = main.as_weak();
    let seek_audio_timeline = timeline.as_weak();
    let seek_audio_playback = audio_playback.clone();
    timeline.on_seek_audio_frame(move |clip_id, frame| {
        if let Some(workspace) = seek_audio_model.borrow_mut().current_workspace_mut()
            && let Some(clip) = workspace
                .document()
                .clips
                .iter()
                .find(|clip| clip.id == clip_id && clip.scene_id == workspace.selected_scene())
        {
            let start = clip.start;
            let duration = clip.duration;
            if clip.clip_type == "audio" && duration > 0 {
                workspace
                    .seek(start.saturating_add(frame.saturating_sub(start).clamp(0, duration)));
                seek_audio_playback.borrow_mut().reset_queue();
            }
        }
        sync_weak_windows(&seek_audio_main, &seek_audio_timeline, &seek_audio_model);
    });

    let object_filter_window = timeline.as_weak();
    let object_filter_catalog = effect_catalog.clone();
    timeline.on_filter_objects(move |query, category_index| {
        if let Some(window) = object_filter_window.upgrade() {
            sync_timeline_object_catalog(
                &window,
                &object_filter_catalog.borrow(),
                query.as_str(),
                category_index,
            );
        }
    });

    let context_filter_window = timeline.as_weak();
    let context_filter_effect_catalog = effect_catalog.clone();
    let context_filter_audio_catalog = object_settings_ui.audio_catalog.clone();
    timeline.on_filter_context_catalog(move |query, target_kind| {
        if let Some(window) = context_filter_window.upgrade() {
            sync_timeline_context_catalog(
                &window,
                &context_filter_effect_catalog.borrow(),
                &context_filter_audio_catalog.borrow(),
                query.as_str(),
                target_kind,
            );
        }
    });

    let object_add_model = model.clone();
    let object_add_settings = settings.clone();
    let object_add_catalog = effect_catalog.clone();
    let object_add_main = main.as_weak();
    let object_add_timeline = timeline.as_weak();
    timeline.on_add_catalog_object(move |object_id, frame, layer| {
        let (pixels_per_frame, last_layer) = object_add_timeline
            .upgrade()
            .map_or((1.0, MAX_TIMELINE_LAYER), |window| {
                (window.get_pixels_per_frame(), timeline_last_layer(&window))
            });
        let default_duration = object_add_settings
            .borrow()
            .i32_value("defaultClipDuration", 100)
            .max(1);
        let catalog = object_add_catalog.borrow();
        let added = object_add_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                let frame = workspace.snap_timeline_frame(
                    f64::from(frame),
                    false,
                    f64::from(pixels_per_frame),
                );
                workspace.insert_catalog_object_at(
                    object_id.as_str(),
                    frame,
                    layer.clamp(0, last_layer),
                    default_duration,
                    &catalog,
                )
            });
        sync_weak_windows(&object_add_main, &object_add_timeline, &object_add_model);
        added
    });

    let scene_model = model.clone();
    let scene_main = main.as_weak();
    let scene_timeline = timeline.as_weak();
    timeline.on_scene_selected(move |scene_id| {
        if let Some(workspace) = scene_model.borrow_mut().current_workspace_mut() {
            workspace.switch_scene(scene_id);
        }
        if let Some(window) = scene_timeline.upgrade() {
            window.set_skimmer_visible(false);
        }
        sync_weak_windows(&scene_main, &scene_timeline, &scene_model);
    });

    let close_scene_model = model.clone();
    let close_scene_main = main.as_weak();
    let close_scene_timeline = timeline.as_weak();
    timeline.on_scene_closed(move |scene_id| {
        if let Some(workspace) = close_scene_model.borrow_mut().current_workspace_mut() {
            workspace.remove_scene(scene_id);
        }
        if let Some(window) = close_scene_timeline.upgrade() {
            window.set_skimmer_visible(false);
        }
        sync_weak_windows(&close_scene_main, &close_scene_timeline, &close_scene_model);
    });

    let scene_edit_model = model.clone();
    let scene_edit_window = scene_settings.as_weak();
    timeline.on_scene_settings(move |scene_id| {
        let input = scene_edit_model
            .borrow()
            .current_workspace()
            .and_then(|workspace| workspace.scene_settings(scene_id));
        if let (Some(window), Some(input)) = (scene_edit_window.upgrade(), input) {
            sync_scene_settings(&window, false, scene_id, &input);
            let _ = show_and_redraw(&window);
        }
    });

    let scene_apply_model = model.clone();
    let scene_apply_main = main.as_weak();
    let scene_apply_timeline = timeline.as_weak();
    let scene_apply_window = scene_settings.as_weak();
    let scene_apply_settings = settings.clone();
    scene_settings.on_apply_settings(move || {
        let Some(window) = scene_apply_window.upgrade() else {
            return false;
        };
        let fps = match parse_required_f64(&window.get_scene_fps(), "FPS", 1.0, 240.0) {
            Ok(fps) => fps,
            Err(message) => {
                show_error_dialog(&message);
                return false;
            }
        };
        let grid_mode = match window.get_grid_mode_index() {
            1 => "BPM",
            2 => "Frame",
            _ => "Auto",
        };
        let input = SceneSettingsInput {
            name: window.get_scene_name().to_string(),
            width: window.get_scene_width(),
            height: window.get_scene_height(),
            fps,
            duration: window.get_scene_duration(),
            grid_mode: grid_mode.to_owned(),
            grid_bpm: parse_finite_f64(&window.get_grid_bpm(), 120.0),
            grid_offset: parse_finite_f64(&window.get_grid_offset(), 0.0),
            grid_interval: parse_i32_unbounded(&window.get_grid_interval(), 10),
            grid_subdivision: parse_i32_unbounded(&window.get_grid_subdivision(), 4),
            enable_snap: window.get_enable_snap(),
            magnetic_snap_range: window.get_magnetic_snap_range(),
        };
        let applied = if window.get_creation_mode() {
            let defaults = project_defaults(&scene_apply_settings.borrow());
            scene_apply_model
                .borrow_mut()
                .current_workspace_mut()
                .and_then(|workspace| workspace.create_scene(defaults, input))
                .is_some()
        } else {
            let scene_id = window.get_target_scene_id();
            scene_apply_model
                .borrow_mut()
                .current_workspace_mut()
                .is_some_and(|workspace| workspace.update_scene_settings(scene_id, input))
        };
        if applied {
            sync_weak_windows(&scene_apply_main, &scene_apply_timeline, &scene_apply_model);
        }
        applied
    });
    let scene_close_window = scene_settings.as_weak();
    let scene_close_settings = settings.clone();
    scene_settings.on_close_window(move || {
        if let Some(window) = scene_close_window.upgrade() {
            persist_window_geometry(&scene_close_settings, "sceneSettings", window.window());
            let _ = window.hide();
        }
    });

    let system_reload_store = settings.clone();
    let system_reload_window = system_settings.as_weak();
    system_settings.on_reload_settings(move || {
        if let Some(window) = system_reload_window.upgrade() {
            sync_system_settings(&window, &system_reload_store.borrow());
        }
    });
    let system_plugin_enabled_window = system_settings.as_weak();
    system_settings.on_plugin_enabled_changed(move |index, enabled| {
        let Some(window) = system_plugin_enabled_window.upgrade() else {
            return;
        };
        let model = window.get_plugin_settings();
        if let Some(mut row) = usize::try_from(index)
            .ok()
            .and_then(|index| model.row_data(index).map(|row| (index, row)))
        {
            row.1.enabled = enabled;
            model.set_row_data(row.0, row.1);
        }
    });
    let system_plugin_paths_window = system_settings.as_weak();
    system_settings.on_plugin_paths_changed(move |index, paths| {
        let Some(window) = system_plugin_paths_window.upgrade() else {
            return;
        };
        let model = window.get_plugin_settings();
        if let Some(mut row) = usize::try_from(index)
            .ok()
            .and_then(|index| model.row_data(index).map(|row| (index, row)))
        {
            row.1.paths = paths;
            model.set_row_data(row.0, row.1);
        }
    });
    let system_shortcut_window = system_settings.as_weak();
    system_settings.on_shortcut_value_changed(move |index, value| {
        let Some(window) = system_shortcut_window.upgrade() else {
            return;
        };
        let model = window.get_shortcut_settings();
        if let Some(mut row) = usize::try_from(index)
            .ok()
            .and_then(|index| model.row_data(index).map(|row| (index, row)))
        {
            row.1.value = value;
            model.set_row_data(row.0, row.1);
        }
    });
    let system_apply_store = settings.clone();
    let system_apply_model = model.clone();
    let system_apply_launcher = launcher.as_weak();
    let system_apply_timeline = timeline.as_weak();
    let system_apply_window = system_settings.as_weak();
    system_settings.on_apply_settings(move || {
        let Some(window) = system_apply_window.upgrade() else {
            return false;
        };
        match apply_system_settings(&window, &system_apply_store, &system_apply_model) {
            Ok(()) => {
                sync_system_settings(&window, &system_apply_store.borrow());
                if let Some(launcher) = system_apply_launcher.upgrade() {
                    sync_launcher(&launcher, &system_apply_store.borrow());
                }
                if let Some(timeline) = system_apply_timeline.upgrade() {
                    sync_timeline_zoom_settings(&timeline, &system_apply_store.borrow());
                }
                system_apply_ui.sync_live_settings();
                system_apply_ui.sync();
                true
            }
            Err(message) => {
                show_error_dialog(&message);
                false
            }
        }
    });
    let system_close_window = system_settings.as_weak();
    let system_close_settings = settings.clone();
    system_settings.on_close_window(move || {
        if let Some(window) = system_close_window.upgrade() {
            persist_window_geometry(&system_close_settings, "systemSettings", window.window());
            let _ = window.hide();
        }
    });

    let clip_select_model = model.clone();
    let clip_select_main = main.as_weak();
    let clip_select_timeline = timeline.as_weak();
    timeline.on_clip_selected(move |clip_id, additive| {
        if let Some(workspace) = clip_select_model.borrow_mut().current_workspace_mut() {
            workspace.click_clip(clip_id, additive);
        }
        sync_weak_windows(&clip_select_main, &clip_select_timeline, &clip_select_model);
    });

    let clip_drag_start_model = model.clone();
    let clip_drag_start_main = main.as_weak();
    let clip_drag_start_timeline = timeline.as_weak();
    timeline.on_clip_drag_started(move |clip_id, additive| {
        if let Some(workspace) = clip_drag_start_model.borrow_mut().current_workspace_mut() {
            workspace.prepare_clip_drag(clip_id, additive);
        }
        sync_weak_windows(
            &clip_drag_start_main,
            &clip_drag_start_timeline,
            &clip_drag_start_model,
        );
    });

    let clip_context_model = model.clone();
    let clip_context_main = main.as_weak();
    let clip_context_timeline = timeline.as_weak();
    timeline.on_clip_context_selected(move |clip_id| {
        if let Some(workspace) = clip_context_model.borrow_mut().current_workspace_mut() {
            workspace.context_click_clip(clip_id);
        }
        sync_weak_windows(
            &clip_context_main,
            &clip_context_timeline,
            &clip_context_model,
        );
    });

    let clip_command_model = model.clone();
    let clip_command_main = main.as_weak();
    let clip_command_timeline = timeline.as_weak();
    let clip_command_settings = object_settings.as_weak();
    let clip_command_ui = object_settings_ui.clone();
    timeline.on_clip_command(move |action, clip_id, frame, layer| {
        let maximum_layers = clip_command_timeline
            .upgrade()
            .map_or(MAX_TIMELINE_LAYERS, |window| window.get_maximum_layers());
        let open_effect_picker = action.as_str() == "browse-effect";
        let extension_id = action
            .as_str()
            .strip_prefix("add-extension:")
            .map(str::to_owned);
        if let Some(workspace) = clip_command_model.borrow_mut().current_workspace_mut() {
            workspace.context_click_clip(clip_id);
            match action.as_str() {
                "delete" => {
                    workspace.remove_selected_clips();
                }
                "split" => {
                    workspace.split_selected_clips_at(frame);
                }
                "duplicate" => {
                    workspace.duplicate_selected_clips_at(frame, layer, maximum_layers);
                }
                "cut" => {
                    workspace.cut_selected_clips();
                }
                "copy" => {
                    workspace.copy_selected_clips();
                }
                "clipping" => {
                    workspace.toggle_clip_by_upper_object(clip_id);
                }
                _ => {}
            }
        }
        if let Some(extension_id) = extension_id {
            clip_command_ui.add_effect(&extension_id);
            return;
        }
        sync_weak_windows(
            &clip_command_main,
            &clip_command_timeline,
            &clip_command_model,
        );
        if open_effect_picker && let Some(window) = clip_command_settings.upgrade() {
            window.set_effect_filter(SharedString::new());
            clip_command_ui.sync();
            window.set_effect_picker_visible(true);
            let _ = show_and_redraw(&window);
        }
    });

    let clip_drag_model = model.clone();
    let clip_drag_main = main.as_weak();
    let clip_drag_timeline = timeline.as_weak();
    timeline.on_clip_drag_finished(move |kind, clip_id, delta_x, delta_y, ignore_snap| {
        let kind = match kind.as_str() {
            "trim-start" => TimelineDragKind::TrimStart,
            "trim-end" => TimelineDragKind::TrimEnd,
            _ => TimelineDragKind::Move,
        };
        let (pixels_per_frame, layer_height, minimum_duration_frames, maximum_layers) =
            clip_drag_timeline
                .upgrade()
                .map_or((1.0, 30.0, 5, MAX_TIMELINE_LAYERS), |window| {
                    (
                        window.get_pixels_per_frame(),
                        window.get_timeline_track_height() as f32,
                        window.get_minimum_clip_duration_frames(),
                        window.get_maximum_layers(),
                    )
                });
        if let Some(workspace) = clip_drag_model.borrow_mut().current_workspace_mut() {
            workspace.drag_selected_clips(TimelineDragRequest {
                anchor_clip_id: clip_id,
                kind,
                delta_pixels: (delta_x, delta_y),
                pixels_per_frame,
                layer_height,
                minimum_duration_frames,
                maximum_layers,
                ignore_snap,
            });
        }
        sync_weak_windows(&clip_drag_main, &clip_drag_timeline, &clip_drag_model);
    });

    let layer_model = model.clone();
    let layer_main = main.as_weak();
    let layer_timeline = timeline.as_weak();
    timeline.on_layer_activated(move |layer| {
        if let Some(workspace) = layer_model.borrow_mut().current_workspace_mut() {
            workspace.select_layer(layer);
        }
        sync_weak_windows(&layer_main, &layer_timeline, &layer_model);
    });

    let layer_command_model = model.clone();
    let layer_command_main = main.as_weak();
    let layer_command_timeline = timeline.as_weak();
    timeline.on_layer_command(move |action, layer| {
        let maximum_layers = layer_command_timeline
            .upgrade()
            .map_or(MAX_TIMELINE_LAYERS, |window| window.get_maximum_layers());
        if let Some(workspace) = layer_command_model.borrow_mut().current_workspace_mut() {
            match action.as_str() {
                "insert-above" => {
                    workspace.insert_layers(layer, 1, true, maximum_layers);
                }
                "insert-below" => {
                    workspace.insert_layers(layer, 1, false, maximum_layers);
                }
                "shift-down" => {
                    workspace.shift_layers(layer, layer, 1, maximum_layers);
                }
                "shift-up" => {
                    workspace.shift_layers(layer, layer, -1, maximum_layers);
                }
                "toggle-lock" => {
                    workspace.toggle_layer_lock(layer);
                }
                "toggle-visible" => {
                    workspace.toggle_layer_visibility(layer);
                }
                "show-all" => {
                    workspace.set_all_layers_visible(true, maximum_layers);
                }
                "hide-all" => {
                    workspace.set_all_layers_visible(false, maximum_layers);
                }
                _ => {}
            }
        }
        sync_weak_windows(
            &layer_command_main,
            &layer_command_timeline,
            &layer_command_model,
        );
    });

    let insert_layers_model = model.clone();
    let insert_layers_main = main.as_weak();
    let insert_layers_timeline = timeline.as_weak();
    timeline.on_insert_layers(move |layer, count, above| {
        let maximum_layers = insert_layers_timeline
            .upgrade()
            .map_or(MAX_TIMELINE_LAYERS, |window| window.get_maximum_layers());
        let _ = insert_layers_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.insert_layers(layer, count, above, maximum_layers));
        sync_weak_windows(
            &insert_layers_main,
            &insert_layers_timeline,
            &insert_layers_model,
        );
    });

    let shift_layers_model = model.clone();
    let shift_layers_main = main.as_weak();
    let shift_layers_timeline = timeline.as_weak();
    timeline.on_shift_layers(move |start, end, delta| {
        let maximum_layers = shift_layers_timeline
            .upgrade()
            .map_or(MAX_TIMELINE_LAYERS, |window| window.get_maximum_layers());
        let _ = shift_layers_model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| workspace.shift_layers(start, end, delta, maximum_layers));
        sync_weak_windows(
            &shift_layers_main,
            &shift_layers_timeline,
            &shift_layers_model,
        );
    });

    let empty_model = model.clone();
    let empty_main = main.as_weak();
    let empty_timeline = timeline.as_weak();
    let empty_audio = audio_playback.clone();
    timeline.on_empty_clicked(move |frame, layer| {
        if let Some(workspace) = empty_model.borrow_mut().current_workspace_mut() {
            workspace.select_layer(layer);
            workspace.seek(frame);
            empty_audio.borrow_mut().reset_queue();
        }
        sync_weak_windows(&empty_main, &empty_timeline, &empty_model);
    });

    let skimmer_model = model.clone();
    let skimmer_window = timeline.as_weak();
    timeline.on_skimmer_hovered(move |frame, layer, ignore_snap| {
        let Some(window) = skimmer_window.upgrade() else {
            return;
        };
        let snapped_frame = skimmer_model
            .borrow()
            .current_workspace()
            .map_or(0, |workspace| {
                workspace.snap_timeline_frame(
                    f64::from(frame),
                    ignore_snap,
                    f64::from(window.get_pixels_per_frame()),
                )
            });
        window.set_skimmer_frame(snapped_frame);
        window.set_skimmer_layer(layer.clamp(0, timeline_last_layer(&window)));
        window.set_skimmer_visible(true);
    });
    let skimmer_leave_window = timeline.as_weak();
    timeline.on_skimmer_left(move || {
        if let Some(window) = skimmer_leave_window.upgrade() {
            window.set_skimmer_visible(false);
        }
    });

    let box_state = Rc::new(RefCell::new(None::<SelectionBox>));
    let box_start_state = box_state.clone();
    timeline.on_box_selection_started(move |frame, layer, additive| {
        *box_start_state.borrow_mut() = Some(SelectionBox {
            frame_a: frame,
            frame_b: frame,
            layer_a: layer,
            layer_b: layer,
            additive,
        });
    });
    let box_update_state = box_state.clone();
    let box_update_model = model.clone();
    let box_update_main = main.as_weak();
    let box_update_timeline = timeline.as_weak();
    timeline.on_box_selection_updated(move |frame, layer| {
        let selection_box = {
            let mut state = box_update_state.borrow_mut();
            let Some(selection_box) = state.as_mut() else {
                return;
            };
            selection_box.frame_b = frame;
            selection_box.layer_b = layer;
            *selection_box
        };
        if let Some(workspace) = box_update_model.borrow_mut().current_workspace_mut() {
            workspace.preview_box_selection(selection_box);
        }
        sync_weak_windows(&box_update_main, &box_update_timeline, &box_update_model);
    });
    let box_finish_state = box_state.clone();
    let box_finish_model = model.clone();
    let box_finish_main = main.as_weak();
    let box_finish_timeline = timeline.as_weak();
    timeline.on_box_selection_finished(move || {
        box_finish_state.borrow_mut().take();
        if let Some(workspace) = box_finish_model.borrow_mut().current_workspace_mut() {
            workspace.finish_box_selection();
        }
        sync_weak_windows(&box_finish_main, &box_finish_timeline, &box_finish_model);
    });
    let box_cancel_state = box_state;
    let box_cancel_model = model.clone();
    let box_cancel_main = main.as_weak();
    let box_cancel_timeline = timeline.as_weak();
    timeline.on_box_selection_cancelled(move || {
        box_cancel_state.borrow_mut().take();
        if let Some(workspace) = box_cancel_model.borrow_mut().current_workspace_mut() {
            workspace.cancel_box_selection();
        }
        sync_weak_windows(&box_cancel_main, &box_cancel_timeline, &box_cancel_model);
    });

    let undo_model = model.clone();
    let undo_main = main.as_weak();
    let undo_timeline = timeline.as_weak();
    main.on_undo(move || {
        if let Some(workspace) = undo_model.borrow_mut().current_workspace_mut() {
            workspace.undo();
        }
        sync_weak_windows(&undo_main, &undo_timeline, &undo_model);
    });
    let redo_model = model.clone();
    let redo_main = main.as_weak();
    let redo_timeline = timeline.as_weak();
    main.on_redo(move || {
        if let Some(workspace) = redo_model.borrow_mut().current_workspace_mut() {
            workspace.redo();
        }
        sync_weak_windows(&redo_main, &redo_timeline, &redo_model);
    });

    let seek_model = model.clone();
    let seek_main = main.as_weak();
    let seek_timeline = timeline.as_weak();
    let seek_audio = audio_playback.clone();
    main.on_seek(move |frame| {
        if let Some(workspace) = seek_model.borrow_mut().current_workspace_mut() {
            workspace.seek(frame.round() as i32);
            seek_audio.borrow_mut().reset_queue();
        }
        sync_transport_weak(&seek_main, &seek_timeline, &seek_model);
    });
    let speed_model = model.clone();
    let speed_main = main.as_weak();
    let speed_timeline = timeline.as_weak();
    main.on_playback_speed_changed(move |percent| {
        if let Some(workspace) = speed_model.borrow_mut().current_workspace_mut() {
            workspace.set_playback_speed(f64::from(percent.clamp(10, 400)) / 100.0);
        }
        sync_transport_weak(&speed_main, &speed_timeline, &speed_model);
    });
    let previous_model = model.clone();
    let previous_main = main.as_weak();
    let previous_timeline = timeline.as_weak();
    let previous_audio = audio_playback.clone();
    main.on_previous_frame(move || {
        if let Some(workspace) = previous_model.borrow_mut().current_workspace_mut() {
            workspace.step_playhead(-1);
            previous_audio.borrow_mut().reset_queue();
        }
        sync_transport_weak(&previous_main, &previous_timeline, &previous_model);
    });
    let next_model = model.clone();
    let next_main = main.as_weak();
    let next_timeline = timeline.as_weak();
    let next_audio = audio_playback.clone();
    main.on_next_frame(move || {
        if let Some(workspace) = next_model.borrow_mut().current_workspace_mut() {
            workspace.step_playhead(1);
            next_audio.borrow_mut().reset_queue();
        }
        sync_transport_weak(&next_main, &next_timeline, &next_model);
    });
    let playback_model = model.clone();
    let playback_main = main.as_weak();
    let playback_timeline = timeline.as_weak();
    main.on_toggle_playback(move || {
        if let Some(workspace) = playback_model.borrow_mut().current_workspace_mut() {
            workspace.toggle_playback();
        }
        sync_transport_weak(&playback_main, &playback_timeline, &playback_model);
    });

    let scrub_begin_model = model.clone();
    let scrub_begin_audio = audio_playback.clone();
    timeline.on_begin_scrub(move || {
        if let Some(workspace) = scrub_begin_model.borrow_mut().current_workspace_mut() {
            workspace.begin_scrub();
            scrub_begin_audio.borrow_mut().reset_queue();
        }
    });
    let scrub_model = model.clone();
    let scrub_main = main.as_weak();
    let scrub_timeline = timeline.as_weak();
    let scrub_audio = audio_playback;
    timeline.on_scrub_to(move |frame| {
        if let Some(workspace) = scrub_model.borrow_mut().current_workspace_mut() {
            workspace.scrub_to(frame.round() as i32);
            scrub_audio.borrow_mut().reset_queue();
        }
        sync_transport_weak(&scrub_main, &scrub_timeline, &scrub_model);
    });
    let scrub_end_model = model;
    let scrub_end_main = main.as_weak();
    let scrub_end_timeline = timeline.as_weak();
    timeline.on_end_scrub(move || {
        if let Some(workspace) = scrub_end_model.borrow_mut().current_workspace_mut() {
            workspace.end_scrub();
        }
        sync_transport_weak(&scrub_end_main, &scrub_end_timeline, &scrub_end_model);
    });
}
