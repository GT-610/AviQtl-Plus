#include "timeline_service.hpp"
#include "commands.hpp"
#include "constants.hpp"
#include "effect_registry.hpp"
#include "rust_project_document.hpp"
#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include <QDebug>
#include <QHash>
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

QVariantMap sceneDocument(const SceneData &scene, QVariantMap existing = {}) {
    existing.insert(QStringLiteral("id"), scene.id);
    existing.insert(QStringLiteral("name"), scene.name);
    existing.insert(QStringLiteral("width"), scene.width);
    existing.insert(QStringLiteral("height"), scene.height);
    existing.insert(QStringLiteral("fps"), scene.fps);
    existing.insert(QStringLiteral("start"), scene.startFrame);
    existing.insert(QStringLiteral("duration"), scene.totalFrames);
    existing.insert(QStringLiteral("nestedDuration"), scene.durationFrames);
    existing.insert(QStringLiteral("lockedLayers"), sortedLayerList(scene.lockedLayers));
    existing.insert(QStringLiteral("hiddenLayers"), sortedLayerList(scene.hiddenLayers));
    existing.insert(QStringLiteral("gridMode"), scene.gridMode);
    existing.insert(QStringLiteral("gridBpm"), scene.gridBpm);
    existing.insert(QStringLiteral("gridOffset"), scene.gridOffset);
    existing.insert(QStringLiteral("gridInterval"), scene.gridInterval);
    existing.insert(QStringLiteral("gridSubdivision"), scene.gridSubdivision);
    existing.insert(QStringLiteral("enableSnap"), scene.enableSnap);
    existing.insert(QStringLiteral("magneticSnapRange"), scene.magneticSnapRange);
    return existing;
}

