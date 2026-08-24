#include "commands.hpp"
#include "constants.hpp"
#include "effect_registry.hpp"
#include "rust_keyframe_document.hpp"
#include "rust_timeline_edit.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include "timeline_service.hpp"
#include <QDebug>
#include <QPoint>
#include <algorithm>
#include <cstdint>
#include <utility>
#include <vector>

namespace AviQtl::UI {

namespace {

std::vector<AviQtl::RustCore::TimelineClipGeometry> timelineGeometry(const QList<ClipData> &clips) {
    std::vector<AviQtl::RustCore::TimelineClipGeometry> result;
    result.reserve(static_cast<std::size_t>(clips.size()));
    for (const ClipData &clip : clips) {
        result.push_back({
            .clip_id = clip.id,
            .layer = clip.layer,
            .start_frame = clip.startFrame,
            .duration_frames = clip.durationFrames,
        });
    }
    return result;
}

std::vector<std::int32_t> timelineLayers(const QSet<int> &layers) {
    std::vector<std::int32_t> result;
    result.reserve(static_cast<std::size_t>(layers.size()));
    for (int layer : layers) {
        result.push_back(layer);
    }
    return result;
}

QString clipDisplayName(const ClipData &clip) { return clip.effects.isEmpty() ? clip.type : clip.effects.first()->name(); }

void applyPlannedMoves(TimelineService *service, std::span<const AviQtl::RustCore::TimelineClipGeometry> planned, const QString &commandText, bool prevalidated) {
    if (planned.empty()) {
        return;
    }
    QList<ClipMoveChange> moves;
    moves.reserve(static_cast<qsizetype>(planned.size()));
    for (const auto &next : planned) {
        const auto *old = service->findClipById(next.clip_id);
        if (old == nullptr) {
            continue;
        }
        moves.append({
            .clipId = old->id,
            .oldLayer = old->layer,
            .oldStart = old->startFrame,
            .oldDuration = old->durationFrames,
            .newLayer = next.layer,
            .newStart = next.start_frame,
            .newDuration = next.duration_frames,
        });
    }
    if (!moves.isEmpty()) {
        service->undoStack()->push(
            new MoveClipsCommand(service, std::move(moves), commandText, prevalidated));
    }
}

} // namespace

int TimelineService::createClip(const QString &type, int startFrame, int layer) {
    const int id = allocateClipId();
    if (id < 0) {
        return startFrame;
    }
    QString clipName = type;
    auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(type);
    if (!meta.name.isEmpty()) {
        clipName = meta.name;
    }
    const int duration = AviQtl::Core::SettingsManager::instance().value(QStringLiteral("defaultClipDuration"), AviQtl::kDefaultClipDuration).toInt();
    int safeFrame = findVacantFrame(layer, startFrame, duration, -1);
    m_undoStack->push(new AddClipCommand(this, id, type, safeFrame, layer, clipName));
    return safeFrame;
}

void TimelineService::createClipInternal(int clipId, const QString &type, int startFrame, int layer,
                                         bool emitSignal, int duration,
                                         const QString &effectId,
                                         const QVariantMap &effectParams) {
    startFrame = std::max(startFrame, 0);
    layer = std::max(layer, 0);

    if (isLayerLocked(layer)) {
        qWarning() << "createClipInternal: layer" << layer << " is locked.";
        return;
    }

    if (duration <= 0) {
        duration = AviQtl::Core::SettingsManager::instance()
                       .settings()
                       .value(QStringLiteral("defaultClipDuration"),
                              AviQtl::kDefaultClipDuration)
                       .toInt();
    }
    int safeStartFrame = findVacantFrame(layer, startFrame, duration, -1);

    ClipData newClip;
    newClip.id = clipId;
    newClip.sceneId = m_currentSceneId;
    newClip.type = type;
    newClip.startFrame = safeStartFrame;
    newClip.durationFrames = duration;
    newClip.layer = layer;

    const auto appendEffect = [this, &newClip](const QString &id) {
        const auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(id);
        auto *model = new EffectModel(meta.id, meta.name, meta.kind, meta.categories,
                                      meta.defaultParams, meta.qmlSource, meta.uiDefinition, this);
        model->syncTrackEndpoints(newClip.durationFrames);
        newClip.effects.append(model);
    };
    if (type != QLatin1String("audio")) {
        appendEffect(QStringLiteral("transform"));
    }
    appendEffect(type);

    if (!effectId.isEmpty()) {
        for (auto *effect : std::as_const(newClip.effects)) {
            if (effect == nullptr || effect->id() != effectId) {
                continue;
            }
            for (auto it = effectParams.cbegin(); it != effectParams.cend(); ++it) {
                effect->setParam(it.key(), it.value());
            }
            break;
        }
    }

    if (type == QLatin1String("scene")) {
        int defaultTargetSceneId = -1;
        for (const auto &scene : std::as_const(m_scenes)) {
            if (scene.id != m_currentSceneId) {
                defaultTargetSceneId = scene.id;
                break;
            }
        }
        for (auto *effect : std::as_const(newClip.effects)) {
            if (effect != nullptr && effect->id() == QLatin1String("scene")) {
                effect->setParam(QStringLiteral("targetSceneId"), defaultTargetSceneId);
                break;
            }
        }
    }

    auto &targetClips = clipsMutable();
    const int targetSceneId = m_currentSceneId;
    const qsizetype insertedIndex = targetClips.size();
    targetClips.append(newClip);
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("insert_clip")},
        {QStringLiteral("clip"), timelineClipDocument(newClip)},
    };
    if (!commitTimelineMutation(request, [this, targetSceneId, insertedIndex, clipId,
                                           effects = newClip.effects]() {
        auto sceneIt = std::ranges::find_if(m_scenes, [targetSceneId](const SceneData &scene) {
            return scene.id == targetSceneId;
        });
        if (sceneIt != m_scenes.end()) {
            if (insertedIndex < sceneIt->clips.size() &&
                sceneIt->clips.at(insertedIndex).id == clipId) {
                sceneIt->clips.removeAt(insertedIndex);
            } else {
                for (qsizetype index = sceneIt->clips.size(); index > 0; --index) {
                    if (sceneIt->clips.at(index - 1).id == clipId) {
                        sceneIt->clips.removeAt(index - 1);
                        break;
                    }
                }
            }
        }
        for (auto *effect : effects) {
            if (effect != nullptr) {
                effect->deleteLater();
            }
        }
    })) {
        qWarning() << "Rust rejected clip creation";
        return;
    }

    // Keep the established Qt notification contract from addEffectInternal() while committing
    // the fully initialized clip to Rust only once.
    for (qsizetype index = 0; index < newClip.effects.size(); ++index) {
        emit clipsChanged();
        emit clipEffectsChanged(newClip.id);
    }
    if (emitSignal) {
        emit clipsChanged();
        emit clipCreated(newClip.id, newClip.layer, newClip.startFrame, newClip.durationFrames, newClip.type);
    }
}

