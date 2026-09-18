//! Background package operations, plugin discovery, and manager presentation.

use crate::localization::localized;
use crate::projection::update_vec_model;
use crate::{
    PackageData, PackageManagerWindow, PackageRepositoryData, PluginPermissionData,
    PluginPermissionWindow,
};
use aviqtl_app::audio_plugin::{AudioPluginScanOutcome, AudioPluginScanner};
use aviqtl_app::package_manager::{
    PackageManagerModel, PackageOperation, PackageOperationOutcome, PackageSection,
    UreqPackageHttpClient, plugin_permission_grants,
};
use aviqtl_app::settings::SettingsStore;
use slint::SharedString;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, mpsc};
use std::thread;
use std::thread::JoinHandle;

pub(super) struct AudioPluginDiscoveryRuntime {
    pub(super) stop: Arc<AtomicBool>,
    pub(super) receiver: Receiver<AudioPluginScanOutcome>,
    pub(super) worker: Option<JoinHandle<()>>,
}

pub(super) enum PackageOperationEvent {
    Progress {
        status: String,
        progress: f32,
    },
    Finished {
        model: Box<PackageManagerModel>,
        outcome: PackageOperationOutcome,
    },
}

pub(super) struct PackageOperationRuntime {
    pub(super) receiver: Receiver<PackageOperationEvent>,
    pub(super) worker: Option<JoinHandle<()>>,
}

impl PackageOperationRuntime {
    pub(super) fn start(
        model: PackageManagerModel,
        operation: PackageOperation,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("aviqtl-package-operation".to_owned())
            .spawn(move || {
                let mut model = model;
                let client = UreqPackageHttpClient::default();
                let progress_sender = sender.clone();
                let outcome =
                    model.execute_operation(operation, &client, move |status, progress| {
                        let _ = progress_sender.send(PackageOperationEvent::Progress {
                            status: status.to_owned(),
                            progress,
                        });
                    });
                let _ = sender.send(PackageOperationEvent::Finished {
                    model: Box::new(model),
                    outcome,
                });
            })
            .map_err(|error| format!("Failed to start package operation: {error}"))?;
        Ok(Self {
            receiver,
            worker: Some(worker),
        })
    }

    pub(super) fn poll(&mut self) -> Result<Option<PackageOperationEvent>, String> {
        match self.receiver.try_recv() {
            Ok(event) => {
                if matches!(event, PackageOperationEvent::Finished { .. })
                    && let Some(worker) = self.worker.take()
                {
                    let _ = worker.join();
                }
                Ok(Some(event))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
                Err("Package operation worker disconnected".to_owned())
            }
        }
    }
}

pub(super) fn start_package_operation(
    window: &PackageManagerWindow,
    runtime: &Rc<RefCell<Option<PackageOperationRuntime>>>,
    model: &Rc<RefCell<PackageManagerModel>>,
    operation: PackageOperation,
) {
    if runtime.borrow().is_some() {
        return;
    }
    window.set_error_message(SharedString::new());
    window.set_busy(true);
    window.set_progress(0.0);
    match PackageOperationRuntime::start(model.borrow().clone(), operation) {
        Ok(operation) => *runtime.borrow_mut() = Some(operation),
        Err(error) => {
            window.set_busy(false);
            window.set_error_message(SharedString::from(error));
        }
    }
}

