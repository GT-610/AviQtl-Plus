//! Object settings commands and metadata-to-Slint model projection.

use crate::dialogs::{show_and_redraw, show_error_dialog, slint_color};
use crate::easing::{
    parameter_button_label, sync_easing_catalog, sync_easing_preview, sync_easing_window,
};
use crate::localization::{localized, localized_effect_categories, localized_effect_metadata};
use crate::projection::{sync_project_tabs, sync_transport, sync_weak_windows, update_vec_model};
use crate::{
    EasingConfigWindow, EffectCatalogItemData, KeyframeMarkerData, MainWindow,
    ObjectCatalogMenuCategoryData, ObjectEffectData, ObjectSettingRowData, ObjectSettingsWindow,
    TimelineWindow,
};
use aviqtl_app::audio_plugin::AudioPluginCatalog;
use aviqtl_app::easing::BezierCurve;
use aviqtl_app::effect_catalog::EffectCatalog;
use aviqtl_app::object_settings::{ObjectControl, ObjectControlKind, ObjectSettings};
use aviqtl_app::preset_store::PresetStore;
use aviqtl_app::{ApplicationModel, WorkspaceModel};
use slint::{Color, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectSettingsSyncKey {
    pub(super) project_instance_id: u64,
    pub(super) document_revision: u64,
    pub(super) clip_id: i32,
    pub(super) playhead: i32,
    pub(super) effect_selection: Vec<bool>,
}

#[derive(Clone)]
pub(super) struct ObjectSettingsUi {
    pub(super) main: slint::Weak<MainWindow>,
    pub(super) timeline: slint::Weak<TimelineWindow>,
    pub(super) window: slint::Weak<ObjectSettingsWindow>,
    pub(super) easing: slint::Weak<EasingConfigWindow>,
    pub(super) easing_curve: Rc<RefCell<BezierCurve>>,
    pub(super) model: Rc<RefCell<ApplicationModel>>,
    pub(super) catalog: Rc<RefCell<EffectCatalog>>,
    pub(super) audio_catalog: Rc<RefCell<aviqtl_app::audio_plugin::AudioPluginCatalog>>,
    pub(super) presets: Rc<PresetStore>,
    pub(super) font_families: Rc<Vec<String>>,
    pub(super) settings: Rc<RefCell<aviqtl_app::settings::SettingsStore>>,
}

impl ObjectSettingsUi {
    pub(super) fn sync(&self) {
        sync_weak_windows(&self.main, &self.timeline, &self.model);
        if let Some(window) = self.window.upgrade() {
            let model = self.model.borrow();
            let catalog = self.catalog.borrow();
            sync_object_settings(&window, &model, &catalog);
            sync_object_catalog(
                &window,
                &model,
                &catalog,
                &self.audio_catalog.borrow(),
                window.get_effect_filter().as_str(),
            );
            self.project_catalog_preferences(&window);
        }
    }

    pub(super) fn catalog_preferences(&self, key: &str) -> Vec<String> {
        self.settings
            .borrow()
            .value(key)
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn catalog_key(&self, id: &str) -> String {
        let audio = self
            .window
            .upgrade()
            .is_some_and(|window| window.get_audio_plugin_mode());
        format!("{}:{id}", if audio { "audio" } else { "effect" })
    }

    pub(super) fn project_catalog_preferences(&self, window: &ObjectSettingsWindow) {
        let favorites = self.catalog_preferences("favoriteEffects");
        let recent = self.catalog_preferences("recentEffects");
        let mut items: Vec<_> = window
            .get_effect_catalog_items()
            .iter()
            .filter(|item| !item.header)
            .filter(|item| match window.get_effect_catalog_mode() {
                1 => favorites.contains(&self.catalog_key(item.id.as_str())),
                2 => recent.contains(&self.catalog_key(item.id.as_str())),
                _ => true,
            })
            .collect();
        if window.get_effect_catalog_mode() == 2 {
            items.sort_by_key(|item| {
                recent
                    .iter()
                    .position(|id| id == &self.catalog_key(item.id.as_str()))
                    .unwrap_or(usize::MAX)
            });
        }
        let rows = items
            .iter()
            .map(|item| {
                slint::language::StandardListViewItem::from(slint::SharedString::from(format!(
                    "{}{}  {}",
                    if favorites.contains(&self.catalog_key(item.id.as_str())) {
                        "★ "
                    } else {
                        ""
                    },
                    item.name,
                    item.categories
                )))
            })
            .collect();
        update_vec_model(&window.get_effect_catalog_items(), items);
        update_vec_model(&window.get_effect_picker_rows(), rows);
        window.set_effect_picker_current(if window.get_effect_catalog_items().row_count() == 0 {
            -1
        } else {
            0
        });
    }

    pub(super) fn remember_catalog_item(&self, id: &str, favorite: bool) {
        let key = if favorite {
            "favoriteEffects"
        } else {
            "recentEffects"
        };
        let id = self.catalog_key(id);
        let mut items = self.catalog_preferences(key);
        let existed = items.contains(&id);
        items.retain(|item| item != &id);
        if !favorite || !existed {
            items.insert(0, id);
        }
        if !favorite {
            items.truncate(24);
        }
        let mut settings = self.settings.borrow().snapshot();
        settings.insert(key.to_owned(), serde_json::json!(items));
        if let Err(message) = self.settings.borrow_mut().apply(settings) {
            show_error_dialog(&message);
        }
    }

    pub(super) fn sync_lightweight(&self) {
        if let (Some(main), Some(timeline)) = (self.main.upgrade(), self.timeline.upgrade()) {
            let model = self.model.borrow();
            sync_project_tabs(&main, &model);
            sync_transport(&main, &timeline, &model);
        }
    }

    pub(super) fn control(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
    ) -> Option<ObjectControl> {
        let projection = {
            let model = self.model.borrow();
            let catalog = self.catalog.borrow();
            model.current_workspace()?.object_settings(&catalog)?
        };
        let controls = if audio_plugin {
            &projection.audio_plugins.get(effect_index)?.controls
        } else {
            &projection.effects.get(effect_index)?.controls
        };
        controls
            .iter()
            .find(|control| control.param.as_deref() == Some(param_name))
            .cloned()
    }

    pub(super) fn set_value(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        value: serde_json::Value,
    ) {
        let changed = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if audio_plugin {
                    workspace.set_audio_plugin_parameter_at_frame(
                        effect_index,
                        param_name,
                        frame,
                        value,
                    )
                } else {
                    workspace.set_effect_parameter_at_frame(effect_index, param_name, frame, value)
                }
            });
        if changed {
            self.sync();
        }
    }

    pub(super) fn set_value_deferred(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        value: serde_json::Value,
    ) {
        let changed = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.edit_continuously(
                    &format!("{audio_plugin}:{effect_index}:{param_name}:{frame}"),
                    |workspace| {
                        if audio_plugin {
                            workspace.set_audio_plugin_parameter_at_frame(
                                effect_index,
                                param_name,
                                frame,
                                value,
                            )
                        } else {
                            workspace.set_effect_parameter_at_frame(
                                effect_index,
                                param_name,
                                frame,
                                value,
                            )
                        }
                    },
                )
            });
        if changed {
            self.sync_lightweight();
        }
    }

    pub(super) fn set_text(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        text: &str,
    ) {
        let Some(control) = self.control(audio_plugin, effect_index, param_name) else {
            return;
        };
        match control.parse_text(text) {
            Ok(value) => self.set_value(audio_plugin, effect_index, param_name, frame, value),
            Err(message) => show_error_dialog(&message),
        }
    }

    pub(super) fn set_number(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        value: f32,
    ) {
        if !value.is_finite() {
            return;
        }
        self.set_value_deferred(
            audio_plugin,
            effect_index,
            param_name,
            frame,
            serde_json::Value::from(f64::from(value)),
        );
    }

    pub(super) fn set_start_number(
        &self,
        effect_index: usize,
        param_name: &str,
        start_frame: i32,
        end_frame: i32,
        value: f32,
    ) {
        if !value.is_finite() {
            return;
        }
        let changed = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.edit_continuously(
                    &format!("start:{effect_index}:{param_name}:{start_frame}:{end_frame}"),
                    |workspace| {
                        workspace.set_effect_interval_start_parameter_at_frame(
                            effect_index,
                            param_name,
                            start_frame,
                            end_frame,
                            serde_json::Value::from(f64::from(value)),
                        )
                    },
                )
            });
        if changed {
            self.sync_lightweight();
        }
    }

    pub(super) fn set_option(
        &self,
        audio_plugin: bool,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        option_index: usize,
    ) {
        let Some(value) = self
            .control(audio_plugin, effect_index, param_name)
            .and_then(|control| control.option_value(option_index))
        else {
            return;
        };
        self.set_value(audio_plugin, effect_index, param_name, frame, value);
    }

    pub(super) fn open_easing(
        &self,
        effect_index: usize,
        param_name: &str,
        start_frame: i32,
        end_frame: i32,
    ) {
        let point = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .and_then(|workspace| {
                workspace.prepare_effect_easing(effect_index, param_name, start_frame, end_frame)
            });
        let Some(point) = point else {
            return;
        };
        self.sync();
        if let Some(window) = self.easing.upgrade() {
            let curve = sync_easing_window(&window, effect_index, param_name, &point);
            *self.easing_curve.borrow_mut() = curve;
            let curve = self.easing_curve.borrow();
            sync_easing_preview(&window, &curve);
            sync_easing_catalog(&window, window.get_easing_filter().as_str(), &curve);
            let _ = show_and_redraw(&window);
        }
    }

    pub(super) fn update_easing_custom_points(&self, controls: [f32; 4]) -> Vec<f64> {
        let mut curve = self.easing_curve.borrow_mut();
        curve.set_first_controls(controls.map(f64::from));
        curve.points().to_vec()
    }

    pub(super) fn apply_easing(
        &self,
        effect_index: usize,
        param_name: &str,
        frame: i32,
        options: serde_json::Value,
    ) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                workspace.set_effect_keyframe_options(effect_index, param_name, frame, options)
            });
        self.sync();
    }

    pub(super) fn add_effect(&self, effect_id: &str) {
        let audio_plugin = self
            .model
            .borrow()
            .current_workspace()
            .is_some_and(WorkspaceModel::object_settings_uses_audio_plugins);
        if audio_plugin {
            match self.audio_catalog.borrow().addition(effect_id) {
                Ok(addition) => {
                    let _ = self
                        .model
                        .borrow_mut()
                        .current_workspace_mut()
                        .is_some_and(|workspace| workspace.add_audio_plugin(addition));
                }
                Err(message) => show_error_dialog(&message),
            }
        } else {
            let catalog = self.catalog.borrow();
            let _ = self
                .model
                .borrow_mut()
                .current_workspace_mut()
                .is_some_and(|workspace| workspace.add_effect(&catalog, effect_id));
        }
        self.sync();
    }

    pub(super) fn reorder_effect(&self, source: usize, delta_y: f32) {
        if !delta_y.is_finite() {
            return;
        }
        let Some(length) = self
            .model
            .borrow()
            .current_workspace()
            .and_then(WorkspaceModel::selected_clip_document)
            .map(|clip| {
                if clip.clip_type == "audio" {
                    clip.audio_plugins.len()
                } else {
                    clip.effects.len()
                }
            })
        else {
            return;
        };
        if length == 0 || source >= length {
            return;
        }
        let target = (source as i64 + i64::from((delta_y / 34.0).round() as i32))
            .clamp(0, length.saturating_sub(1) as i64) as usize;
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.reorder_audio_plugins(source, target)
                } else {
                    workspace.reorder_effects(source, target)
                }
            });
        self.sync();
    }

    pub(super) fn save_preset(&self, effect_index: usize, name: &str) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.save_audio_plugin_preset(&self.presets, effect_index, name)
                } else {
                    workspace.save_effect_preset(&self.presets, effect_index, name)
                }
            });
        self.sync();
    }

    pub(super) fn load_preset(&self, effect_index: usize, name: &str) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.load_audio_plugin_preset(&self.presets, effect_index, name)
                } else {
                    workspace.load_effect_preset(&self.presets, effect_index, name)
                }
            });
        self.sync();
    }

    pub(super) fn delete_preset(&self, effect_index: usize, name: &str) {
        let _ = self
            .model
            .borrow_mut()
            .current_workspace_mut()
            .is_some_and(|workspace| {
                if workspace.object_settings_uses_audio_plugins() {
                    workspace.delete_audio_plugin_preset(&self.presets, effect_index, name)
                } else {
                    workspace.delete_effect_preset(&self.presets, effect_index, name)
                }
            });
        self.sync();
    }
}