bool TimelineService::addClipsDirectInternal(const QList<ClipData> &clips) {
    beginTimelineProjectionTransaction();
    for (const auto &clip : std::as_const(clips)) {
        if (!addClipDirectInternal(clip, false)) {
            abortTimelineProjectionTransaction();
            static_cast<void>(endTimelineProjectionTransaction());
            qWarning() << "Rejected clip in batch restoration" << clip.id;
            return false;
        }
    }
    if (!endTimelineProjectionTransaction()) {
        qWarning() << "Rust rejected batch clip restoration";
        return false;
    }
    emit clipsChanged();
    return true;
}

void TimelineService::updateClip(int id, int layer, int startFrame, int duration) {
    const auto *clip = findClipById(id);
    if (clip == nullptr) {
        return;
    }

    QString clipName = clip->type;
    if (!clip->effects.isEmpty()) {
        clipName = clip->effects.first()->name();
    }

    m_undoStack->push(new MoveClipCommand(this, id, clip->layer, clip->startFrame, clip->durationFrames, layer, startFrame, duration, clipName));
}

void TimelineService::insertLayers(int targetLayer, int count, bool above) {
    if (count <= 0)
        return;

    const auto geometry = timelineGeometry(clips());
    std::vector<AviQtl::RustCore::TimelineClipGeometry> planned;
    if (AviQtl::RustCore::planInsertLayers(geometry, targetLayer, count, above, planned) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline layer insertion planning failed";
        return;
    }

    applyPlannedMoves(this, planned,
                      above ? tr("レイヤーを上に挿入") : tr("レイヤーを下に挿入"), true);
}

void TimelineService::shiftLayers(int startLayer, int endLayer, int delta) {
    if (delta == 0 || startLayer > endLayer)
        return;

    const auto geometry = timelineGeometry(clips());
    std::vector<AviQtl::RustCore::TimelineClipGeometry> planned;
    if (AviQtl::RustCore::planShiftLayers(geometry, startLayer, endLayer, delta, planned) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline layer shift planning failed";
        return;
    }

    applyPlannedMoves(this, planned,
                      delta > 0 ? tr("レイヤーをまとめて下へ移動")
                                : tr("レイヤーをまとめて上へ移動"),
                      true);
}

