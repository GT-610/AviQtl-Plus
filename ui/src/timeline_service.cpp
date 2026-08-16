#include "timeline_service.hpp"
#include "commands.hpp"
#include "constants.hpp"
#include "effect_registry.hpp"
#include "rust_project_document.hpp"
#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QPoint>
#include <algorithm>
#include <cstdint>
#include <utility>
#include <vector>

namespace AviQtl::UI {
namespace {

QVariantList sortedLayerList(const QSet<int> &layers) {
    QList<int> sorted(layers.cbegin(), layers.cend());
    std::sort(sorted.begin(), sorted.end());
    QVariantList result;
    result.reserve(sorted.size());
    for (int layer : std::as_const(sorted)) {
        result.append(layer);
    }
    return result;
}

QVariantMap effectDocument(const EffectModel *effect, QVariantMap existing = {}) {
    if (effect == nullptr) {
        return existing;
    }
    existing.insert(QStringLiteral("id"), effect->id());
    existing.insert(QStringLiteral("name"), effect->name());
    existing.insert(QStringLiteral("enabled"), effect->isEnabled());
    existing.insert(QStringLiteral("params"), effect->params());
    existing.insert(QStringLiteral("keyframes"), effect->keyframeTracks());
    return existing;
}

QVariantMap audioPluginDocument(const AudioPluginState &plugin, QVariantMap existing = {}) {
    existing.insert(QStringLiteral("id"), plugin.id);
    existing.insert(QStringLiteral("enabled"), plugin.enabled);
    existing.insert(QStringLiteral("params"), plugin.params);
    if (plugin.keyframeTracks.isEmpty()) {
        existing.remove(QStringLiteral("keyframes"));
    } else {
        existing.insert(QStringLiteral("keyframes"), plugin.keyframeTracks);
    }
    return existing;
}

QVariantMap projectionDocument(const QList<SceneData> &scenes, QVariantMap base) {
    if (base.isEmpty()) {
        base.insert(QStringLiteral("version"), AviQtl::RustCore::currentProjectVersion());
        QVariantMap settings;
        const SceneData *root = scenes.isEmpty() ? nullptr : &scenes.first();
        settings.insert(QStringLiteral("width"), root != nullptr ? root->width : AviQtl::kDefaultWidth);
        settings.insert(QStringLiteral("height"), root != nullptr ? root->height : AviQtl::kDefaultHeight);
        settings.insert(QStringLiteral("fps"), root != nullptr ? root->fps : AviQtl::kDefaultFps);
        settings.insert(QStringLiteral("sampleRate"), AviQtl::kDefaultSampleRate);
        base.insert(QStringLiteral("settings"), settings);
    }

    QHash<int, QVariantMap> existingScenes;
    for (const QVariant &value : base.value(QStringLiteral("scenes")).toList()) {
        const QVariantMap scene = value.toMap();
        existingScenes.insert(scene.value(QStringLiteral("id")).toInt(), scene);
    }
    QHash<int, QVariantMap> existingClips;
    for (const QVariant &value : base.value(QStringLiteral("clips")).toList()) {
        const QVariantMap clip = value.toMap();
        existingClips.insert(clip.value(QStringLiteral("id")).toInt(), clip);
    }

    QVariantList sceneDocuments;
    QVariantList clipDocuments;
    for (const SceneData &scene : scenes) {
        QVariantMap sceneObject = existingScenes.value(scene.id);
        sceneObject.insert(QStringLiteral("id"), scene.id);
        sceneObject.insert(QStringLiteral("name"), scene.name);
        sceneObject.insert(QStringLiteral("width"), scene.width);
        sceneObject.insert(QStringLiteral("height"), scene.height);
        sceneObject.insert(QStringLiteral("fps"), scene.fps);
        sceneObject.insert(QStringLiteral("start"), scene.startFrame);
        sceneObject.insert(QStringLiteral("duration"), scene.totalFrames);
        sceneObject.insert(QStringLiteral("nestedDuration"), scene.durationFrames);
        sceneObject.insert(QStringLiteral("lockedLayers"), sortedLayerList(scene.lockedLayers));
        sceneObject.insert(QStringLiteral("hiddenLayers"), sortedLayerList(scene.hiddenLayers));
        sceneObject.insert(QStringLiteral("gridMode"), scene.gridMode);
        sceneObject.insert(QStringLiteral("gridBpm"), scene.gridBpm);
        sceneObject.insert(QStringLiteral("gridOffset"), scene.gridOffset);
        sceneObject.insert(QStringLiteral("gridInterval"), scene.gridInterval);
        sceneObject.insert(QStringLiteral("gridSubdivision"), scene.gridSubdivision);
        sceneObject.insert(QStringLiteral("enableSnap"), scene.enableSnap);
        sceneObject.insert(QStringLiteral("magneticSnapRange"), scene.magneticSnapRange);
        sceneDocuments.append(sceneObject);

        for (const ClipData &clip : scene.clips) {
            QVariantMap clipObject = existingClips.value(clip.id);
            const QVariantList oldEffects = clipObject.value(QStringLiteral("effects")).toList();
            const QVariantList oldPlugins = clipObject.value(QStringLiteral("audioPlugins")).toList();
            clipObject.insert(QStringLiteral("id"), clip.id);
            clipObject.insert(QStringLiteral("sceneId"), clip.sceneId);
            clipObject.insert(QStringLiteral("type"), clip.type);
            clipObject.insert(QStringLiteral("start"), clip.startFrame);
            clipObject.insert(QStringLiteral("duration"), clip.durationFrames);
            clipObject.insert(QStringLiteral("layer"), clip.layer);
            clipObject.insert(QStringLiteral("clipByUpperObject"), clip.clipByUpperObject);
            clipObject.insert(QStringLiteral("params"), clip.params);

            QVariantList plugins;
            plugins.reserve(clip.audioPlugins.size());
            for (qsizetype index = 0; index < clip.audioPlugins.size(); ++index) {
                plugins.append(audioPluginDocument(
                    clip.audioPlugins.at(index),
                    index < oldPlugins.size() ? oldPlugins.at(index).toMap() : QVariantMap()));
            }
            clipObject.insert(QStringLiteral("audioPlugins"), plugins);

            QVariantList effects;
            effects.reserve(clip.effects.size());
            for (qsizetype index = 0; index < clip.effects.size(); ++index) {
                effects.append(effectDocument(
                    clip.effects.at(index),
                    index < oldEffects.size() ? oldEffects.at(index).toMap() : QVariantMap()));
            }
            clipObject.insert(QStringLiteral("effects"), effects);
            clipDocuments.append(clipObject);
        }
    }
    base.insert(QStringLiteral("scenes"), sceneDocuments);
    base.insert(QStringLiteral("clips"), clipDocuments);
    return base;
}

QVariantMap jsonObject(const QByteArray &bytes) {
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(bytes, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject()) {
        return {};
    }
    return document.object().toVariantMap();
}

QByteArray compactJson(const QVariantMap &value) {
    return QJsonDocument(QJsonObject::fromVariantMap(value)).toJson(QJsonDocument::Compact);
}

} // namespace

TimelineService::TimelineService(SelectionService *selection, QObject *parent) : QObject(parent), m_undoStack(new QUndoStack(this)), m_selection(selection) {
    const auto &settings = AviQtl::Core::SettingsManager::instance().settings();
    m_undoStack->setUndoLimit(settings.value(QStringLiteral("undoCount"), 32).toInt());

    // 初期シーンを作成
    SceneData rootScene;
    rootScene.id = 0;
    rootScene.name = QObject::tr("ルート");
    rootScene.width = settings.value(QStringLiteral("defaultProjectWidth"), AviQtl::kDefaultWidth).toInt();
    rootScene.height = settings.value(QStringLiteral("defaultProjectHeight"), AviQtl::kDefaultHeight).toInt();
    rootScene.fps = settings.value(QStringLiteral("defaultProjectFps"), AviQtl::kDefaultFps).toDouble();
    m_scenes.append(rootScene);
    if (!resetTimelineState(projectionDocument(m_scenes, {}))) {
        qWarning() << "Failed to initialize Rust timeline state";
    }
}

TimelineService::~TimelineService() {
    for (auto &scene : m_scenes) {
        for (auto &clip : scene.clips) {
            for (auto *eff : clip.effects) {
                if (eff)
                    eff->deleteLater();
            }
        }
    }
    for (auto &clip : m_clipboard) {
        for (auto *eff : clip.effects) {
            if (eff)
                eff->deleteLater();
        }
    }
}

void TimelineService::undo() { m_undoStack->undo(); }
void TimelineService::redo() { m_undoStack->redo(); }

QVariantMap TimelineService::timelineStateSnapshot() const {
    QByteArray output;
    const auto status = m_timelineState.snapshot(output);
    if (status != AviQtl::RustCore::TimelineStateStatus::Ok) {
        qWarning() << "Rust timeline state snapshot failed:" << static_cast<std::uint32_t>(status);
        return {};
    }
    const QVariantMap result = jsonObject(output);
    if (result.isEmpty()) {
        qWarning() << "Rust timeline state returned invalid JSON";
    }
    return result;
}

bool TimelineService::resetTimelineState(const QVariantMap &document, int nextClipHint,
                                         int nextSceneHint) {
    const auto status = m_timelineState.reset(compactJson(document), nextClipHint, nextSceneHint);
    if (status != AviQtl::RustCore::TimelineStateStatus::Ok) {
        qWarning() << "Rust timeline state reset failed:" << static_cast<std::uint32_t>(status);
        return false;
    }
    return true;
}

bool TimelineService::commitTimelineProjection() {
    if (m_timelineProjectionTransactionDepth > 0) {
        return true;
    }
    QVariantMap current = timelineStateSnapshot();
    if (current.isEmpty()) {
        int nextClipHint = std::max(1, m_timelineState.nextClipId());
        int nextSceneHint = std::max(1, m_timelineState.nextSceneId());
        for (const auto &scene : std::as_const(m_scenes)) {
            nextSceneHint = std::max(nextSceneHint, scene.id + 1);
            for (const auto &clip : scene.clips) {
                nextClipHint = std::max(nextClipHint, clip.id + 1);
            }
        }
        return resetTimelineState(projectionDocument(m_scenes, {}), nextClipHint,
                                  nextSceneHint);
    }
    const QVariantMap proposed = projectionDocument(m_scenes, current);
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("replace_document")},
        {QStringLiteral("document"), proposed},
    };
    QByteArray transactionBytes;
    auto status = m_timelineState.plan(compactJson(request), transactionBytes);
    if (status != AviQtl::RustCore::TimelineStateStatus::Ok) {
        qWarning() << "Rust timeline state transaction planning failed:"
                   << static_cast<std::uint32_t>(status);
        return false;
    }
    const QVariantMap transaction = jsonObject(transactionBytes);
    const QVariantMap forward = transaction.value(QStringLiteral("forward")).toMap();
    if (forward.isEmpty()) {
        qWarning() << "Rust timeline state returned an invalid transaction";
        return false;
    }
    status = m_timelineState.applyPatch(compactJson(forward));
    if (status != AviQtl::RustCore::TimelineStateStatus::Ok) {
        qWarning() << "Rust timeline state transaction failed:" << static_cast<std::uint32_t>(status);
        return false;
    }
    return true;
}