impl AudioPluginDiscoveryRuntime {
    pub(super) fn start(settings: &SettingsStore) -> Result<Self, String> {
        let scanner = AudioPluginScanner::from_settings(settings);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("aviqtl-audio-plugin-discovery".to_owned())
            .spawn(move || {
                let result = scanner.scan(&worker_stop);
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Audio plugins · failed to start scanner: {error}"))?;
        Ok(Self {
            stop,
            receiver,
            worker: Some(worker),
        })
    }

    pub(super) fn poll(&mut self) -> Result<Option<AudioPluginScanOutcome>, String> {
        match self.receiver.try_recv() {
            Ok(result) => {
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
                Ok(Some(result))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
                Err("Audio plugin discovery worker disconnected".to_owned())
            }
        }
    }
}

impl Drop for AudioPluginDiscoveryRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) fn sync_package_manager(window: &PackageManagerWindow, model: &PackageManagerModel) {
    let section = match window.get_tab_index() {
        0 => Some(PackageSection::Effect),
        1 => Some(PackageSection::Object),
        2 => Some(PackageSection::Mod),
        3 => Some(PackageSection::Installed),
        4 => Some(PackageSection::Application),
        _ => None,
    };
    let packages = section
        .map(|section| model.packages(section, window.get_search_query().as_str()))
        .unwrap_or_default()
        .into_iter()
        .map(|package| PackageData {
            can_manage_permissions: package.can_manage_permissions(),
            can_remove: package.can_remove(),
            id: SharedString::from(package.id),
            package_type: SharedString::from(package.package_type),
            display_name: SharedString::from(package.display_name),
            description: SharedString::from(package.description),
            author: SharedString::from(package.author),
            version: SharedString::from(package.version),
            installed_version: SharedString::from(package.installed_version),
            latest_version: SharedString::from(package.latest_version),
            source_repository: SharedString::from(package.source_repository),
            local_file_plugin: package.local_file_plugin,
            has_update: package.has_update,
        })
        .collect::<Vec<_>>();
    update_vec_model(&window.get_packages(), packages);
    update_vec_model(
        &window.get_repositories(),
        model
            .repositories()
            .into_iter()
            .map(|repository| PackageRepositoryData {
                name: SharedString::from(repository.name),
                url: SharedString::from(repository.url),
                enabled: repository.enabled,
                priority: repository.priority,
            })
            .collect(),
    );
    window.set_has_updates(model.has_updates());
    window.set_status_text(SharedString::from(model.status()));
}

pub(super) fn sync_plugin_permissions(window: &PluginPermissionWindow, settings: &SettingsStore) {
    let rows = plugin_permission_grants(settings, window.get_plugin_id().as_str())
        .into_iter()
        .map(|permission| {
            let (title, description) = plugin_permission_metadata(&permission.name);
            PluginPermissionData {
                name: SharedString::from(permission.name),
                title: SharedString::from(title),
                description: SharedString::from(description),
                granted: permission.granted,
            }
        })
        .collect();
    update_vec_model(&window.get_permissions(), rows);
}

fn plugin_permission_metadata(name: &str) -> (&'static str, &'static str) {
    match name {
        "transport.control" => (
            localized("Playback control", "播放控制", "再生制御"),
            localized(
                "Play, pause, and seek",
                "播放、暂停和定位",
                "再生、一時停止、シーク",
            ),
        ),
        "clip.read" => (
            localized("Read clips", "读取剪辑", "クリップ読み取り"),
            localized(
                "List clip information",
                "列出剪辑信息",
                "クリップ情報の一覧表示",
            ),
        ),
        "clip.modify" => (
            localized("Modify clips", "修改剪辑", "クリップ変更"),
            localized(
                "Create, delete, and move clips",
                "创建、删除和移动剪辑",
                "クリップの作成、削除、移動",
            ),
        ),
        "effect.modify" => (
            localized("Modify effects", "修改特效", "エフェクト変更"),
            localized(
                "Add, delete, and change effects",
                "添加、删除和修改特效",
                "エフェクトの追加、削除、変更",
            ),
        ),
        "project.read" => (
            localized("Read project", "读取项目", "プロジェクト読み取り"),
            localized(
                "Read resolution, FPS, and other project information",
                "读取分辨率、FPS 等项目信息",
                "解像度、FPS等の情報取得",
            ),
        ),
        "project.save" => (
            localized("Save project", "保存项目", "プロジェクト保存"),
            localized(
                "Save project files",
                "保存项目文件",
                "プロジェクトファイルの保存",
            ),
        ),
        "project.load" => (
            localized("Load project", "加载项目", "プロジェクト読み込み"),
            localized(
                "Load project files",
                "加载项目文件",
                "プロジェクトファイルの読み込み",
            ),
        ),
        "scene.manage" => (
            localized("Manage scenes", "管理场景", "シーン管理"),
            localized(
                "Create, delete, and switch scenes",
                "创建、删除和切换场景",
                "シーンの作成、削除、切り替え",
            ),
        ),
        "settings.read" => (
            localized("Read settings", "读取设置", "設定読み取り"),
            localized(
                "Read plugin settings",
                "读取插件设置",
                "プラグイン設定の読み取り",
            ),
        ),
        "settings.write" => (
            localized("Write settings", "写入设置", "設定書き込み"),
            localized(
                "Save plugin settings",
                "保存插件设置",
                "プラグイン設定の保存",
            ),
        ),
        "clipboard.access" => (
            localized("Clipboard access", "剪贴板访问", "クリップボード"),
            localized(
                "Copy, cut, and paste",
                "复制、剪切和粘贴",
                "コピー、切り取り、貼り付け",
            ),
        ),
        "history.control" => (
            localized("History control", "历史记录控制", "履歴操作"),
            localized(
                "Undo, redo, and group commands",
                "撤销、重做和命令分组",
                "元に戻す、やり直し、コマンドのグループ化",
            ),
        ),
        "log.output" => (
            localized("Log output", "日志输出", "ログ出力"),
            localized(
                "Write messages to the console",
                "向控制台输出消息",
                "コンソールへのログ出力",
            ),
        ),
        _ => ("", ""),
    }
}