void TimelineService::applyClipBatchMove(const QVariantList &moves) {
    if (moves.isEmpty()) {
        return;
    }

    std::vector<AviQtl::RustCore::TimelineMoveInput> requested;
    requested.reserve(static_cast<std::size_t>(moves.size()));

    for (const QVariant &vMove : std::as_const(moves)) {
        const QVariantMap move = vMove.toMap();
        const int id = move.value(QStringLiteral("id")).toInt();
        const auto *clip = findClipById(id);
        if (clip != nullptr) {
            bool durationOk = false;
            const int requestedDuration =
                move.value(QStringLiteral("duration")).toInt(&durationOk);
            AviQtl::RustCore::TimelineMoveInput movement{
                .clip_id = id,
                .old_layer = clip->layer,
                .old_start_frame = clip->startFrame,
                .duration_frames = durationOk && requestedDuration > 0
                                       ? requestedDuration
                                       : clip->durationFrames,
                .target_layer = move.value(QStringLiteral("layer")).toInt(),
                .target_start_frame = move.value(QStringLiteral("startFrame")).toInt(),
            };
            requested.push_back(movement);
        }
    }

    if (requested.empty()) {
        return;
    }

    const auto geometry = timelineGeometry(clips());
    const auto lockedLayers = timelineLayers(currentScene()->lockedLayers);
    std::vector<AviQtl::RustCore::TimelineClipGeometry> planned;
    const auto status = AviQtl::RustCore::planBatchMove(geometry, requested, lockedLayers, planned);
    if (status == AviQtl::RustCore::TimelineEditStatus::LockedLayer) {
        return;
    }
    if (status != AviQtl::RustCore::TimelineEditStatus::Ok || planned.size() != requested.size()) {
        qWarning() << "Rust timeline batch move planning failed";
        return;
    }

    applyPlannedMoves(this, planned, QObject::tr("複数クリップ絶対移動: %1").arg(static_cast<qsizetype>(planned.size())), true);
}

void TimelineService::moveSelectedClips(int deltaLayer, int deltaFrame) {
    if ((m_selection == nullptr) || (deltaLayer == 0 && deltaFrame == 0)) {
        return;
    }

    const QList<int> &selectedIds = m_selection->selectedClipIdsNative();
    if (selectedIds.isEmpty()) {
        return;
    }

    std::vector<std::int32_t> movingIds;
    movingIds.reserve(static_cast<std::size_t>(selectedIds.size()));
    for (int id : selectedIds) {
        movingIds.push_back(id);
    }

    const auto geometry = timelineGeometry(clips());
    const auto lockedLayers = timelineLayers(currentScene()->lockedLayers);
    std::vector<AviQtl::RustCore::TimelineClipGeometry> planned;
    const auto status = AviQtl::RustCore::planDeltaMove(geometry, movingIds, lockedLayers, deltaLayer, deltaFrame, planned);
    if (status == AviQtl::RustCore::TimelineEditStatus::LockedLayer) {
        return;
    }
    if (status != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline delta move planning failed";
        return;
    }
    applyPlannedMoves(this, planned, QObject::tr("複数クリップ絶対移動: %1").arg(static_cast<qsizetype>(planned.size())), true);
}

void TimelineService::resizeSelectedClips(int deltaStartFrame, int deltaDuration) {
    if ((m_selection == nullptr) || (deltaStartFrame == 0 && deltaDuration == 0)) {
        return;
    }

    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    std::vector<AviQtl::RustCore::TimelineClipGeometry> selected;
    selected.reserve(static_cast<std::size_t>(ids.size()));

    for (const QVariant &value : std::as_const(ids)) {
        const int id = value.toInt();
        const auto *clip = findClipById(id);
        if (clip == nullptr) {
            continue;
        }
        selected.push_back({
            .clip_id = clip->id,
            .layer = clip->layer,
            .start_frame = clip->startFrame,
            .duration_frames = clip->durationFrames,
        });
    }

    std::vector<AviQtl::RustCore::TimelineClipGeometry> planned;
    if (AviQtl::RustCore::planResize(selected, deltaStartFrame, deltaDuration, planned) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline resize planning failed";
        return;
    }

    applyPlannedMoves(this, planned, QObject::tr("複数クリップ変形: %1").arg(static_cast<qsizetype>(planned.size())), false);
}