bool TimelineService::commitTimelineMutation(std::function<void()> rollback,
                                             std::function<void()> commitAction) {
    if (m_timelineProjectionTransactionDepth > 0) {
        m_timelineProjectionRollbacks.append(std::move(rollback));
        m_timelineProjectionCommitActions.append(std::move(commitAction));
        return true;
    }
    if (!commitTimelineProjection()) {
        if (rollback) {
            rollback();
        }
        return false;
    }
    if (commitAction) {
        commitAction();
    }
    return true;
}

void TimelineService::beginTimelineProjectionTransaction() {
    if (m_timelineProjectionTransactionDepth == 0) {
        m_timelineProjectionRollbacks.clear();
        m_timelineProjectionCommitActions.clear();
    }
    ++m_timelineProjectionTransactionDepth;
}

bool TimelineService::endTimelineProjectionTransaction() {
    if (m_timelineProjectionTransactionDepth <= 0) {
        qWarning() << "Unbalanced timeline projection transaction";
        m_timelineProjectionTransactionDepth = 0;
        return false;
    }
    --m_timelineProjectionTransactionDepth;
    if (m_timelineProjectionTransactionDepth > 0) {
        return true;
    }

    auto rollbacks = std::move(m_timelineProjectionRollbacks);
    auto commitActions = std::move(m_timelineProjectionCommitActions);
    m_timelineProjectionRollbacks.clear();
    m_timelineProjectionCommitActions.clear();
    if (commitTimelineProjection()) {
        for (auto &action : commitActions) {
            if (action) {
                action();
            }
        }
        return true;
    }
    for (auto it = rollbacks.rbegin(); it != rollbacks.rend(); ++it) {
        if (*it) {
            (*it)();
        }
    }
    return false;
}

int TimelineService::allocateClipId() {
    const QList<int> ids = allocateClipIds(1);
    return ids.isEmpty() ? -1 : ids.first();
}

QList<int> TimelineService::allocateClipIds(qsizetype count) {
    if (count <= 0) {
        return {};
    }
    std::vector<std::int32_t> ids;
    if (m_timelineState.reserveClipIds(static_cast<std::size_t>(count), ids) !=
            AviQtl::RustCore::TimelineStateStatus::Ok ||
        ids.size() != static_cast<std::size_t>(count)) {
        qWarning() << "Rust timeline state clip ID reservation failed";
        return {};
    }
    return QList<int>(ids.cbegin(), ids.cend());
}

int TimelineService::allocateSceneId() {
    std::vector<std::int32_t> ids;
    if (m_timelineState.reserveSceneIds(1, ids) !=
            AviQtl::RustCore::TimelineStateStatus::Ok ||
        ids.size() != 1) {
        qWarning() << "Rust timeline state scene ID reservation failed";
        return -1;
    }
    return ids.front();
}

} // namespace AviQtl::UI
