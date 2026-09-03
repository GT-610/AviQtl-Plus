#include "commands.hpp"
#include "constants.hpp"
#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include "timeline_service.hpp"
#include <QDebug>
#include <algorithm>

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

QSet<EffectModel *> sceneEffects(const QList<SceneData> &scenes) {
    QSet<EffectModel *> effects;
    for (const auto &scene : scenes) {
        for (const auto &clip : scene.clips) {
            for (auto *effect : clip.effects) {
                if (effect != nullptr) {
                    effects.insert(effect);
                }
            }
        }
    }
    return effects;
}

void deleteUnretainedEffects(const QList<SceneData> &source,
                             const QList<SceneData> &retainedScenes) {
    const QSet<EffectModel *> retained = sceneEffects(retainedScenes);
    for (auto *effect : sceneEffects(source)) {
        if (!retained.contains(effect)) {
            effect->deleteLater();
        }
    }
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

bool TimelineService::setScenes(const QList<SceneData> &scenes) {
    const QList<SceneData> previousScenes = m_scenes;
    const int previousCurrentSceneId = m_currentSceneId;
    m_scenes = scenes;
    invalidateCurrentSceneCache();
    if (m_scenes.isEmpty()) {
        const auto &settings = AviQtl::Core::SettingsManager::instance().settings();
        SceneData rootScene;
        rootScene.id = 0;
        rootScene.name = QObject::tr("ルート");
        rootScene.width = settings.value(QStringLiteral("defaultProjectWidth"), AviQtl::kDefaultWidth).toInt();
        rootScene.height = settings.value(QStringLiteral("defaultProjectHeight"), AviQtl::kDefaultHeight).toInt();
        rootScene.fps = settings.value(QStringLiteral("defaultProjectFps"), AviQtl::kDefaultFps).toDouble();
        m_scenes.append(rootScene);
        invalidateCurrentSceneCache();
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
    if (!commitTimelineProjection()) {
        const QList<SceneData> rejectedScenes = m_scenes;
        m_scenes = previousScenes;
        m_currentSceneId = previousCurrentSceneId;
        invalidateCurrentSceneCache();
        deleteUnretainedEffects(rejectedScenes, m_scenes);
        qWarning() << "Failed to commit loaded scenes to Rust timeline state";
        return false;
    }
    deleteUnretainedEffects(previousScenes, m_scenes);
    emit scenesChanged();
    emit currentSceneIdChanged();
    emit clipsChanged();
    return true;
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
    const qsizetype insertedIndex = m_scenes.size();
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("insert_scene")},
        {QStringLiteral("scene"), timelineSceneDocument(newScene)},
    };
    if (!commitTimelineStructureMutation(
            request,
            [this, newScene]() {
                if (std::ranges::any_of(m_scenes, [sceneId = newScene.id](const SceneData &scene) {
                        return scene.id == sceneId;
                    })) {
                    return false;
                }
                m_scenes.append(newScene);
                invalidateCurrentSceneCache();
                return true;
            },
            [this, sceneId, insertedIndex]() {
                if (insertedIndex < m_scenes.size() &&
                    m_scenes.at(insertedIndex).id == sceneId) {
                    m_scenes.removeAt(insertedIndex);
                    invalidateCurrentSceneCache();
                    return true;
                }
                const auto sceneIt = std::ranges::find_if(
                    m_scenes, [sceneId](const SceneData &scene) { return scene.id == sceneId; });
                if (sceneIt == m_scenes.end()) {
                    return false;
                }
                m_scenes.erase(sceneIt);
                invalidateCurrentSceneCache();
                return true;
            })) {
        qWarning() << "Rust rejected scene creation";
        return;
    }
    emit scenesChanged();
    switchScene(newScene.id);
}

void TimelineService::removeSceneInternal(int sceneId) {
    if (sceneId == 0) {
        return;
    }
    auto it = std::ranges::find_if(m_scenes, [sceneId](const SceneData &s) -> bool { return s.id == sceneId; });
    if (it != m_scenes.end()) {
        const SceneData removed = *it;
        const auto removedIndex = std::distance(m_scenes.begin(), it);
        const QVariantMap request{
            {QStringLiteral("operation"), QStringLiteral("remove_scene")},
            {QStringLiteral("scene_id"), sceneId},
        };
        if (!commitTimelineStructureMutation(
                request,
                [this, sceneId]() {
                    const auto removedIt = std::ranges::find_if(
                        m_scenes,
                        [sceneId](const SceneData &scene) { return scene.id == sceneId; });
                    if (removedIt == m_scenes.end()) {
                        return false;
                    }
                    m_scenes.erase(removedIt);
                    invalidateCurrentSceneCache();
                    return true;
                },
                [this, removedIndex, removed]() {
                    m_scenes.insert(std::min<qsizetype>(removedIndex, m_scenes.size()), removed);
                    invalidateCurrentSceneCache();
                    return true;
                },
                [removed]() {
                    for (const auto &clip : removed.clips) {
                        for (auto *effect : clip.effects) {
                            if (effect != nullptr) {
                                effect->deleteLater();
                            }
                        }
                    }
                })) {
            qWarning() << "Rust rejected scene removal";
            return;
        }
        if (m_currentSceneId == sceneId) {
            switchScene(0);
        }
        emit scenesChanged();
    }
}