auto TimelineService::resolveDragPosition(int clipId, int targetLayer, int proposedStartFrame, const QVariantList &batchIds) -> QPoint { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *movingClip = findClipById(clipId);
    if (movingClip == nullptr) {
        return {proposedStartFrame, targetLayer};
    }

    std::vector<std::int32_t> movingIds;
    if (!batchIds.isEmpty()) {
        for (const QVariant &v : std::as_const(batchIds)) {
            movingIds.push_back(v.toInt());
        }
    } else if ((m_selection != nullptr) && m_selection->isSelected(clipId)) {
        for (const QVariant &v : m_selection->selectedClipIds()) {
            movingIds.push_back(v.toInt());
        }
    } else {
        movingIds.push_back(clipId);
    }

    const auto geometry = timelineGeometry(clips());
    const auto lockedLayers = timelineLayers(currentScene()->lockedLayers);
    AviQtl::RustCore::TimelinePosition position{};
    const auto status = AviQtl::RustCore::resolveDrag(geometry, movingIds, lockedLayers, clipId, targetLayer, proposedStartFrame, position);
    if (status == AviQtl::RustCore::TimelineEditStatus::LockedLayer) {
        return {movingClip->startFrame, movingClip->layer};
    }
    if (status != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline drag resolution failed";
        return {proposedStartFrame, targetLayer};
    }
    return {position.frame, position.layer};
}

void TimelineService::updateClipInternal(int id, int layer, int startFrame, int duration,
                                         bool emitSignal, bool forcePosition, bool commitState) {
    const auto *existingClip = findClipById(id);
    if (existingClip == nullptr) {
        return;
    }

    // 移動元または移動先がロックされている場合は拒否
    if (isLayerLocked(layer) || isLayerLocked(existingClip->layer)) {
        qWarning() << "updateClipInternal: Rejected operation on/to a locked layer.";
        return;
    }

    startFrame = std::max(startFrame, 0);
    duration = std::max(duration, 1);
    layer = std::max(layer, 0);

    // [FINAL LOGIC] The ultimate gatekeeper for collision.
    // All position updates, whether from drag, undo, or other operations, must pass this check.
    // Skip collision detection when forcePosition is true (e.g., undo operations).
    if (!forcePosition) {
        int safeStartFrame = findVacantFrame(layer, startFrame, duration, id);
        if (safeStartFrame != startFrame) {
            qWarning() << "updateClipInternal: Collision detected. Position adjusted from" << startFrame << "to" << safeStartFrame;
            startFrame = safeStartFrame;
        }
    }

    for (auto &clip : clipsMutable()) {
        if (clip.id == id) {
            if (clip.layer != layer || clip.startFrame != startFrame || clip.durationFrames != duration) {
                const int oldLayer = clip.layer;
                const int oldStartFrame = clip.startFrame;
                const int oldDuration = clip.durationFrames;
                QList<QVariantMap> oldEffectTracks;
                oldEffectTracks.reserve(clip.effects.size());
                for (const auto *effect : std::as_const(clip.effects)) {
                    oldEffectTracks.append(effect != nullptr ? effect->keyframeTracks() : QVariantMap{});
                }
                const QList<AudioPluginState> oldAudioPlugins = clip.audioPlugins;
                clip.layer = layer;
                clip.startFrame = startFrame;
                clip.durationFrames = duration;
                for (auto *effect : std::as_const(clip.effects)) {
                    if (effect != nullptr) {
                        effect->syncTrackEndpoints(duration);
                    }
                }
                for (auto &plugin : clip.audioPlugins) {
                    for (auto it = plugin.params.cbegin(); it != plugin.params.cend(); ++it) {
                        const auto result = AviQtl::Core::RustKeyframeDocument::sync(
                            plugin.keyframeTracks.value(it.key()), it.value(), oldDuration,
                            duration);
                        if (result) {
                            plugin.keyframeTracks[it.key()] = result->track;
                        }
                    }
                    plugin.invalidateKeyframeCache();
                }
                const QVariantMap request{
                    {QStringLiteral("operation"), QStringLiteral("update_clip_geometry")},
                    {QStringLiteral("clip_id"), id},
                    {QStringLiteral("layer"), layer},
                    {QStringLiteral("start"), startFrame},
                    {QStringLiteral("duration"), duration},
                };
                if (commitState && !commitTimelineMutation(
                        request, [this, id, oldLayer, oldStartFrame, oldDuration, oldEffectTracks,
                                  oldAudioPlugins]() {
                            auto *restored = findClipById(id);
                            if (restored == nullptr) {
                                return;
                            }
                            restored->layer = oldLayer;
                            restored->startFrame = oldStartFrame;
                            restored->durationFrames = oldDuration;
                            for (qsizetype index = 0;
                                 index < restored->effects.size() && index < oldEffectTracks.size();
                                 ++index) {
                                if (auto *effect = restored->effects.at(index); effect != nullptr) {
                                    effect->syncTrackEndpoints(oldDuration);
                                    effect->setKeyframeTracks(oldEffectTracks.at(index));
                                }
                            }
                            restored->audioPlugins = oldAudioPlugins;
                            for (auto &plugin : restored->audioPlugins) {
                                plugin.invalidateKeyframeCache();
                            }
                        })) {
                    qWarning() << "Rust rejected clip geometry update";
                    return;
                }
                if (emitSignal) {
                    emit clipsChanged();
                }
                // 選択中のクリップであればSelectionServiceのキャッシュも更新する
                if (m_selection->selectedClipId() == id) {
                    QVariantMap data = m_selection->selectedClipData();
                    data.insert(QStringLiteral("layer"), layer);
                    data.insert(QStringLiteral("startFrame"), startFrame);
                    data.insert(QStringLiteral("durationFrames"), duration);
                    m_selection->refreshSelectionData(id, data);
                }
            }
            break;
        }
    }
}