pub(super) fn object_settings_sync_key(model: &ApplicationModel) -> Option<ObjectSettingsSyncKey> {
    let project_instance_id = model.current_project_instance_id()?;
    let workspace = model.current_workspace()?;
    let clip = workspace.selected_clip_document()?;
    Some(ObjectSettingsSyncKey {
        project_instance_id,
        document_revision: workspace.document_revision(),
        clip_id: clip.id,
        playhead: workspace.playhead(),
        effect_selection: (0..if clip.clip_type == "audio" {
            clip.audio_plugins.len()
        } else {
            clip.effects.len()
        })
            .map(|index| workspace.effect_is_selected(index))
            .collect(),
    })
}

pub(super) fn sync_object_settings(
    window: &ObjectSettingsWindow,
    model: &ApplicationModel,
    catalog: &EffectCatalog,
) {
    window.set_selection_key(
        object_settings_sync_key(model)
            .map(|key| format!("{}:{}", key.project_instance_id, key.clip_id))
            .unwrap_or_default()
            .into(),
    );
    let projection = model
        .current_workspace()
        .and_then(|workspace| workspace.object_settings(catalog));
    let Some(projection) = projection else {
        window.set_has_selection(false);
        window.set_clip_title(SharedString::new());
        window.set_audio_plugin_mode(false);
        window.set_selected_effect_count(0);
        window.set_selected_effects_removable(false);
        update_vec_model(&window.get_effects(), Vec::new());
        update_vec_model(&window.get_setting_rows(), Vec::new());
        return;
    };
    window.set_has_selection(true);
    window.set_audio_plugin_mode(projection.audio_plugin_mode);
    window.set_clip_title(SharedString::from(format!(
        "{}  (ID {})",
        localized_effect_metadata(&projection.clip_label),
        projection.clip_id
    )));
    let effects = if projection.audio_plugin_mode {
        window.set_selected_effect_count(
            projection
                .audio_plugins
                .iter()
                .filter(|plugin| plugin.selected)
                .count() as i32,
        );
        window.set_selected_effects_removable(
            projection
                .audio_plugins
                .iter()
                .any(|plugin| plugin.selected),
        );
        projection
            .audio_plugins
            .iter()
            .map(|plugin| ObjectEffectData {
                index: plugin.index as i32,
                id: SharedString::from(plugin.id.clone()),
                name: SharedString::from(plugin.name.clone()),
                enabled: plugin.enabled,
                selected: plugin.selected,
                removable: true,
            })
            .collect::<Vec<_>>()
    } else {
        window.set_selected_effect_count(
            projection
                .effects
                .iter()
                .filter(|effect| effect.selected)
                .count() as i32,
        );
        window.set_selected_effects_removable(
            projection
                .effects
                .iter()
                .any(|effect| effect.selected && effect.removable),
        );
        projection
            .effects
            .iter()
            .map(|effect| ObjectEffectData {
                index: effect.index as i32,
                id: SharedString::from(effect.id.clone()),
                name: SharedString::from(localized_effect_metadata(&effect.name).into_owned()),
                enabled: effect.enabled,
                selected: effect.selected,
                removable: effect.removable,
            })
            .collect::<Vec<_>>()
    };
    update_vec_model(&window.get_effects(), effects);
    let current_rows = window.get_setting_rows();
    let folded: std::collections::BTreeSet<_> = current_rows
        .iter()
        .filter(|row| row.row_kind == "effect" && row.folded)
        .map(|row| (row.audio_plugin, row.effect_index))
        .collect();
    let mut rows = object_settings_rows(&projection);
    for row in &mut rows {
        row.folded = folded.contains(&(row.audio_plugin, row.effect_index));
    }
    update_vec_model(&current_rows, rows);
}