QVariantMap clipDocument(const ClipData &clip, QVariantMap existing = {}) {
    const QVariantList oldEffects = existing.value(QStringLiteral("effects")).toList();
    const QVariantList oldPlugins = existing.value(QStringLiteral("audioPlugins")).toList();
    existing.insert(QStringLiteral("id"), clip.id);
    existing.insert(QStringLiteral("sceneId"), clip.sceneId);
    existing.insert(QStringLiteral("type"), clip.type);
    existing.insert(QStringLiteral("start"), clip.startFrame);
    existing.insert(QStringLiteral("duration"), clip.durationFrames);
    existing.insert(QStringLiteral("layer"), clip.layer);
    existing.insert(QStringLiteral("clipByUpperObject"), clip.clipByUpperObject);
    existing.insert(QStringLiteral("params"), clip.params);

    QVariantList plugins;
    plugins.reserve(clip.audioPlugins.size());
    for (qsizetype index = 0; index < clip.audioPlugins.size(); ++index) {
        plugins.append(audioPluginDocument(
            clip.audioPlugins.at(index),
            index < oldPlugins.size() ? oldPlugins.at(index).toMap() : QVariantMap()));
    }
    existing.insert(QStringLiteral("audioPlugins"), plugins);

    QVariantList effects;
    effects.reserve(clip.effects.size());
    for (qsizetype index = 0; index < clip.effects.size(); ++index) {
        effects.append(effectDocument(
            clip.effects.at(index),
            index < oldEffects.size() ? oldEffects.at(index).toMap() : QVariantMap()));
    }
    existing.insert(QStringLiteral("effects"), effects);
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
        sceneDocuments.append(sceneDocument(scene, existingScenes.value(scene.id)));

        for (const ClipData &clip : scene.clips) {
            clipDocuments.append(clipDocument(clip, existingClips.value(clip.id)));
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

QVariantMap TimelineService::timelineSceneDocument(const SceneData &scene) const {
    return sceneDocument(scene);
}

QVariantMap TimelineService::timelineClipDocument(const ClipData &clip) const {
    return clipDocument(clip);
}

bool TimelineService::applyTimelinePatch(const QVariantMap &patch) {
    if (patch.isEmpty()) {
        return false;
    }
    const auto status = m_timelineState.applyPatch(compactJson(patch));
    if (status != AviQtl::RustCore::TimelineStateStatus::Ok) {
        qWarning() << "Rust timeline state patch failed:" << static_cast<std::uint32_t>(status);
        return false;
    }
    return true;
}

bool TimelineService::synchronizeTimelineProjection() {
    const QVariantMap document = timelineStateSnapshot();
    if (document.isEmpty()) {
        return false;
    }

    const QVariantList sceneDocuments = document.value(QStringLiteral("scenes")).toList();
    const QVariantList clipDocuments = document.value(QStringLiteral("clips")).toList();
    QHash<int, QVariantMap> scenesById;
    QHash<int, QVariantMap> clipsById;
    QHash<int, qsizetype> clipCountsByScene;
    for (const QVariant &value : sceneDocuments) {
        const QVariantMap scene = value.toMap();
        scenesById.insert(scene.value(QStringLiteral("id")).toInt(), scene);
    }
    for (const QVariant &value : clipDocuments) {
        const QVariantMap clip = value.toMap();
        clipsById.insert(clip.value(QStringLiteral("id")).toInt(), clip);
        ++clipCountsByScene[clip.value(QStringLiteral("sceneId")).toInt()];
    }

    // Targeted requests change collection membership in the same adapter operation that owns
    // QObject lifetimes. Reconcile only matching objects here, so Rust supplies their committed
    // value state without this path constructing or destroying native runtime objects.
    bool sceneSetMatches = scenesById.size() == m_scenes.size();
    for (auto &scene : m_scenes) {
        const auto sceneIt = scenesById.constFind(scene.id);
        if (sceneIt == scenesById.cend()) {
            sceneSetMatches = false;
            continue;
        }
        const QVariantMap &source = sceneIt.value();
        scene.name = source.value(QStringLiteral("name"), scene.name).toString();
        scene.width = source.value(QStringLiteral("width"), scene.width).toInt();
        scene.height = source.value(QStringLiteral("height"), scene.height).toInt();
        scene.fps = source.value(QStringLiteral("fps"), scene.fps).toDouble();
        scene.startFrame = source.value(QStringLiteral("start"), scene.startFrame).toInt();
        scene.totalFrames = source.value(QStringLiteral("duration"), scene.totalFrames).toInt();
        scene.durationFrames =
            source.value(QStringLiteral("nestedDuration"), scene.durationFrames).toInt();
        scene.lockedLayers.clear();
        for (const QVariant &layer : source.value(QStringLiteral("lockedLayers")).toList()) {
            scene.lockedLayers.insert(layer.toInt());
        }
        scene.hiddenLayers.clear();
        for (const QVariant &layer : source.value(QStringLiteral("hiddenLayers")).toList()) {
            scene.hiddenLayers.insert(layer.toInt());
        }
        scene.gridMode = source.value(QStringLiteral("gridMode"), scene.gridMode).toString();
        scene.gridBpm = source.value(QStringLiteral("gridBpm"), scene.gridBpm).toDouble();
        scene.gridOffset = source.value(QStringLiteral("gridOffset"), scene.gridOffset).toDouble();
        scene.gridInterval = source.value(QStringLiteral("gridInterval"), scene.gridInterval).toInt();
        scene.gridSubdivision = source.value(QStringLiteral("gridSubdivision"), scene.gridSubdivision).toInt();
        scene.enableSnap = source.value(QStringLiteral("enableSnap"), scene.enableSnap).toBool();
        scene.magneticSnapRange =
            source.value(QStringLiteral("magneticSnapRange"), scene.magneticSnapRange).toInt();

        for (auto &clip : scene.clips) {
            const auto clipIt = clipsById.constFind(clip.id);
            if (clipIt == clipsById.cend()) {
                sceneSetMatches = false;
                continue;
            }
            const QVariantMap &sourceClip = clipIt.value();
            const int previousDuration = clip.durationFrames;
            clip.sceneId = sourceClip.value(QStringLiteral("sceneId"), clip.sceneId).toInt();
            clip.type = sourceClip.value(QStringLiteral("type"), clip.type).toString();
            clip.startFrame = sourceClip.value(QStringLiteral("start"), clip.startFrame).toInt();
            clip.durationFrames = sourceClip.value(QStringLiteral("duration"), clip.durationFrames).toInt();
            const bool durationChanged = clip.durationFrames != previousDuration;
            clip.layer = sourceClip.value(QStringLiteral("layer"), clip.layer).toInt();
            clip.clipByUpperObject =
                sourceClip.value(QStringLiteral("clipByUpperObject"), clip.clipByUpperObject).toBool();
            const QVariantMap clipParams =
                sourceClip.value(QStringLiteral("params"), clip.params).toMap();
            if (compactJson(clip.params) != compactJson(clipParams)) {
                clip.params = clipParams;
            }

            const QVariantList effects = sourceClip.value(QStringLiteral("effects")).toList();
            if (effects.size() != clip.effects.size()) {
                qWarning() << "Rust timeline projection effect count mismatch for clip" << clip.id;
                sceneSetMatches = false;
            } else {
                for (qsizetype index = 0; index < effects.size(); ++index) {
                    auto *effect = clip.effects.at(index);
                    const QVariantMap effectDocument = effects.at(index).toMap();
                    if (effect == nullptr ||
                        effect->id() != effectDocument.value(QStringLiteral("id")).toString()) {
                        qWarning() << "Rust timeline projection effect mismatch for clip" << clip.id;
                        sceneSetMatches = false;
                        continue;
                    }
                    effect->setEnabled(
                        effectDocument.value(QStringLiteral("enabled"), effect->isEnabled()).toBool());
                    const QVariantMap effectParams =
                        effectDocument.value(QStringLiteral("params"), effect->params()).toMap();
                    if (compactJson(effect->params()) != compactJson(effectParams)) {
                        effect->setParams(effectParams);
                    }
                    const QVariantMap keyframes =
                        effectDocument.value(QStringLiteral("keyframes")).toMap();
                    if (durationChanged ||
                        compactJson(effect->keyframeTracks()) != compactJson(keyframes)) {
                        effect->setKeyframeTracks(keyframes, clip.durationFrames);
                    }
                }
            }

            const QVariantList plugins = sourceClip.value(QStringLiteral("audioPlugins")).toList();
            if (plugins.size() != clip.audioPlugins.size()) {
                qWarning() << "Rust timeline projection plugin count mismatch for clip" << clip.id;
                sceneSetMatches = false;
            } else {
                for (qsizetype index = 0; index < plugins.size(); ++index) {
                    AudioPluginState &plugin = clip.audioPlugins[index];
                    const QVariantMap pluginDocument = plugins.at(index).toMap();
                    if (plugin.id != pluginDocument.value(QStringLiteral("id")).toString()) {
                        qWarning() << "Rust timeline projection plugin mismatch for clip" << clip.id;
                        sceneSetMatches = false;
                        continue;
                    }
                    const bool enabled =
                        pluginDocument.value(QStringLiteral("enabled"), plugin.enabled).toBool();
                    const QVariantMap pluginParams =
                        pluginDocument.value(QStringLiteral("params"), plugin.params).toMap();
                    const QVariantMap keyframes =
                        pluginDocument.value(QStringLiteral("keyframes")).toMap();
                    const bool paramsChanged =
                        compactJson(plugin.params) != compactJson(pluginParams);
                    const bool keyframesChanged =
                        compactJson(plugin.keyframeTracks) != compactJson(keyframes);
                    plugin.enabled = enabled;
                    if (paramsChanged) {
                        plugin.params = pluginParams;
                    }
                    if (keyframesChanged) {
                        plugin.keyframeTracks = keyframes;
                    }
                    if (paramsChanged || keyframesChanged) {
                        plugin.invalidateKeyframeCache();
                    }
                }
            }
        }
        if (scene.clips.size() != clipCountsByScene.value(scene.id)) {
            sceneSetMatches = false;
        }
    }

    if (!sceneSetMatches) {
        qWarning() << "Rust timeline projection structure mismatch";
    }
    invalidateCurrentSceneCache();
    return sceneSetMatches;
}

bool TimelineService::applyTimelineEditRequest(const QVariantMap &request,
                                               QVariantMap &inversePatch) {
    inversePatch.clear();
    QByteArray transactionBytes;
    const auto status = m_timelineState.plan(compactJson(request), transactionBytes);
    if (status != AviQtl::RustCore::TimelineStateStatus::Ok) {
        qWarning() << "Rust targeted timeline edit planning failed:"
                   << static_cast<std::uint32_t>(status);
        return false;
    }
    const QVariantMap transaction = jsonObject(transactionBytes);
    const QVariantMap forward = transaction.value(QStringLiteral("forward")).toMap();
    inversePatch = transaction.value(QStringLiteral("inverse")).toMap();
    if (forward.isEmpty() || inversePatch.isEmpty()) {
        qWarning() << "Rust targeted timeline edit returned an invalid transaction";
        inversePatch.clear();
        return false;
    }
    if (!applyTimelinePatch(forward)) {
        inversePatch.clear();
        return false;
    }
    return true;
}

bool TimelineService::commitTimelineMutation(const QVariantMap &request,
                                             std::function<void()> rollback,
                                             std::function<void()> commitAction) {
    if (m_timelineProjectionTransactionDepth > 0) {
        m_timelineProjectionRequests.append(request);
        m_timelineProjectionRollbacks.append(std::move(rollback));
        m_timelineProjectionCommitActions.append(std::move(commitAction));
        return true;
    }
    QVariantMap inversePatch;
    if (!applyTimelineEditRequest(request, inversePatch)) {
        if (rollback) {
            rollback();
        }
        return false;
    }
    if (!synchronizeTimelineProjection()) {
        if (!applyTimelinePatch(inversePatch)) {
            qWarning() << "Failed to roll back a Rust targeted timeline edit";
        }
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

bool TimelineService::commitTimelineEdit(const QVariantMap &request,
                                         std::function<void()> rollback,
                                         std::function<void()> commitAction) {
    return commitTimelineMutation(request, std::move(rollback), std::move(commitAction));
}

void TimelineService::beginTimelineProjectionTransaction() {
    if (m_timelineProjectionTransactionDepth == 0) {
        m_timelineProjectionTransactionAborted = false;
        m_timelineProjectionRequests.clear();
        m_timelineProjectionRollbacks.clear();
        m_timelineProjectionCommitActions.clear();
    }
    ++m_timelineProjectionTransactionDepth;
}

void TimelineService::abortTimelineProjectionTransaction() {
    if (m_timelineProjectionTransactionDepth <= 0) {
        qWarning() << "Cannot abort an inactive timeline projection transaction";
        return;
    }
    m_timelineProjectionTransactionAborted = true;
}

bool TimelineService::endTimelineProjectionTransaction() {
    if (m_timelineProjectionTransactionDepth <= 0) {
        qWarning() << "Unbalanced timeline projection transaction";
        m_timelineProjectionTransactionDepth = 0;
        m_timelineProjectionTransactionAborted = false;
        m_timelineProjectionRequests.clear();
        m_timelineProjectionRollbacks.clear();
        m_timelineProjectionCommitActions.clear();
        return false;
    }
    --m_timelineProjectionTransactionDepth;
    if (m_timelineProjectionTransactionDepth > 0) {
        return !m_timelineProjectionTransactionAborted;
    }

    const bool aborted = m_timelineProjectionTransactionAborted;
    auto requests = std::move(m_timelineProjectionRequests);
    auto rollbacks = std::move(m_timelineProjectionRollbacks);
    auto commitActions = std::move(m_timelineProjectionCommitActions);
    m_timelineProjectionTransactionAborted = false;
    m_timelineProjectionRequests.clear();
    m_timelineProjectionRollbacks.clear();
    m_timelineProjectionCommitActions.clear();
    if (aborted) {
        for (auto it = rollbacks.rbegin(); it != rollbacks.rend(); ++it) {
            if (*it) {
                (*it)();
            }
        }
        return false;
    }
    bool committed = false;
    QList<QVariantMap> inversePatches;
    const bool batchClipGeometry = requests.size() > 1 && std::ranges::all_of(
            requests, [](const QVariantMap &request) {
                return request.value(QStringLiteral("operation")).toString() ==
                       QStringLiteral("update_clip_geometry");
            });
    if (batchClipGeometry) {
            QVariantList updates;
            updates.reserve(requests.size());
            for (QVariantMap request : std::as_const(requests)) {
                request.remove(QStringLiteral("operation"));
                updates.append(std::move(request));
            }
            const QVariantMap batchRequest{
                {QStringLiteral("operation"), QStringLiteral("batch_update_clip_geometry")},
                {QStringLiteral("updates"), updates},
            };
            QVariantMap inversePatch;
            committed = applyTimelineEditRequest(batchRequest, inversePatch);
            if (committed) {
                inversePatches.append(std::move(inversePatch));
            }
    } else {
        committed = true;
        inversePatches.reserve(requests.size());
        for (const QVariantMap &request : std::as_const(requests)) {
            QVariantMap inversePatch;
            if (!applyTimelineEditRequest(request, inversePatch)) {
                committed = false;
                break;
            }
            inversePatches.append(std::move(inversePatch));
        }
    }
    if (committed && !synchronizeTimelineProjection()) {
        committed = false;
    }
    if (committed) {
        for (auto &action : commitActions) {
            if (action) {
                action();
            }
        }
        return true;
    }
    for (auto it = inversePatches.crbegin(); it != inversePatches.crend(); ++it) {
        if (!applyTimelinePatch(*it)) {
            qWarning() << "Failed to roll back a Rust targeted timeline edit";
        }
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