auto TimelineService::clipByUpperObject(int clipId) const -> bool {
    const auto *clip = findClipById(clipId);
    return clip != nullptr ? clip->clipByUpperObject : false;
}

void TimelineService::setClipByUpperObject(int clipId, bool enabled) {
    const auto *clip = findClipById(clipId);
    if (clip == nullptr || clip->clipByUpperObject == enabled) {
        return;
    }
    m_undoStack->push(new SetClipByUpperObjectCommand(this, clipId, enabled));
}

void TimelineService::setClipByUpperObjectInternal(int clipId, bool enabled, bool emitSignal,
                                                   bool commitState) {
    auto *clip = findClipById(clipId);
    if (clip == nullptr || clip->clipByUpperObject == enabled) {
        return;
    }
    clip->clipByUpperObject = enabled;
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("set_clip_by_upper_object")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("enabled"), enabled},
    };
    if (commitState && !commitTimelineMutation(request, [this, clipId, previous = !enabled]() {
            if (auto *restored = findClipById(clipId); restored != nullptr) {
                restored->clipByUpperObject = previous;
            }
        })) {
        qWarning() << "Rust rejected clip compositing update";
        return;
    }
    if (emitSignal) {
        emit clipsChanged();
    }
    if (m_selection != nullptr && m_selection->selectedClipId() == clipId) {
        QVariantMap data = m_selection->selectedClipData();
        data.insert(QStringLiteral("clipByUpperObject"), enabled);
        m_selection->refreshSelectionData(clipId, data);
    }
}

void TimelineService::selectClip(int id) { applySelectionIds(QVariantList{id}); }

void TimelineService::toggleSelection(int id, const QVariantMap &data) {
    if (m_selection == nullptr) {
        return;
    }

    QVariantList ids = m_selection->selectedClipIds();
    int idx = -1;
    for (int i = 0; i < ids.size(); ++i) {
        if (ids.value(i).toInt() == id) {
            idx = i;
            break;
        }
    }

    if (idx >= 0) {
        ids.removeAt(idx);
    } else {
        ids.prepend(id);
    }

    applySelectionIds(ids);
}

void TimelineService::applySelectionIds(const QVariantList &ids) {
    int primaryId = -1;
    QVariantMap primaryData;

    // 選択されたクリップのリストを更新
    QVariantList newSelectedIds;
    for (const QVariant &v : std::as_const(ids)) {
        if (!newSelectedIds.contains(v)) {
            newSelectedIds.append(v);
        }
    }

    if (!newSelectedIds.isEmpty()) {
        int id = newSelectedIds.first().toInt(); // 最初のクリップをプライマリとする
        const auto *clip = findClipById(id);
        if (clip != nullptr) { // findClipById は nullptr を返す可能性があるのでチェック
            primaryId = clip->id;
            for (auto *eff : clip->effects) {
                QVariantMap params = eff->params();
                for (auto it = params.begin(); it != params.end(); ++it) {
                    primaryData.insert(it.key(), it.value());
                }
            }
            primaryData.insert(QStringLiteral("startFrame"), clip->startFrame);
            primaryData.insert(QStringLiteral("durationFrames"), clip->durationFrames);
            primaryData.insert(QStringLiteral("layer"), clip->layer);
            primaryData.insert(QStringLiteral("clipByUpperObject"), clip->clipByUpperObject);
            primaryData.insert(QStringLiteral("type"), clip->type);
        }
    }

    // SelectionService の replaceSelection を呼び出す
    m_selection->replaceSelection(newSelectedIds, primaryId, primaryData);
}