bool TimelineService::restoreSceneProjectionsInternal(
    const QList<SceneProjectionRestore> &restores) {
    return replaceSceneProjectionsInternal({}, restores);
}

bool TimelineService::removeSceneProjectionsInternal(const QList<int> &sceneIds) {
    return replaceSceneProjectionsInternal(sceneIds, {});
}

bool TimelineService::replaceSceneProjectionsInternal(
    const QList<int> &removeIds, const QList<SceneProjectionRestore> &restores) {
    QSet<int> removedIds;
    for (int sceneId : removeIds) {
        const bool exists = std::ranges::any_of(
            m_scenes, [sceneId](const SceneData &scene) { return scene.id == sceneId; });
        if (sceneId < 0 || removedIds.contains(sceneId) || !exists) {
            return false;
        }
        removedIds.insert(sceneId);
    }

    QSet<int> restoredIds;
    for (const auto &restore : restores) {
        const bool exists = std::ranges::any_of(
            m_scenes, [&restore](const SceneData &scene) {
                return scene.id == restore.scene.id;
            });
        if (restore.scene.id < 0 || restoredIds.contains(restore.scene.id) ||
            (exists && !removedIds.contains(restore.scene.id))) {
            return false;
        }
        restoredIds.insert(restore.scene.id);
    }

    QList<SceneProjectionRestore> prepared;
    prepared.reserve(restores.size());
    for (const auto &restore : restores) {
        SceneData scene = deepCopyScene(restore.scene);
        scene.id = restore.scene.id;
        prepared.append({.scene = std::move(scene), .index = restore.index});
    }

    QList<SceneData> removed;
    removed.reserve(removeIds.size());
    for (int sceneId : removeIds) {
        const auto sceneIt = std::ranges::find_if(
            m_scenes, [sceneId](const SceneData &scene) { return scene.id == sceneId; });
        removed.append(*sceneIt);
        m_scenes.erase(sceneIt);
    }

    std::ranges::sort(prepared, [](const SceneProjectionRestore &left,
                                   const SceneProjectionRestore &right) {
        return left.index < right.index;
    });
    for (const auto &restore : std::as_const(prepared)) {
        const qsizetype index = std::clamp<qsizetype>(restore.index, 0, m_scenes.size());
        m_scenes.insert(index, restore.scene);
    }

    for (const auto &scene : std::as_const(removed)) {
        for (const auto &clip : scene.clips) {
            for (auto *effect : clip.effects) {
                if (effect != nullptr) {
                    effect->deleteLater();
                }
            }
        }
    }
    invalidateCurrentSceneCache();
    return true;
}

void TimelineService::applySceneSettingsInternal(int sceneId, const SceneData &data) {
    const auto sceneIt = std::ranges::find_if(
        m_scenes, [sceneId](const SceneData &scene) { return scene.id == sceneId; });
    if (sceneIt == m_scenes.end()) {
        return;
    }

    SceneData updated = *sceneIt;
    updated.name = data.name;
    updated.width = data.width;
    updated.height = data.height;
    updated.fps = data.fps;
    updated.totalFrames = data.totalFrames;
    updated.gridMode = data.gridMode;
    updated.gridBpm = data.gridBpm;
    updated.gridOffset = data.gridOffset;
    updated.gridInterval = data.gridInterval;
    updated.gridSubdivision = data.gridSubdivision;
    updated.enableSnap = data.enableSnap;
    updated.magneticSnapRange = data.magneticSnapRange;
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("update_scene")},
        {QStringLiteral("scene_id"), sceneId},
        {QStringLiteral("scene"), timelineSceneDocument(updated)},
    };
    if (!commitTimelineStateMutation(request)) {
        qWarning() << "Rust rejected scene settings update";
        return;
    }
    emit scenesChanged();
}

auto TimelineService::deepCopyScene(const SceneData &source) -> SceneData {
    SceneData scene = source;
    scene.clips.clear();
    scene.clips.reserve(source.clips.size());
    for (const auto &clip : source.clips) {
        ClipData copied = deepCopyClip(clip);
        copied.id = clip.id;
        scene.clips.append(std::move(copied));
    }
    return scene;
}

} // namespace AviQtl::UI
