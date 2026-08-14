#include "commands.hpp"
#include "constants.hpp"
#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include "timeline_service.hpp"
#include <QDebug>

namespace AviQtl::UI {

namespace {

auto sceneGridModeName(std::uint32_t mode) -> QString {
    switch (static_cast<AviQtl::RustCore::SceneGridMode>(mode)) {
    case AviQtl::RustCore::SceneGridMode::Bpm:
        return QStringLiteral("BPM");
    case AviQtl::RustCore::SceneGridMode::Frame:
        return QStringLiteral("Frame");
    case AviQtl::RustCore::SceneGridMode::Auto:
        return QStringLiteral("Auto");
    }
    return QStringLiteral("Auto");
}

} // namespace

auto TimelineService::currentScene() -> SceneData * {
    if (m_currentSceneCache) {
        return m_currentSceneCache;
    }
    for (auto &scene : m_scenes) {
        if (scene.id == m_currentSceneId) {
            m_currentSceneCache = &scene;
            return &scene;
        }
    }
    Q_ASSERT(!m_scenes.isEmpty());
    return m_scenes.data();
}

auto TimelineService::currentScene() const -> const SceneData * {
    for (const auto &scene : std::as_const(m_scenes)) {
        if (scene.id == m_currentSceneId) {
            return &scene;
        }
    }
    Q_ASSERT(!m_scenes.isEmpty());
    return m_scenes.data();
}

auto TimelineService::scenes() const -> QVariantList {
    QVariantList list;
    list.reserve(m_scenes.size());
    for (const auto &scene : std::as_const(m_scenes)) {
        QVariantMap map;
        map.insert(QStringLiteral("id"), scene.id);
        map.insert(QStringLiteral("name"), scene.name);
        map.insert(QStringLiteral("width"), scene.width);
        map.insert(QStringLiteral("height"), scene.height);
        map.insert(QStringLiteral("fps"), scene.fps);
        map.insert(QStringLiteral("totalFrames"), scene.totalFrames);
        map.insert(QStringLiteral("gridMode"), scene.gridMode);
        map.insert(QStringLiteral("gridBpm"), scene.gridBpm);
        map.insert(QStringLiteral("gridOffset"), scene.gridOffset);
        map.insert(QStringLiteral("gridInterval"), scene.gridInterval);
        map.insert(QStringLiteral("gridSubdivision"), scene.gridSubdivision);
        map.insert(QStringLiteral("enableSnap"), scene.enableSnap);
        map.insert(QStringLiteral("magneticSnapRange"), scene.magneticSnapRange);
        list.append(map);
    }
    return list;
}

void TimelineService::setScenes(const QList<SceneData> &scenes) {
    for (auto &scene : m_scenes) {
        for (auto &clip : scene.clips) {
            for (auto *eff : std::as_const(clip.effects)) {
                if (eff)
                    eff->deleteLater();
            }
            clip.effects.clear();
        }
    }

    m_scenes = scenes;
    invalidateCurrentSceneCache();
    if (m_scenes.isEmpty()) {
        createScene(QObject::tr("ルート"));
    }
    // 現在のシーンIDが有効か確認
    bool found = false;
    for (const auto &s : std::as_const(m_scenes)) {
        if (s.id == m_currentSceneId) {
            found = true;
            break;
        }
    }
    if (!found && !m_scenes.isEmpty()) {
        m_currentSceneId = m_scenes.first().id;
    }
    emit scenesChanged();
    emit currentSceneIdChanged();
    emit clipsChanged();
}

void TimelineService::createScene(const QString &name) {
    const int id = allocateSceneId();
    if (id < 0) {
        return;
    }
    m_undoStack->push(new AddSceneCommand(this, id, name));
}

void TimelineService::removeScene(int sceneId) {
    for (const auto &s : getAllScenes()) {
        if (s.id == sceneId) {
            m_undoStack->push(new RemoveSceneCommand(this, sceneId, s.name));
            return;
        }
    }
}

void TimelineService::switchScene(int sceneId) {
    if (m_currentSceneId == sceneId) {
        return;
    }

    bool exists = false;
    for (const auto &s : std::as_const(m_scenes)) {
        if (s.id == sceneId) {
            exists = true;
            break;
        }
    }
    if (!exists) {
        return;
    }

    m_currentSceneId = sceneId;
    invalidateCurrentSceneCache();
    emit currentSceneIdChanged();
    emit clipsChanged();

    if (m_selection != nullptr) {
        m_selection->select(-1, QVariantMap());
    }
}

void TimelineService::updateSceneSettings(int sceneId, const QString &name, int width, int height, double fps, int totalFrames, const QString &gridMode, double gridBpm, double gridOffset, int gridInterval, int gridSubdivision,
                                          bool enableSnap, // NOLINT(bugprone-easily-swappable-parameters)
                                          int magneticSnapRange) {
    const auto sceneIt = std::ranges::find_if(m_scenes, [sceneId](const SceneData &scene) {
        return scene.id == sceneId;
    });
    if (sceneIt == m_scenes.end()) {
        return;
    }
    const AviQtl::RustCore::SceneSettings requested{
        .width = width,
        .height = height,
        .fps = fps,
        .total_frames = totalFrames,
        .grid_mode = static_cast<std::uint32_t>(AviQtl::RustCore::sceneGridMode(gridMode)),
        .grid_bpm = gridBpm,
        .grid_offset = gridOffset,
        .grid_interval = gridInterval,
        .grid_subdivision = gridSubdivision,
        .enable_snap = enableSnap ? 1U : 0U,
        .magnetic_snap_range = magneticSnapRange,
    };
    AviQtl::RustCore::SceneSettings normalized{};
    if (AviQtl::RustCore::normalizeSceneSettings(requested, normalized) !=
        AviQtl::RustCore::TimelineDomainStatus::Ok) {
        qWarning() << "Rust scene settings normalization failed";
        return;
    }

    const SceneData oldData = *sceneIt;
    SceneData newData = oldData;
    newData.name = name;
    newData.width = normalized.width;
    newData.height = normalized.height;
    newData.fps = normalized.fps;
    newData.totalFrames = normalized.total_frames;
    newData.gridMode = sceneGridModeName(normalized.grid_mode);
    newData.gridBpm = normalized.grid_bpm;
    newData.gridOffset = normalized.grid_offset;
    newData.gridInterval = normalized.grid_interval;
    newData.gridSubdivision = normalized.grid_subdivision;
    newData.enableSnap = normalized.enable_snap != 0;
    newData.magneticSnapRange = normalized.magnetic_snap_range;
    m_undoStack->push(new UpdateSceneSettingsCommand(this, sceneId, oldData, newData));
}

void TimelineService::createSceneInternal(int sceneId, const QString &name) {
    SceneData newScene;
    newScene.id = sceneId;
    newScene.name = name;
    const auto &settings = AviQtl::Core::SettingsManager::instance().settings();
    newScene.width = settings.value(QStringLiteral("defaultProjectWidth"), AviQtl::kDefaultWidth).toInt();
    newScene.height = settings.value(QStringLiteral("defaultProjectHeight"), AviQtl::kDefaultHeight).toInt();
    newScene.fps = settings.value(QStringLiteral("defaultProjectFps"), AviQtl::kDefaultFps).toDouble();
    m_scenes.append(newScene);
    invalidateCurrentSceneCache();
    emit scenesChanged();
    switchScene(newScene.id);
}

void TimelineService::removeSceneInternal(int sceneId) {
    if (sceneId == 0) {
        return;
    }
    auto it = std::ranges::find_if(m_scenes, [sceneId](const SceneData &s) -> bool { return s.id == sceneId; });
    if (it != m_scenes.end()) {
        for (auto &clip : it->clips) {
            for (auto *eff : std::as_const(clip.effects)) {
                eff->deleteLater();
            }
        }
        if (m_currentSceneId == sceneId) {
            switchScene(0);
        }
        m_scenes.erase(it);
        invalidateCurrentSceneCache();
        emit scenesChanged();
    }
}

void TimelineService::restoreSceneInternal(const SceneData &scene) {
    m_scenes.append(scene);
    invalidateCurrentSceneCache();
    emit scenesChanged();
}

void TimelineService::applySceneSettingsInternal(int sceneId, const SceneData &data) {
    for (auto &scene : m_scenes) {
        if (scene.id == sceneId) {
            scene.name = data.name;
            scene.width = data.width;
            scene.height = data.height;
            scene.fps = data.fps;
            scene.totalFrames = data.totalFrames;
            scene.gridMode = data.gridMode;
            scene.gridBpm = data.gridBpm;
            scene.gridOffset = data.gridOffset;
            scene.gridInterval = data.gridInterval;
            scene.gridSubdivision = data.gridSubdivision;
            scene.enableSnap = data.enableSnap;
            scene.magneticSnapRange = data.magneticSnapRange;
            emit scenesChanged();
            return;
        }
    }
}

} // namespace AviQtl::UI