void TimelineService::deleteSelectedClips() {
    if (m_selection == nullptr) {
        return;
    }
    deleteClipsByIds(m_selection->selectedClipIds());
}

void TimelineService::deleteClipsByIds(const QVariantList &ids) {
    if (ids.isEmpty()) {
        return;
    }

    QList<int> intIds;
    for (const QVariant &v : std::as_const(ids)) {
        int id = v.toInt();
        if (id >= 0) {
            intIds.append(id);
        }
    }

    if (intIds.isEmpty()) {
        return;
    }

    QString macroText = intIds.size() == 1 ? QObject::tr("クリップ削除") : QObject::tr("複数クリップ削除: %1").arg(intIds.size());

    m_undoStack->push(new DeleteClipsCommand(this, intIds, macroText));

    m_selection->clearSelection();
}

void TimelineService::deleteClip(int clipId) { deleteClipsByIds({clipId}); }

void TimelineService::deleteClipInternal(int clipId, bool emitSignal, bool commitState) {
    auto &currentClips = clipsMutable();
    auto it = std::ranges::find_if(currentClips, [clipId](const ClipData &c) -> bool { return c.id == clipId; });
    if (it != currentClips.end()) {
        const auto index = std::distance(currentClips.begin(), it);
        const ClipData removed = *it;
        const int sceneId = removed.sceneId;
        currentClips.erase(it);
        const QVariantMap request{
            {QStringLiteral("operation"), QStringLiteral("remove_clip")},
            {QStringLiteral("clip_id"), clipId},
        };
        if (commitState && !commitTimelineMutation(
                request, [this, sceneId, index, removed]() {
                    auto sceneIt = std::ranges::find_if(m_scenes, [sceneId](const SceneData &scene) {
                        return scene.id == sceneId;
                    });
                    if (sceneIt != m_scenes.end()) {
                        sceneIt->clips.insert(std::min<qsizetype>(index, sceneIt->clips.size()), removed);
                    }
                },
                [removed]() {
                    for (auto *effect : removed.effects) {
                        if (effect != nullptr) {
                            effect->deleteLater();
                        }
                    }
                })) {
            qWarning() << "Rust rejected clip deletion";
            return;
        }
        if (emitSignal) {
            emit clipsChanged();
        }
    }
}

bool TimelineService::addClipDirectInternal(const ClipData &clip, bool emitSignal,
                                            bool commitState) {
    auto sceneIt = std::ranges::find_if(m_scenes, [&clip](const SceneData &scene) {
        return scene.id == clip.sceneId;
    });
    if (sceneIt == m_scenes.end()) {
        qWarning() << "Cannot insert clip into missing scene" << clip.sceneId;
        return false;
    }
    auto &targetClips = sceneIt->clips;
    const int targetSceneId = clip.sceneId;
    const qsizetype insertedIndex = targetClips.size();
    targetClips.append(clip);
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("insert_clip")},
        {QStringLiteral("clip"), timelineClipDocument(clip)},
    };
    if (commitState && !commitTimelineMutation(request, [this, targetSceneId, insertedIndex,
                                                          clipId = clip.id]() {
            auto sceneIt = std::ranges::find_if(m_scenes, [targetSceneId](const SceneData &scene) {
                return scene.id == targetSceneId;
            });
            if (sceneIt == m_scenes.end()) {
                return;
            }
            if (insertedIndex < sceneIt->clips.size() &&
                sceneIt->clips.at(insertedIndex).id == clipId) {
                sceneIt->clips.removeAt(insertedIndex);
                return;
            }
            for (qsizetype index = sceneIt->clips.size(); index > 0; --index) {
                if (sceneIt->clips.at(index - 1).id == clipId) {
                    sceneIt->clips.removeAt(index - 1);
                    return;
                }
            }
        })) {
        qWarning() << "Rust rejected direct clip insertion";
        return false;
    }
    if (emitSignal) {
        emit clipsChanged();
        emit clipCreated(clip.id, clip.layer, clip.startFrame, clip.durationFrames, clip.type);
    }
    return true;
}

auto TimelineService::findClipById(int clipId) -> ClipData * {
    // Fast path: search current scene first (most operations work within current scene)
    auto *scene = currentScene();
    if (scene != nullptr) {
        auto it = std::ranges::find_if(scene->clips, [clipId](const ClipData &c) -> bool { return c.id == clipId; });
        if (it != scene->clips.end())
            return &(*it);
    }
    // Fallback: search all scenes
    for (auto &s : m_scenes) {
        if (s.id == m_currentSceneId)
            continue; // Already searched
        auto it = std::ranges::find_if(s.clips, [clipId](const ClipData &c) -> bool { return c.id == clipId; });
        if (it != s.clips.end())
            return &(*it);
    }
    return nullptr;
}