pub(super) fn sync_effect_catalog(
    window: &ObjectSettingsWindow,
    catalog: &EffectCatalog,
    query: &str,
) {
    let query = query.trim().to_lowercase();
    let items = catalog
        .query("effect", "", "")
        .into_iter()
        .filter(|metadata| {
            query.is_empty()
                || metadata.id.to_lowercase().contains(&query)
                || metadata.name.to_lowercase().contains(&query)
                || localized_effect_metadata(&metadata.name)
                    .to_lowercase()
                    .contains(&query)
                || metadata.categories.iter().any(|category| {
                    category.to_lowercase().contains(&query)
                        || localized_effect_metadata(category)
                            .to_lowercase()
                            .contains(&query)
                })
        })
        .map(|metadata| EffectCatalogItemData {
            header: false,
            id: SharedString::from(metadata.id.clone()),
            name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
            categories: SharedString::from(localized_effect_categories(&metadata.categories)),
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_effect_catalog_items(), items);
}

pub(super) fn initialize_timeline_object_catalog(window: &TimelineWindow, catalog: &EffectCatalog) {
    let categories = std::iter::once(SharedString::from(localized(
        "All categories",
        "所有分类",
        "すべてのカテゴリ",
    )))
    .chain(
        catalog
            .categories("object")
            .into_iter()
            .map(|category| SharedString::from(localized_effect_metadata(&category).into_owned())),
    )
    .collect::<Vec<_>>();
    update_vec_model(&window.get_object_catalog_categories(), categories);
    let all_items = catalog
        .query("object", "", "")
        .into_iter()
        .map(|metadata| EffectCatalogItemData {
            header: false,
            id: SharedString::from(metadata.id.clone()),
            name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
            categories: SharedString::from(localized_effect_categories(&metadata.categories)),
        })
        .collect::<Vec<_>>();
    let mut menu_categories = vec![ObjectCatalogMenuCategoryData {
        name: SharedString::from(localized("All categories", "所有分类", "すべてのカテゴリ")),
        items: ModelRc::new(VecModel::from(all_items.clone())),
    }];
    menu_categories.extend(catalog.categories("object").into_iter().map(|category| {
        ObjectCatalogMenuCategoryData {
            name: SharedString::from(localized_effect_metadata(&category).into_owned()),
            items: ModelRc::new(VecModel::from(
                catalog
                    .query("object", "", &category)
                    .into_iter()
                    .map(|metadata| EffectCatalogItemData {
                        header: false,
                        id: SharedString::from(metadata.id.clone()),
                        name: SharedString::from(
                            localized_effect_metadata(&metadata.name).into_owned(),
                        ),
                        categories: SharedString::from(localized_effect_categories(
                            &metadata.categories,
                        )),
                    })
                    .collect::<Vec<_>>(),
            )),
        }
    }));
    update_vec_model(
        &window.get_object_catalog_menu_categories(),
        menu_categories,
    );
    sync_timeline_object_catalog(window, catalog, "", 0);
}

pub(super) fn sync_timeline_object_catalog(
    window: &TimelineWindow,
    catalog: &EffectCatalog,
    query: &str,
    category_index: i32,
) {
    let categories = catalog.categories("object");
    let category = usize::try_from(category_index)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| categories.get(index))
        .map_or("", String::as_str);
    let query = query.trim().to_lowercase();
    let items = catalog
        .query("object", "", category)
        .into_iter()
        .filter(|metadata| {
            query.is_empty()
                || metadata.id.to_lowercase().contains(&query)
                || metadata.name.to_lowercase().contains(&query)
                || localized_effect_metadata(&metadata.name)
                    .to_lowercase()
                    .contains(&query)
                || metadata.categories.iter().any(|category| {
                    category.to_lowercase().contains(&query)
                        || localized_effect_metadata(category)
                            .to_lowercase()
                            .contains(&query)
                })
        })
        .map(|metadata| EffectCatalogItemData {
            header: false,
            id: SharedString::from(metadata.id.clone()),
            name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
            categories: SharedString::from(localized_effect_categories(&metadata.categories)),
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_object_catalog_items(), items);
}

pub(super) fn sync_timeline_context_catalog(
    window: &TimelineWindow,
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    query: &str,
    target_kind: i32,
) {
    update_vec_model(
        &window.get_context_catalog_items(),
        timeline_context_catalog_items(effect_catalog, audio_catalog, query, target_kind),
    );
    update_vec_model(
        &window.get_context_catalog_categories(),
        timeline_context_catalog_categories(effect_catalog, audio_catalog, target_kind),
    );
}

/// Group the unfiltered context catalog into Qt's category submenus.
///
/// Qt builds one submenu per category for effects (buildEffectMenu) and for audio
/// plugins (buildAudioPluginMenu), so insertion walks a tree instead of one flat
/// list. Only populated when the search query is empty; the search popup projects
/// its results from the flat model.
pub(super) fn timeline_context_catalog_categories(
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    target_kind: i32,
) -> Vec<ObjectCatalogMenuCategoryData> {
    if target_kind == 2 {
        let mut categories: Vec<(String, Vec<EffectCatalogItemData>)> = Vec::new();
        for plugin in audio_catalog.entries("") {
            let entry = EffectCatalogItemData {
                header: false,
                id: SharedString::from(plugin.id),
                name: SharedString::from(plugin.name),
                categories: SharedString::from(plugin.category.clone()),
            };
            match categories
                .iter_mut()
                .find(|(name, _)| *name == plugin.category)
            {
                Some((_, items)) => items.push(entry),
                None => categories.push((plugin.category, vec![entry])),
            }
        }
        return categories
            .into_iter()
            .map(|(name, items)| ObjectCatalogMenuCategoryData {
                name: SharedString::from(name),
                items: ModelRc::new(VecModel::from(items)),
            })
            .collect();
    }
    if target_kind != 1 {
        return Vec::new();
    }
    let mut categories: Vec<(String, Vec<EffectCatalogItemData>)> = Vec::new();
    for metadata in effect_catalog.query("effect", "", "") {
        let category = metadata
            .categories
            .first()
            .map(String::as_str)
            .unwrap_or_default()
            .to_owned();
        let entry = EffectCatalogItemData {
            header: false,
            id: SharedString::from(metadata.id.clone()),
            name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
            categories: SharedString::from(localized_effect_categories(&metadata.categories)),
        };
        match categories.iter_mut().find(|(name, _)| *name == category) {
            Some((_, items)) => items.push(entry),
            None => categories.push((category, vec![entry])),
        }
    }
    categories
        .into_iter()
        .map(|(name, items)| ObjectCatalogMenuCategoryData {
            name: SharedString::from(localized_effect_metadata(&name).into_owned()),
            items: ModelRc::new(VecModel::from(items)),
        })
        .collect()
}

/// Resolve the highlighted result after an Up/Down press in the context-menu search.
///
/// `selected` is the current highlight, `-1` when nothing is highlighted yet.
/// A first press enters the list from the end the movement heads towards, and
/// every result clamps at both ends. An empty result set clears the highlight.
pub(super) fn moved_context_search_selection(selected: i32, count: usize, step: i32) -> i32 {
    if count == 0 {
        return -1;
    }
    let last = count as i32 - 1;
    let start = if selected < 0 {
        if step > 0 { -1 } else { count as i32 }
    } else {
        selected
    };
    (start + step).clamp(0, last)
}

pub(super) fn timeline_context_catalog_items(
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    query: &str,
    target_kind: i32,
) -> Vec<EffectCatalogItemData> {
    if target_kind == 2 {
        audio_catalog
            .entries(query)
            .into_iter()
            .map(|plugin| EffectCatalogItemData {
                header: false,
                id: SharedString::from(plugin.id),
                name: SharedString::from(plugin.name),
                categories: SharedString::from(plugin.category),
            })
            .collect::<Vec<_>>()
    } else {
        let kind = if target_kind == 0 { "object" } else { "effect" };
        let query = query.trim().to_lowercase();
        effect_catalog
            .query(kind, "", "")
            .into_iter()
            .filter(|metadata| {
                query.is_empty()
                    || metadata.id.to_lowercase().contains(&query)
                    || metadata.name.to_lowercase().contains(&query)
                    || localized_effect_metadata(&metadata.name)
                        .to_lowercase()
                        .contains(&query)
                    || metadata.categories.iter().any(|category| {
                        category.to_lowercase().contains(&query)
                            || localized_effect_metadata(category)
                                .to_lowercase()
                                .contains(&query)
                    })
            })
            .map(|metadata| EffectCatalogItemData {
                header: false,
                id: SharedString::from(metadata.id.clone()),
                name: SharedString::from(localized_effect_metadata(&metadata.name).into_owned()),
                categories: SharedString::from(localized_effect_categories(&metadata.categories)),
            })
            .collect::<Vec<_>>()
    }
}

fn sync_audio_plugin_catalog(
    window: &ObjectSettingsWindow,
    catalog: &AudioPluginCatalog,
    query: &str,
) {
    let mut items = Vec::new();
    let mut previous_category = None::<String>;
    for plugin in catalog.entries(query) {
        if query.trim().is_empty() && previous_category.as_deref() != Some(plugin.category.as_str())
        {
            previous_category = Some(plugin.category.clone());
            items.push(EffectCatalogItemData {
                header: true,
                id: SharedString::new(),
                name: SharedString::from(plugin.category.clone()),
                categories: SharedString::new(),
            });
        }
        items.push(EffectCatalogItemData {
            header: false,
            id: SharedString::from(plugin.id),
            name: SharedString::from(plugin.name),
            categories: SharedString::from(plugin.category),
        });
    }
    update_vec_model(&window.get_effect_catalog_items(), items);
}

pub(super) fn sync_object_catalog(
    window: &ObjectSettingsWindow,
    model: &ApplicationModel,
    effect_catalog: &EffectCatalog,
    audio_catalog: &AudioPluginCatalog,
    query: &str,
) {
    if model
        .current_workspace()
        .is_some_and(WorkspaceModel::object_settings_uses_audio_plugins)
    {
        sync_audio_plugin_catalog(window, audio_catalog, query);
    } else {
        sync_effect_catalog(window, effect_catalog, query);
    }
}

pub(super) fn object_settings_rows(settings: &ObjectSettings) -> Vec<ObjectSettingRowData> {
    let mut rows = Vec::new();
    for effect in &settings.effects {
        push_object_settings_rows(
            &mut rows,
            effect.index,
            &effect.name,
            effect.enabled,
            effect.selected,
            effect.removable && !settings.audio_plugin_mode,
            !settings.audio_plugin_mode,
            false,
            &effect.controls,
        );
    }
    for plugin in &settings.audio_plugins {
        let label = if plugin.format.is_empty() {
            plugin.name.clone()
        } else {
            format!("{} ({})", plugin.name, plugin.format)
        };
        push_object_settings_rows(
            &mut rows,
            plugin.index,
            &label,
            plugin.enabled,
            plugin.selected,
            true,
            false,
            true,
            &plugin.controls,
        );
    }
    rows
}

#[allow(clippy::too_many_arguments)]
fn push_object_settings_rows(
    rows: &mut Vec<ObjectSettingRowData>,
    index: usize,
    name: &str,
    enabled: bool,
    selected: bool,
    removable: bool,
    header_toggle_visible: bool,
    audio_plugin: bool,
    controls: &[ObjectControl],
) {
    rows.push(ObjectSettingRowData {
        row_kind: SharedString::from("effect"),
        folded: false,
        source_kind: SharedString::new(),
        audio_plugin,
        effect_index: index as i32,
        param_name: SharedString::new(),
        label: SharedString::from(localized_effect_metadata(name).into_owned()),
        effect_enabled: enabled,
        header_toggle_visible,
        selected,
        removable,
        interactive: true,
        checked: false,
        keyframed: false,
        range_mode: false,
        right_interactive: false,
        start_frame: 0,
        end_frame: 0,
        current_frame: 0,
        clip_duration: 0,
        interpolation: SharedString::new(),
        parameter_button_label: SharedString::new(),
        number_value: 0.0,
        end_number_value: 0.0,
        minimum: 0.0,
        maximum: 0.0,
        step: 1.0,
        text_value: SharedString::new(),
        end_text_value: SharedString::new(),
        filter: SharedString::new(),
        color_value: Color::from_rgb_u8(255, 255, 255),
        end_color_value: Color::from_rgb_u8(255, 255, 255),
        unit: SharedString::new(),
        keyframe_markers: ModelRc::new(VecModel::<KeyframeMarkerData>::default()),
        option_labels: ModelRc::new(VecModel::<SharedString>::default()),
        selected_option: -1,
    });
    for control in controls {
        let (minimum, maximum) = object_control_range(control);
        let localized_label = localized_effect_metadata(&control.label).into_owned();
        let option_labels = control
            .options
            .iter()
            .map(|option| SharedString::from(localized_effect_metadata(&option.label).into_owned()))
            .collect::<Vec<_>>();
        let supports_track = matches!(
            control.kind,
            ObjectControlKind::Number | ObjectControlKind::Integer | ObjectControlKind::Color
        ) && control.param.is_some();
        let start_value = if audio_plugin {
            &control.value
        } else {
            &control.start_value
        };
        let text_value = control.display_value_at(start_value);
        let end_text_value = control.display_value_at(&control.end_value);
        let end_keyframe_exists = control
            .keyframes
            .iter()
            .any(|point| point.frame == control.interval_end);
        let right_interactive = !audio_plugin
            && supports_track
            && control.keyframed
            && end_keyframe_exists
            && !matches!(
                control.start_interpolation.as_str(),
                "" | "constant" | "none"
            );
        let parameter_row = ObjectSettingRowData {
            row_kind: SharedString::from(control.kind.as_str()),
            folded: false,
            source_kind: SharedString::from(control.source_kind.clone()),
            audio_plugin,
            effect_index: index as i32,
            param_name: SharedString::from(control.param.clone().unwrap_or_default()),
            label: SharedString::from(localized_label.clone()),
            effect_enabled: enabled,
            header_toggle_visible,
            selected,
            removable,
            interactive: (audio_plugin || enabled) && !control.disabled && control.param.is_some(),
            checked: control.bool_value_at(start_value),
            keyframed: control.keyframed,
            range_mode: !audio_plugin && supports_track && control.keyframed,
            right_interactive,
            start_frame: if audio_plugin {
                control.relative_frame
            } else {
                control.interval_start
            },
            end_frame: control.interval_end,
            current_frame: control.relative_frame,
            clip_duration: control.clip_duration,
            interpolation: SharedString::from(control.start_interpolation.clone()),
            parameter_button_label: SharedString::from(parameter_button_label(
                &localized_label,
                control.keyframed,
                &control.start_interpolation,
            )),
            number_value: finite_f32(control.number_value_at(start_value), 0.0),
            end_number_value: finite_f32(control.number_value_at(&control.end_value), 0.0),
            minimum,
            maximum,
            step: control
                .step
                .filter(|step| step.is_finite() && *step > 0.0)
                .map_or(if audio_plugin { 0.001 } else { 1.0 }, |step| {
                    finite_f32(step, if audio_plugin { 0.001 } else { 1.0 })
                }),
            text_value: SharedString::from(text_value.clone()),
            end_text_value: SharedString::from(end_text_value.clone()),
            filter: SharedString::from(control.filter.clone()),
            color_value: slint_color(&text_value),
            end_color_value: slint_color(&end_text_value),
            unit: SharedString::from(control.unit.clone()),
            keyframe_markers: ModelRc::new(VecModel::from(keyframe_markers(control, audio_plugin))),
            option_labels: ModelRc::new(VecModel::from(option_labels)),
            selected_option: control
                .selected_option_at(start_value)
                .map_or(-1, |option| option as i32),
        };
        rows.push(parameter_row.clone());
        if supports_track {
            rows.push(ObjectSettingRowData {
                row_kind: SharedString::from("keyframes"),
                label: SharedString::new(),
                ..parameter_row
            });
        }
    }
}

pub(super) fn keyframe_markers(
    control: &ObjectControl,
    audio_plugin: bool,
) -> Vec<KeyframeMarkerData> {
    let mut points = control
        .keyframes
        .iter()
        .map(|point| (point.frame, false))
        .collect::<Vec<_>>();
    if !audio_plugin
        && control.clip_duration > 0
        && !points
            .iter()
            .any(|(frame, _)| *frame == control.clip_duration)
    {
        points.push((control.clip_duration, true));
    }
    points.sort_unstable_by_key(|(frame, _)| *frame);
    points.dedup_by_key(|(frame, _)| *frame);
    points
        .iter()
        .enumerate()
        .map(|(index, (frame, virtual_end))| {
            let minimum_frame = index
                .checked_sub(1)
                .and_then(|previous| points.get(previous))
                .map_or(0, |(frame, _)| frame.saturating_add(1));
            let maximum_frame = points
                .get(index + 1)
                .map_or(control.clip_duration, |(frame, _)| frame.saturating_sub(1));
            KeyframeMarkerData {
                frame: *frame,
                minimum_frame,
                maximum_frame,
                virtual_end: *virtual_end,
                removable: *frame != 0
                    && !virtual_end
                    && !(audio_plugin && *frame == control.clip_duration),
                draggable: !audio_plugin && *frame != 0 && !virtual_end,
            }
        })
        .collect()
}

fn object_control_range(control: &ObjectControl) -> (f32, f32) {
    let minimum = control.minimum.unwrap_or(-100_000.0);
    let maximum = control.maximum.unwrap_or(100_000.0);
    if minimum.is_finite() && maximum.is_finite() && minimum <= maximum {
        (
            finite_f32(minimum, -100_000.0),
            finite_f32(maximum, 100_000.0),
        )
    } else {
        (-100_000.0, 100_000.0)
    }
}

pub(super) fn finite_f32(value: f64, fallback: f32) -> f32 {
    if value.is_finite() && value >= f64::from(f32::MIN) && value <= f64::from(f32::MAX) {
        value as f32
    } else {
        fallback
    }
}