auto TimelineService::findClipById(int clipId) const -> const ClipData * {
    // Fast path: search current scene first (most operations work within current scene)
    const auto *scene = currentScene();
    if (scene != nullptr) {
        auto it = std::ranges::find_if(scene->clips, [clipId](const ClipData &c) -> bool { return c.id == clipId; });
        if (it != scene->clips.end())
            return &(*it);
    }
    // Fallback: search all scenes
    for (const auto &s : std::as_const(m_scenes)) {
        if (s.id == m_currentSceneId)
            continue; // Already searched
        auto it = std::ranges::find_if(s.clips, [clipId](const ClipData &c) -> bool { return c.id == clipId; });
        if (it != s.clips.end())
            return &(*it);
    }
    return nullptr;
}

auto TimelineService::deepCopyClip(const ClipData &source) -> ClipData {
    ClipData newClip;
    newClip.id = -1;
    newClip.type = source.type;
    newClip.startFrame = source.startFrame;
    newClip.durationFrames = source.durationFrames;
    newClip.layer = source.layer;
    newClip.clipByUpperObject = source.clipByUpperObject;
    newClip.sceneId = source.sceneId;
    newClip.params = source.params;
    newClip.audioPlugins = source.audioPlugins;

    for (const auto *oldEffect : std::as_const(source.effects)) {
        auto *newEffect = new EffectModel(oldEffect->id(), oldEffect->name(), oldEffect->kind(), oldEffect->categories(), oldEffect->params(), oldEffect->qmlSource(), oldEffect->uiDefinition(), this);
        newEffect->setEnabled(oldEffect->isEnabled());
        newEffect->setKeyframeTracks(oldEffect->keyframeTracks());
        newEffect->syncTrackEndpoints(source.durationFrames);
        newClip.effects.append(newEffect);
    }
    return newClip;
}

void TimelineService::copyClip(int clipId) {
    auto &currentClips = clipsMutable();
    auto it = std::ranges::find_if(currentClips, [clipId](const ClipData &c) -> bool { return c.id == clipId; });
    if (it == currentClips.end()) {
        return;
    }

    setClipboard(*it);
}

void TimelineService::copySelectedClips() {
    QList<ClipData> copied;
    const QVariantList ids = m_selection->selectedClipIds();
    for (const QVariant &value : std::as_const(ids)) {
        const int id = value.toInt();
        const auto *clip = findClipById(id);
        if (clip != nullptr) {
            copied.append(deepCopyClip(*clip));
        }
    }
    if (!copied.isEmpty()) {
        setClipboard(copied);
    }
}

void TimelineService::cutClip(int clipId) {
    const auto *clip = findClipById(clipId); // findClipById は const なので、ここでコピー
    if (clip == nullptr) {
        return;
    }
    QString name = clip->effects.isEmpty() ? clip->type : clip->effects.first()->name();
    m_undoStack->push(new CutClipCommand(this, clipId, name));
}

void TimelineService::cutSelectedClips() {
    if ((m_selection == nullptr) || m_selection->selectedClipIds().isEmpty()) {
        return;
    }

    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    QList<ClipData> copied;
    for (const QVariant &v : std::as_const(ids)) {
        const auto *clip = findClipById(v.toInt());
        if (clip != nullptr) {
            copied.append(deepCopyClip(*clip));
        }
    }
    setClipboard(copied); // クリップボードにコピー

    QList<int> intIds;
    for (const QVariant &v : std::as_const(ids)) {
        intIds.append(v.toInt());
    }
    m_undoStack->push(new DeleteClipsCommand(this, intIds, QString(QStringLiteral("複数クリップ切り取り: %1")).arg(ids.size())));
    m_selection->clearSelection();
}

void TimelineService::splitSelectedClips(int frame) {
    if (m_selection == nullptr) {
        return;
    }
    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    m_undoStack->beginMacro(QObject::tr("複数クリップ分割: %1").arg(ids.size()));
    for (const QVariant &v : std::as_const(ids)) {
        splitClip(v.toInt(), frame);
    }
    m_undoStack->endMacro();
}

int TimelineService::pasteClip(int frame, int layer) {
    if (m_clipboard.isEmpty()) {
        return frame;
    }

    frame = std::max(frame, 0);
    layer = std::max(layer, 0);
    const auto existing = timelineGeometry(clips());
    const auto clipboard = timelineGeometry(m_clipboard);
    std::vector<AviQtl::RustCore::TimelineClipGeometry> placement;
    std::int32_t safeFrame = frame;
    if (AviQtl::RustCore::planClipboardPlacement(existing, clipboard, frame, layer, placement, safeFrame) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline clipboard placement failed";
        return frame;
    }

    QList<ClipData> pending;
    pending.reserve(m_clipboard.size());
    for (qsizetype index = 0; index < m_clipboard.size(); ++index) {
        const auto &src = m_clipboard[index];
        const auto &planned = placement[static_cast<std::size_t>(index)];
        ClipData newClip = deepCopyClip(src);
        newClip.sceneId = m_currentSceneId;
        newClip.startFrame = planned.start_frame;
        newClip.layer = planned.layer;
        newClip.durationFrames = planned.duration_frames;
        pending.append(newClip);
    }

    if (pending.size() == 1) {
        const int newId = allocateClipId();
        if (newId < 0) {
            return frame;
        }
        m_undoStack->push(new PasteClipCommand(this, newId, pending.first()));
        return safeFrame;
    }

    const QList<int> newIds = allocateClipIds(pending.size());
    if (newIds.size() != pending.size()) {
        return frame;
    }
    m_undoStack->beginMacro(QObject::tr("複数クリップ貼り付け: %1").arg(pending.size()));
    for (qsizetype index = 0; index < pending.size(); ++index) {
        m_undoStack->push(new PasteClipCommand(this, newIds.at(index), pending.at(index)));
    }
    m_undoStack->endMacro();
    return safeFrame;
}

void TimelineService::splitClip(int clipId, int frame) {
    const auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }

    const AviQtl::RustCore::TimelineClipGeometry geometry{
        .clip_id = clip->id,
        .layer = clip->layer,
        .start_frame = clip->startFrame,
        .duration_frames = clip->durationFrames,
    };
    AviQtl::RustCore::TimelineClipGeometry first{};
    AviQtl::RustCore::TimelineClipGeometry second{};
    if (AviQtl::RustCore::splitClip(geometry, frame, first, second) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        return;
    }
    m_undoStack->push(new SplitClipCommand(this, clipId, frame, clip->durationFrames, first.duration_frames, second.duration_frames, clipDisplayName(*clip)));
}

auto TimelineService::clips() const -> const QList<ClipData> & { return currentScene()->clips; }

auto TimelineService::clipsMutable() -> QList<ClipData> & { return currentScene()->clips; }

auto TimelineService::clips(int sceneId) const -> const QList<ClipData> & {
    for (const auto &scene : std::as_const(m_scenes)) {
        if (scene.id == sceneId) {
            return scene.clips;
        }
    }
    static QList<ClipData> empty;
    return empty;
}

auto TimelineService::findVacantFrame(int layer, int startFrame, int duration, int excludeClipId) const -> int { // NOLINT(bugprone-easily-swappable-parameters)
    std::vector<std::int32_t> excludedIds;
    excludedIds.push_back(excludeClipId);
    if ((m_selection != nullptr) && m_selection->isSelected(excludeClipId)) {
        const QList<int> &selection = m_selection->selectedClipIdsNative();
        excludedIds.reserve(static_cast<std::size_t>(selection.size()) + 1);
        for (int id : selection) {
            excludedIds.push_back(id);
        }
    }

    const auto geometry = timelineGeometry(clips());
    std::int32_t result = std::max(0, startFrame);
    if (AviQtl::RustCore::findVacantFrame(geometry, excludedIds, layer, startFrame, duration, result) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline vacancy search failed";
    }
    return result;
}

void TimelineService::setClipboard(const ClipData &clip) { setClipboard(QList<ClipData>{clip}); }

void TimelineService::setClipboard(const QList<ClipData> &clips) {
    for (auto &c : m_clipboard) {
        for (auto *eff : std::as_const(c.effects)) {
            if (eff)
                eff->deleteLater();
        }
        c.effects.clear();
    }
    m_clipboard.clear();

    for (const auto &clip : std::as_const(clips)) {
        m_clipboard.append(deepCopyClip(clip));
    }
}

int TimelineService::getClipboardDuration() const {
    const auto geometry = timelineGeometry(m_clipboard);
    std::int32_t duration = 0;
    if (AviQtl::RustCore::clipboardDuration(geometry, duration) != AviQtl::RustCore::TimelineEditStatus::Ok) {
        qWarning() << "Rust timeline clipboard duration failed";
        return 0;
    }
    return duration;
}

} // namespace AviQtl::UI
