#include "commands.hpp"
#include "effect_registry.hpp"
#include "rust_keyframe_document.hpp"
#include "timeline_service.hpp"
#include <QDebug>
#include <QObject>

namespace AviQtl::UI {

namespace {

bool captureClipProjection(TimelineService *service, int clipId,
                           ClipProjectionRestore &restore, QObject *snapshotOwner) {
    for (const auto &scene : service->getAllScenes()) {
        for (qsizetype index = 0; index < scene.clips.size(); ++index) {
            if (scene.clips.at(index).id != clipId) {
                continue;
            }
            restore.clip = service->deepCopyClip(scene.clips.at(index));
            restore.clip.id = clipId;
            restore.index = index;
            for (auto *effect : std::as_const(restore.clip.effects)) {
                if (effect != nullptr) {
                    effect->setParent(snapshotOwner);
                }
            }
            return true;
        }
    }
    return false;
}

bool captureClipProjections(TimelineService *service, const QList<int> &clipIds,
                            QList<ClipProjectionRestore> &restores,
                            QObject *snapshotOwner) {
    restores.clear();
    restores.reserve(clipIds.size());
    for (int clipId : clipIds) {
        ClipProjectionRestore restore;
        if (!captureClipProjection(service, clipId, restore, snapshotOwner)) {
            restores.clear();
            return false;
        }
        restores.append(std::move(restore));
    }
    return true;
}

QList<int> projectionClipIds(const QList<ClipProjectionRestore> &restores) {
    QList<int> ids;
    ids.reserve(restores.size());
    for (const auto &restore : restores) {
        ids.append(restore.clip.id);
    }
    return ids;
}

bool replayClipProjections(TimelineService *service,
                           const TimelineEditTransaction &transaction, bool forward,
                           const QList<ClipProjectionRestore> &before,
                           const QList<ClipProjectionRestore> &after) {
    const auto &source = forward ? before : after;
    const auto &target = forward ? after : before;
    return service->applyTimelineEditTransaction(
        transaction, forward,
        [service, removeIds = projectionClipIds(source), &target]() {
            return service->replaceClipProjectionsInternal(removeIds, target);
        },
        [service, removeIds = projectionClipIds(target), &source]() {
            return service->replaceClipProjectionsInternal(removeIds, source);
        });
}

bool captureSceneProjection(TimelineService *service, int sceneId,
                            SceneProjectionRestore &restore, QObject *snapshotOwner) {
    const auto &scenes = service->getAllScenes();
    for (qsizetype index = 0; index < scenes.size(); ++index) {
        if (scenes.at(index).id != sceneId) {
            continue;
        }
        restore.scene = service->deepCopyScene(scenes.at(index));
        restore.scene.id = sceneId;
        restore.index = index;
        for (const auto &clip : std::as_const(restore.scene.clips)) {
            for (auto *effect : clip.effects) {
                if (effect != nullptr) {
                    effect->setParent(snapshotOwner);
                }
            }
        }
        return true;
    }
    return false;
}

QList<int> projectionSceneIds(const QList<SceneProjectionRestore> &restores) {
    QList<int> ids;
    ids.reserve(restores.size());
    for (const auto &restore : restores) {
        ids.append(restore.scene.id);
    }
    return ids;
}

bool replaySceneProjections(TimelineService *service,
                            const TimelineEditTransaction &transaction, bool forward,
                            const QList<SceneProjectionRestore> &before,
                            const QList<SceneProjectionRestore> &after) {
    const auto &source = forward ? before : after;
    const auto &target = forward ? after : before;
    return service->applyTimelineEditTransaction(
        transaction, forward,
        [service, removeIds = projectionSceneIds(source), &target]() {
            return service->replaceSceneProjectionsInternal(removeIds, target);
        },
        [service, removeIds = projectionSceneIds(target), &source]() {
            return service->replaceSceneProjectionsInternal(removeIds, source);
        });
}

void publishClipCreated(TimelineService *service, int clipId) {
    const auto *clip = service->findClipById(clipId);
    if (clip != nullptr) {
        emit service->clipCreated(clip->id, clip->layer, clip->startFrame,
                                  clip->durationFrames, clip->type);
    }
}

void publishEffectKeyframeChange(TimelineService *service, int clipId, int effectIndex,
                                 const QString &paramName, const QVariant &value) {
    emit service->effectParamChanged(clipId, effectIndex, paramName, value);
    if (paramName == QLatin1String("path") || paramName == QLatin1String("source") ||
        paramName == QStringLiteral("targetSceneId")) {
        emit service->clipsChanged();
    }
}

void publishAudioPluginChange(TimelineService *service, int clipId) {
    emit service->clipEffectsChanged(clipId);
    emit service->clipsChanged();
}

} // namespace

AddClipCommand::AddClipCommand(TimelineService *service, int clipId, QString type, int startFrame, int layer, const QString &clipName, int duration, QString effectId,
                               QVariantMap effectParams) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_type(std::move(type)), m_startFrame(startFrame), m_layer(layer), m_clipName(clipName), m_duration(duration), m_effectId(std::move(effectId)), m_effectParams(std::move(effectParams)), m_snapshotOwner(std::make_unique<QObject>()) {
    setText(QObject::tr("クリップ追加: %1").arg(clipName));
}

AddClipCommand::~AddClipCommand() = default;

void AddClipCommand::undo() {
    if (!m_transaction.isValid()) {
        return;
    }
    if (m_service->applyTimelineEditTransaction(
            m_transaction, false,
            [this]() { return m_service->removeClipProjectionsInternal({m_clipId}); },
            [this]() { return m_service->restoreClipProjectionsInternal({m_restore}); })) {
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust clip creation transaction";
    }
}

void AddClipCommand::redo() {
    const bool replaying = m_transaction.isValid();
    if (replaying) {
        if (!m_service->applyTimelineEditTransaction(
                m_transaction, true,
                [this]() { return m_service->restoreClipProjectionsInternal({m_restore}); },
                [this]() { return m_service->removeClipProjectionsInternal({m_clipId}); })) {
            qWarning() << "Failed to redo a Rust clip creation transaction";
            return;
        }
    } else {
        m_service->createClipInternal(m_clipId, m_type, m_startFrame, m_layer, false, m_duration,
                                      m_effectId, m_effectParams, &m_transaction);
        if (!m_transaction.isValid() ||
            !captureClipProjection(m_service, m_clipId, m_restore, m_snapshotOwner.get())) {
            qWarning() << "Failed to capture a committed Rust clip creation transaction";
            return;
        }
    }
    auto *clip = m_service->findClipById(m_clipId);
    if (clip == nullptr) {
        return;
    }
    if (replaying && !clip->effects.isEmpty()) {
        emit m_service->clipEffectsChanged(clip->id);
    }
    emit m_service->clipsChanged();
    emit m_service->clipCreated(clip->id, clip->layer, clip->startFrame, clip->durationFrames, clip->type);
}

MoveClipCommand::MoveClipCommand(TimelineService *service, int clipId, int newLayer,
                                 int newStart, int newDuration, const QString &clipName,
                                 bool prevalidated) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_newLayer(newLayer), m_newStart(newStart),
      m_newDuration(newDuration), m_clipName(clipName), m_prevalidated(prevalidated) {
    setText(QObject::tr("クリップ移動: %1").arg(clipName));
}
void MoveClipCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        m_service->publishClipGeometryChange({m_clipId});
    } else {
        qWarning() << "Failed to undo a Rust clip geometry transaction";
    }
}
void MoveClipCommand::redo() {
    if (!m_transaction.isValid()) {
        m_service->updateClipInternal(m_clipId, m_newLayer, m_newStart, m_newDuration, true,
                                      m_prevalidated, true, &m_transaction);
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        m_service->publishClipGeometryChange({m_clipId});
    } else {
        qWarning() << "Failed to redo a Rust clip geometry transaction";
    }
}

MoveClipsCommand::MoveClipsCommand(TimelineService *service, QList<ClipMoveChange> moves,
                                   QString commandText, bool prevalidated)
    : m_service(service), m_moves(std::move(moves)), m_prevalidated(prevalidated) {
    setText(std::move(commandText));
}

void MoveClipsCommand::undo() { apply(false); }
void MoveClipsCommand::redo() { apply(true); }

void MoveClipsCommand::apply(bool forward) {
    if (m_transaction.isValid()) {
        if (!m_service->applyTimelineEditTransaction(m_transaction, forward)) {
            qWarning() << "Failed to replay a Rust batch clip geometry transaction";
            return;
        }
        QList<int> clipIds;
        clipIds.reserve(m_moves.size());
        for (const auto &move : std::as_const(m_moves)) {
            clipIds.append(move.clipId);
        }
        m_service->publishClipGeometryChange(clipIds);
        return;
    }
    if (!forward) {
        qWarning() << "Cannot undo an uncommitted Rust batch clip geometry transaction";
        return;
    }
    m_service->beginTimelineProjectionTransaction();
    for (const auto &move : std::as_const(m_moves)) {
        m_service->updateClipInternal(
            move.clipId, forward ? move.newLayer : move.oldLayer,
            forward ? move.newStart : move.oldStart,
            forward ? move.newDuration : move.oldDuration, false,
            forward ? m_prevalidated : true);
    }
    if (m_service->endTimelineProjectionTransaction(&m_transaction)) {
        emit m_service->clipsChanged();
    }
}

SetClipByUpperObjectCommand::SetClipByUpperObjectCommand(TimelineService *service, int clipId, bool enabled) : m_service(service), m_clipId(clipId), m_enabled(enabled) {
    setText(enabled ? QObject::tr("上のオブジェクトでクリッピング") : QObject::tr("上のオブジェクトでクリッピング解除"));
}
void SetClipByUpperObjectCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        m_service->publishClipCompositingChange(m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust clip compositing transaction";
    }
}
void SetClipByUpperObjectCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setClipByUpperObjectInternal(m_clipId, m_enabled);
            })) {
            qWarning() << "Failed to capture a Rust clip compositing transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        m_service->publishClipCompositingChange(m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust clip compositing transaction";
    }
}

UpdateEffectParamCommand::UpdateEffectParamCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, QVariant newValue, QVariant oldValue, const QString &effectName) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_paramName(paramName), m_newValue(std::move(newValue)), m_oldValue(std::move(oldValue)), m_effectName(effectName) {
    setText(QObject::tr("パラメータ変更: %1 - %2").arg(effectName).arg(paramName));
}
void UpdateEffectParamCommand::undo() {
    const auto *clip = m_service->findClipById(m_clipId);
    const int previousDuration = clip != nullptr ? clip->durationFrames : 0;
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        m_service->publishEffectParameterChange(m_clipId, m_effectIndex, m_paramName,
                                                previousDuration);
    } else {
        qWarning() << "Failed to undo a Rust effect parameter transaction";
    }
}
void UpdateEffectParamCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->updateEffectParamInternal(m_clipId, m_effectIndex, m_paramName,
                                                     m_newValue);
            })) {
            qWarning() << "Failed to capture a Rust effect parameter transaction";
        }
        return;
    }
    const auto *clip = m_service->findClipById(m_clipId);
    const int previousDuration = clip != nullptr ? clip->durationFrames : 0;
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        m_service->publishEffectParameterChange(m_clipId, m_effectIndex, m_paramName,
                                                previousDuration);
    } else {
        qWarning() << "Failed to redo a Rust effect parameter transaction";
    }
}
auto UpdateEffectParamCommand::id() const -> int { return 1001; } // パラメータ変更コマンドのID
auto UpdateEffectParamCommand::mergeWith(const QUndoCommand *other) -> bool {
    if (other->id() != id()) {
        return false;
    }
    const auto *cmd = dynamic_cast<const UpdateEffectParamCommand *>(other);
    if (cmd->m_clipId != m_clipId || cmd->m_effectIndex != m_effectIndex || cmd->m_paramName != m_paramName) {
        return false;
    }
    if (!m_service->mergeTimelineEditTransactions(m_transaction, cmd->m_transaction)) {
        return false;
    }
    m_newValue = cmd->m_newValue; // 連続する同じパラメータの変更はマージする
    return true;
}

AddEffectCommand::AddEffectCommand(TimelineService *service, int clipId, QString effectId, const QString &effectName)
    : m_service(service), m_clipId(clipId), m_effectId(std::move(effectId)),
      m_effectName(effectName), m_snapshotOwner(std::make_unique<QObject>()) { // NOLINT(bugprone-easily-swappable-parameters)
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(QObject::tr("エフェクト追加: %1").arg(effectName));
}
void AddEffectCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust effect insertion transaction";
    }
}
void AddEffectCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->addEffectInternal(m_clipId, m_effectId);
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust effect insertion transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to redo a Rust effect insertion transaction";
    }
}

RemoveEffectCommand::RemoveEffectCommand(TimelineService *service, int clipId, int effectIndex, const QString &effectName)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex),
      m_effectName(effectName), m_snapshotOwner(std::make_unique<QObject>()) { // NOLINT(bugprone-easily-swappable-parameters)
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(QObject::tr("エフェクト削除: %1").arg(effectName));
}
void RemoveEffectCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->removeEffectInternal(m_clipId, m_effectIndex);
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust effect removal transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to redo a Rust effect removal transaction";
    }
}
void RemoveEffectCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust effect removal transaction";
    }
}

RemoveMultipleEffectsCommand::RemoveMultipleEffectsCommand(TimelineService *service, int clipId, const QList<int> &sortedDescIndices, const QString &macroText)
    : m_service(service), m_clipId(clipId), m_sortedDescIndices(sortedDescIndices),
      m_snapshotOwner(std::make_unique<QObject>()) {
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(macroText);
}

void RemoveMultipleEffectsCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->removeMultipleEffectsInternal(m_clipId, m_sortedDescIndices,
                                                         &m_removedEffectsData);
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust multiple-effect removal transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to redo a Rust multiple-effect removal transaction";
    }
}

void RemoveMultipleEffectsCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust multiple-effect removal transaction";
    }
}

ReorderMultipleEffectsCommand::ReorderMultipleEffectsCommand(TimelineService *service, int clipId, QList<int> redoPerm, QList<int> undoPerm, const QString &text)
    : m_service(service), m_clipId(clipId), m_redoPerm(std::move(redoPerm)),
      m_undoPerm(std::move(undoPerm)), m_snapshotOwner(std::make_unique<QObject>()) {
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(text);
}
void ReorderMultipleEffectsCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust effect reorder transaction";
    }
}
void ReorderMultipleEffectsCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->applyPermutationInternal(m_clipId, m_redoPerm);
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust effect reorder transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to redo a Rust effect reorder transaction";
    }
}

ReorderAudioPluginCommand::ReorderAudioPluginCommand(TimelineService *service, int clipId, QList<int> redoPerm, QList<int> undoPerm)
    : m_service(service), m_clipId(clipId), m_redoPerm(std::move(redoPerm)),
      m_undoPerm(std::move(undoPerm)), m_snapshotOwner(std::make_unique<QObject>()) {
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(QObject::tr("オーディオプラグイン順序変更"));
}
void ReorderAudioPluginCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin reorder transaction";
    }
}
void ReorderAudioPluginCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->applyAudioPluginPermutationInternal(m_clipId, m_redoPerm);
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust audio plugin reorder transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin reorder transaction";
    }
}

SetEffectEnabledCommand::SetEffectEnabledCommand(TimelineService *service, int clipId, int effectIndex, bool enabled) : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_enabled(enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("エフェクト有効/無効切り替え"));
}
void SetEffectEnabledCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        emit m_service->clipEffectsChanged(m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust effect enabled-state transaction";
    }
}
void SetEffectEnabledCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setEffectEnabledInternal(m_clipId, m_effectIndex, m_enabled);
            })) {
            qWarning() << "Failed to capture a Rust effect enabled-state transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        emit m_service->clipEffectsChanged(m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust effect enabled-state transaction";
    }
}

SetAudioPluginEnabledCommand::SetAudioPluginEnabledCommand(TimelineService *service, int clipId, int index, bool enabled) : m_service(service), m_clipId(clipId), m_index(index), m_enabled(enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("オーディオプラグイン有効/無効切り替え"));
}
void SetAudioPluginEnabledCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin enabled-state transaction";
    }
}
void SetAudioPluginEnabledCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setAudioPluginEnabledInternal(m_clipId, m_index, m_enabled);
            })) {
            qWarning() << "Failed to capture a Rust audio plugin enabled-state transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin enabled-state transaction";
    }
}

PasteEffectCommand::PasteEffectCommand(TimelineService *service, int clipId, int targetIndex, EffectModel *templateEffect)
    : m_service(service), m_clipId(clipId), m_targetIndex(targetIndex),
      m_effect(templateEffect->clone()), m_snapshotOwner(std::make_unique<QObject>()) { // NOLINT(bugprone-easily-swappable-parameters)
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));

    setText(QObject::tr("エフェクト貼り付け"));
}
void PasteEffectCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust pasted-effect transaction";
    }
}
void PasteEffectCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->pasteEffectInternal(m_clipId, m_targetIndex, m_effect.get());
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust pasted-effect transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipEffectsChanged(m_clipId);
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to redo a Rust pasted-effect transaction";
    }
}

UpdateLayerStateCommand::UpdateLayerStateCommand(TimelineService *service, int sceneId, int layer, bool value, StateType type)
    : m_service(service), m_sceneId(sceneId), m_layer(layer), m_value(value), m_type(type) { // NOLINT(bugprone-easily-swappable-parameters)
    QString actionName = (type == Lock) ? (value ? QObject::tr("レイヤーロック") : QObject::tr("ロック解除")) : (value ? QObject::tr("レイヤー非表示") : QObject::tr("レイヤー表示"));
    setText(QObject::tr("%1: レイヤー %2").arg(actionName).arg(m_layer));
}
void UpdateLayerStateCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        emit m_service->layerStateChanged(m_layer);
    } else {
        qWarning() << "Failed to undo a Rust layer-state transaction";
    }
}
void UpdateLayerStateCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setLayerStateInternal(m_sceneId, m_layer, m_value, m_type);
            })) {
            qWarning() << "Failed to capture a Rust layer-state transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        emit m_service->layerStateChanged(m_layer);
    } else {
        qWarning() << "Failed to redo a Rust layer-state transaction";
    }
}

SplitClipCommand::SplitClipCommand(TimelineService *service, int clipId, int frame, int originalDuration, int firstDuration, int secondDuration, const QString &clipName)
    : m_service(service), m_originalClipId(clipId), m_newClipId(-1), m_splitFrame(frame), m_originalDuration(originalDuration), m_firstDuration(firstDuration), m_secondDuration(secondDuration),
      m_clipName(clipName), m_snapshotOwner(std::make_unique<QObject>()) { // NOLINT(bugprone-easily-swappable-parameters)
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(QObject::tr("クリップ分割: %1").arg(clipName));
}

SplitClipCommand::~SplitClipCommand() = default;

void SplitClipCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust clip split transaction";
    }
}

void SplitClipCommand::redo() {
    if (m_transaction.isValid()) {
        if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
            emit m_service->clipsChanged();
        } else {
            qWarning() << "Failed to redo a Rust clip split transaction";
        }
        return;
    }

    const bool captured = m_service->captureTimelineEdit(m_transaction, [this]() {
        m_service->beginTimelineProjectionTransaction();
        auto *original = m_service->findClipById(m_originalClipId);
        if (original == nullptr) {
            m_service->endTimelineProjectionTransaction();
            return;
        }

        // 分割前の状態を保存・計算
        if (m_newClipId == -1) {
            m_newClipId = m_service->allocateClipId();
            if (m_newClipId < 0) {
                m_service->endTimelineProjectionTransaction();
                return;
            }
        }

        // 後半部分のクリップを作成
        ClipData newClip = m_service->deepCopyClip(*original);
        newClip.id = m_newClipId;
        newClip.startFrame = m_splitFrame;
        newClip.durationFrames = m_secondDuration;

        for (int i = 0; i < original->effects.size() && i < newClip.effects.size(); ++i) {
            auto *originalEffect = original->effects.value(i);
            auto *newEffect = newClip.effects.value(i);
            if ((originalEffect == nullptr) || (newEffect == nullptr)) {
                continue;
            }

            QVariantMap secondHalfTracks =
                originalEffect->splitTracks(m_firstDuration, m_originalDuration);
            originalEffect->syncTrackEndpoints(m_firstDuration);
            newEffect->setKeyframeTracks(secondHalfTracks);
            newEffect->syncTrackEndpoints(m_secondDuration);
        }

        for (int i = 0;
             i < original->audioPlugins.size() && i < newClip.audioPlugins.size(); ++i) {
            auto &originalPlugin = original->audioPlugins[i];
            auto &newPlugin = newClip.audioPlugins[i];
            QVariantMap firstTracks = originalPlugin.keyframeTracks;
            QVariantMap secondTracks;
            for (auto it = originalPlugin.params.cbegin();
                 it != originalPlugin.params.cend(); ++it) {
                const auto result = AviQtl::Core::RustKeyframeDocument::split(
                    firstTracks.value(it.key()), it.value(), m_firstDuration,
                    m_originalDuration);
                if (!result) {
                    continue;
                }
                firstTracks[it.key()] = result->track;
                if (result->secondaryTrack) {
                    secondTracks[it.key()] = *result->secondaryTrack;
                }
            }
            originalPlugin.keyframeTracks = firstTracks;
            originalPlugin.invalidateKeyframeCache();
            newPlugin.keyframeTracks = secondTracks;
            newPlugin.invalidateKeyframeCache();
        }

        m_service->updateClipInternal(m_originalClipId, original->layer,
                                      original->startFrame, m_firstDuration, false, true,
                                      false);
        m_service->addClipDirectInternal(newClip, false, false);
        const QVariantMap request{
            {QStringLiteral("operation"), QStringLiteral("split_clip")},
            {QStringLiteral("clip_id"), m_originalClipId},
            {QStringLiteral("frame"), m_splitFrame},
            {QStringLiteral("new_clip_id"), m_newClipId},
        };
        static_cast<void>(m_service->commitTimelineEdit(request, {}));
        if (m_service->endTimelineProjectionTransaction()) {
            emit m_service->clipsChanged();
        } else {
            const ClipData &originalSnapshot = m_before.first().clip;
            if (auto *restored = m_service->findClipById(m_originalClipId);
                restored != nullptr) {
                restored->layer = originalSnapshot.layer;
                restored->startFrame = originalSnapshot.startFrame;
                restored->durationFrames = originalSnapshot.durationFrames;
                restored->params = originalSnapshot.params;
                restored->audioPlugins = originalSnapshot.audioPlugins;
                for (auto &plugin : restored->audioPlugins) {
                    plugin.invalidateKeyframeCache();
                }
                for (qsizetype index = 0;
                     index < restored->effects.size() &&
                     index < originalSnapshot.effects.size();
                     ++index) {
                    auto *effect = restored->effects.at(index);
                    const auto *snapshotEffect = originalSnapshot.effects.at(index);
                    if (effect == nullptr || snapshotEffect == nullptr) {
                        continue;
                    }
                    effect->setEnabled(snapshotEffect->isEnabled());
                    effect->setParams(snapshotEffect->params());
                    effect->syncTrackEndpoints(originalSnapshot.durationFrames);
                    effect->setKeyframeTracks(snapshotEffect->keyframeTracks());
                }
            }
            for (auto *effect : std::as_const(newClip.effects)) {
                if (effect != nullptr) {
                    effect->deleteLater();
                }
            }
            m_newClipId = -1;
        }
    });
    if (!captured || m_newClipId < 0 ||
        !captureClipProjections(m_service, {m_originalClipId, m_newClipId}, m_after,
                                m_snapshotOwner.get())) {
        m_transaction.clear();
        qWarning() << "Failed to capture a Rust clip split transaction";
    }
}

DeleteClipsCommand::DeleteClipsCommand(TimelineService *service, const QList<int> &clipIds,
                                       const QString &macroText)
    : m_service(service), m_snapshotOwner(std::make_unique<QObject>()) {
    setText(macroText);
    for (int id : std::as_const(clipIds)) {
        if (m_clipIds.contains(id)) {
            continue;
        }
        ClipProjectionRestore restore;
        if (captureClipProjection(service, id, restore, m_snapshotOwner.get())) {
            m_clipIds.append(id);
            m_restores.append(std::move(restore));
        }
    }
}

DeleteClipsCommand::~DeleteClipsCommand() = default;

void DeleteClipsCommand::redo() {
    if (m_clipIds.isEmpty()) {
        return;
    }
    if (m_transaction.isValid()) {
        if (!m_service->applyTimelineEditTransaction(
                m_transaction, true,
                [this]() { return m_service->removeClipProjectionsInternal(m_clipIds); },
                [this]() { return m_service->restoreClipProjectionsInternal(m_restores); })) {
            qWarning() << "Failed to redo a Rust clip deletion transaction";
            return;
        }
        emit m_service->clipsChanged();
        return;
    }

    m_service->beginTimelineProjectionTransaction();
    for (int id : std::as_const(m_clipIds)) {
        m_service->deleteClipInternal(id, false);
    }
    if (!m_service->endTimelineProjectionTransaction(&m_transaction)) {
        qWarning() << "Failed to commit a Rust clip deletion transaction";
        return;
    }
    emit m_service->clipsChanged();
}

void DeleteClipsCommand::undo() {
    if (!m_transaction.isValid()) {
        return;
    }
    if (m_service->applyTimelineEditTransaction(
            m_transaction, false,
            [this]() { return m_service->restoreClipProjectionsInternal(m_restores); },
            [this]() { return m_service->removeClipProjectionsInternal(m_clipIds); })) {
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust clip deletion transaction";
    }
}

CutClipCommand::CutClipCommand(TimelineService *service, int clipId, const QString &clipName)
    : m_service(service), m_clipId(clipId), m_snapshotOwner(std::make_unique<QObject>()) {
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(QObject::tr("切り取り: %1").arg(clipName));
}
void CutClipCommand::redo() {
    if (m_before.isEmpty()) {
        return;
    }
    m_service->setClipboard(m_before.first().clip);
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->deleteClipInternal(m_clipId);
            })) {
            qWarning() << "Failed to capture a Rust clip cut transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to redo a Rust clip cut transaction";
    }
}
void CutClipCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipsChanged();
        publishClipCreated(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust clip cut transaction";
    }
}

PasteClipCommand::PasteClipCommand(TimelineService *service, int newClipId, const ClipData &clipData)
    : m_service(service), m_newClipId(newClipId), m_clipData(service->deepCopyClip(clipData)),
      m_snapshotOwner(std::make_unique<QObject>()) {
    m_clipData.id = newClipId;
    for (auto *effect : std::as_const(m_clipData.effects)) {
        if (effect != nullptr) {
            effect->setParent(m_snapshotOwner.get());
        }
    }
    setText(QObject::tr("貼り付け: %1").arg(clipData.type));
}
void PasteClipCommand::redo() {
    if (!m_transaction.isValid()) {
        ClipData clip = m_service->deepCopyClip(m_clipData);
        clip.id = m_newClipId;
        if (!m_service->captureTimelineEdit(m_transaction, [this, clip]() {
                static_cast<void>(m_service->addClipDirectInternal(clip));
            }) ||
            !captureClipProjections(m_service, {m_newClipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust pasted-clip transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->clipsChanged();
        publishClipCreated(m_service, m_newClipId);
    } else {
        qWarning() << "Failed to redo a Rust pasted-clip transaction";
    }
}
void PasteClipCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->clipsChanged();
    } else {
        qWarning() << "Failed to undo a Rust pasted-clip transaction";
    }
}

SetKeyframeCommand::SetKeyframeCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, int frame, QVariant newValue, QVariantMap options, QVariant oldValue, QVariantMap oldOptions,
                                       bool wasExisting) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_frame(frame), m_paramName(paramName), m_newValue(std::move(newValue)), m_oldValue(std::move(oldValue)), m_newOptions(std::move(options)), m_oldOptions(std::move(oldOptions)),
      m_wasExisting(wasExisting) {
    setText(QObject::tr("キーフレーム設定: %1").arg(paramName));
}
void SetKeyframeCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_frame,
                                               m_newValue, m_newOptions);
            })) {
            qWarning() << "Failed to capture a Rust effect keyframe transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishEffectKeyframeChange(m_service, m_clipId, m_effectIndex, m_paramName,
                                    m_newValue);
    } else {
        qWarning() << "Failed to redo a Rust effect keyframe transaction";
    }
}
void SetKeyframeCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishEffectKeyframeChange(m_service, m_clipId, m_effectIndex, m_paramName,
                                    m_wasExisting ? m_oldValue : QVariant());
    } else {
        qWarning() << "Failed to undo a Rust effect keyframe transaction";
    }
}
auto SetKeyframeCommand::id() const -> int { return 1002; }
auto SetKeyframeCommand::mergeWith(const QUndoCommand *other) -> bool {
    if (other->id() != id()) {
        return false;
    }
    const auto *cmd = dynamic_cast<const SetKeyframeCommand *>(other);
    if (cmd->m_clipId != m_clipId || cmd->m_effectIndex != m_effectIndex || cmd->m_paramName != m_paramName || cmd->m_frame != m_frame) {
        return false;
    }
    if (!m_service->mergeTimelineEditTransactions(m_transaction, cmd->m_transaction)) {
        return false;
    }
    m_newValue = cmd->m_newValue;
    m_newOptions = cmd->m_newOptions;
    return true;
}

RemoveKeyframeCommand::RemoveKeyframeCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, int frame, QVariant savedValue, QVariantMap savedOptions) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_frame(frame), m_paramName(paramName), m_savedValue(std::move(savedValue)), m_savedOptions(std::move(savedOptions)) {
    setText(QObject::tr("キーフレーム削除: %1 [%2]").arg(paramName).arg(frame));
}
void RemoveKeyframeCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->removeKeyframeInternal(m_clipId, m_effectIndex, m_paramName,
                                                  m_frame);
            })) {
            qWarning() << "Failed to capture a Rust effect keyframe removal transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishEffectKeyframeChange(m_service, m_clipId, m_effectIndex, m_paramName,
                                    QVariant());
    } else {
        qWarning() << "Failed to redo a Rust effect keyframe removal transaction";
    }
}
void RemoveKeyframeCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishEffectKeyframeChange(m_service, m_clipId, m_effectIndex, m_paramName,
                                    m_savedValue);
    } else {
        qWarning() << "Failed to undo a Rust effect keyframe removal transaction";
    }
}

MoveKeyframeCommand::MoveKeyframeCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, int oldFrame, int newFrame) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_oldFrame(oldFrame), m_newFrame(newFrame), m_paramName(paramName) {
    setText(QObject::tr("キーフレーム移動: %1 [%2 -> %3]").arg(paramName).arg(oldFrame).arg(newFrame));
}
void MoveKeyframeCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->moveKeyframeInternal(m_clipId, m_effectIndex, m_paramName,
                                                m_oldFrame, m_newFrame);
            })) {
            qWarning() << "Failed to capture a Rust effect keyframe move transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishEffectKeyframeChange(m_service, m_clipId, m_effectIndex, m_paramName,
                                    QVariant());
    } else {
        qWarning() << "Failed to redo a Rust effect keyframe move transaction";
    }
}
void MoveKeyframeCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishEffectKeyframeChange(m_service, m_clipId, m_effectIndex, m_paramName,
                                    QVariant());
    } else {
        qWarning() << "Failed to undo a Rust effect keyframe move transaction";
    }
}

AddSceneCommand::AddSceneCommand(TimelineService *service, int sceneId, const QString &name)
    : m_service(service), m_sceneId(sceneId), m_name(name),
      m_snapshotOwner(std::make_unique<QObject>()) {
    setText(QObject::tr("シーン追加: %1").arg(name));
}
void AddSceneCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->createSceneInternal(m_sceneId, m_name);
            }) ||
            !([this]() {
                SceneProjectionRestore restore;
                if (!captureSceneProjection(m_service, m_sceneId, restore,
                                            m_snapshotOwner.get())) {
                    return false;
                }
                m_after = {std::move(restore)};
                return true;
            })()) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust scene creation transaction";
        }
        return;
    }
    if (replaySceneProjections(m_service, m_transaction, true, m_before, m_after)) {
        emit m_service->scenesChanged();
        m_service->switchScene(m_sceneId);
    } else {
        qWarning() << "Failed to redo a Rust scene creation transaction";
    }
}
void AddSceneCommand::undo() {
    if (replaySceneProjections(m_service, m_transaction, false, m_before, m_after)) {
        if (m_service->currentSceneId() == m_sceneId) {
            m_service->switchScene(0);
        }
        emit m_service->scenesChanged();
    } else {
        qWarning() << "Failed to undo a Rust scene creation transaction";
    }
}

RemoveSceneCommand::RemoveSceneCommand(TimelineService *service, int sceneId, const QString &name)
    : m_service(service), m_sceneId(sceneId),
      m_snapshotOwner(std::make_unique<QObject>()) {
    const auto &scenes = service->getAllScenes();
    for (qsizetype index = 0; index < scenes.size(); ++index) {
        if (scenes.at(index).id == sceneId) {
            SceneProjectionRestore restore;
            if (captureSceneProjection(service, sceneId, restore, m_snapshotOwner.get())) {
                m_before = {std::move(restore)};
            }
            break;
        }
    }
    setText(QObject::tr("シーン削除: %1").arg(name));
}
void RemoveSceneCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->removeSceneInternal(m_sceneId);
            })) {
            qWarning() << "Failed to capture a Rust scene removal transaction";
        }
        return;
    }
    if (replaySceneProjections(m_service, m_transaction, true, m_before, m_after)) {
        if (m_service->currentSceneId() == m_sceneId) {
            m_service->switchScene(0);
        }
        emit m_service->scenesChanged();
    } else {
        qWarning() << "Failed to redo a Rust scene removal transaction";
    }
}
void RemoveSceneCommand::undo() {
    if (replaySceneProjections(m_service, m_transaction, false, m_before, m_after)) {
        emit m_service->scenesChanged();
    } else {
        qWarning() << "Failed to undo a Rust scene removal transaction";
    }
}

UpdateSceneSettingsCommand::UpdateSceneSettingsCommand(TimelineService *service, int sceneId, SceneData oldData, const SceneData &newData)
    : m_service(service), m_sceneId(sceneId), m_oldData(std::move(oldData)), m_newData(newData) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("シーン設定変更: %1").arg(newData.name));
}
void UpdateSceneSettingsCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->applySceneSettingsInternal(m_sceneId, m_newData);
            })) {
            qWarning() << "Failed to capture a Rust scene settings transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        emit m_service->scenesChanged();
    } else {
        qWarning() << "Failed to redo a Rust scene settings transaction";
    }
}
void UpdateSceneSettingsCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        emit m_service->scenesChanged();
    } else {
        qWarning() << "Failed to undo a Rust scene settings transaction";
    }
}

SetAudioPluginKeyframeCommand::SetAudioPluginKeyframeCommand(TimelineService *service, int clipId, int pluginIndex, const QString &paramKey, int frame, QVariant newValue, QVariantMap options, QVariant oldValue, QVariantMap oldOptions,
                                                             bool wasExisting) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_frame(frame), m_paramKey(paramKey), m_newValue(std::move(newValue)), m_oldValue(std::move(oldValue)), m_newOptions(std::move(options)), m_oldOptions(std::move(oldOptions)),
      m_wasExisting(wasExisting) {
    setText(QObject::tr("オーディオプラグインキーフレーム設定: %1").arg(paramKey));
}
void SetAudioPluginKeyframeCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setAudioPluginKeyframeInternal(
                    m_clipId, m_pluginIndex, m_paramKey, m_frame, m_newValue, m_newOptions);
            })) {
            qWarning() << "Failed to capture a Rust audio plugin keyframe transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin keyframe transaction";
    }
}
void SetAudioPluginKeyframeCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin keyframe transaction";
    }
}
auto SetAudioPluginKeyframeCommand::id() const -> int { return 1003; }
auto SetAudioPluginKeyframeCommand::mergeWith(const QUndoCommand *other) -> bool {
    if (other->id() != id()) {
        return false;
    }
    const auto *cmd = dynamic_cast<const SetAudioPluginKeyframeCommand *>(other);
    if (cmd->m_clipId != m_clipId || cmd->m_pluginIndex != m_pluginIndex || cmd->m_paramKey != m_paramKey || cmd->m_frame != m_frame) {
        return false;
    }
    if (!m_service->mergeTimelineEditTransactions(m_transaction, cmd->m_transaction)) {
        return false;
    }
    m_newValue = cmd->m_newValue;
    m_newOptions = cmd->m_newOptions;
    return true;
}

RemoveAudioPluginKeyframeCommand::RemoveAudioPluginKeyframeCommand(TimelineService *service, int clipId, int pluginIndex, const QString &paramKey, int frame, QVariant savedValue, QVariantMap savedOptions) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_frame(frame), m_paramKey(paramKey), m_savedValue(std::move(savedValue)), m_savedOptions(std::move(savedOptions)) {
    setText(QObject::tr("オーディオプラグインキーフレーム削除: %1 [%2]").arg(paramKey).arg(frame));
}
void RemoveAudioPluginKeyframeCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->removeAudioPluginKeyframeInternal(m_clipId, m_pluginIndex,
                                                             m_paramKey, m_frame);
            })) {
            qWarning() << "Failed to capture a Rust audio plugin keyframe removal transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin keyframe removal transaction";
    }
}
void RemoveAudioPluginKeyframeCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin keyframe removal transaction";
    }
}

MoveAudioPluginKeyframeCommand::MoveAudioPluginKeyframeCommand(TimelineService *service, int clipId, int pluginIndex, const QString &paramKey, int oldFrame, int newFrame) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_oldFrame(oldFrame), m_newFrame(newFrame), m_paramKey(paramKey) {
    setText(QObject::tr("オーディオプラグインキーフレーム移動: %1 [%2 -> %3]").arg(paramKey).arg(oldFrame).arg(newFrame));
}
void MoveAudioPluginKeyframeCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->moveAudioPluginKeyframeInternal(m_clipId, m_pluginIndex,
                                                           m_paramKey, m_oldFrame, m_newFrame);
            })) {
            qWarning() << "Failed to capture a Rust audio plugin keyframe move transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin keyframe move transaction";
    }
}
void MoveAudioPluginKeyframeCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin keyframe move transaction";
    }
}

SetAudioPluginParamCommand::SetAudioPluginParamCommand(TimelineService *service, int clipId, int pluginIndex, int paramIndex, float newValue, float oldValue, const QString &pluginName) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_paramIndex(paramIndex), m_newValue(newValue), m_oldValue(oldValue), m_pluginName(pluginName) {
    setText(QObject::tr("オーディオプラグインパラメータ変更: %1").arg(pluginName));
}
void SetAudioPluginParamCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->setAudioPluginParamInternal(m_clipId, m_pluginIndex,
                                                       m_paramIndex, m_newValue);
            })) {
            qWarning() << "Failed to capture a Rust audio plugin parameter transaction";
        }
        return;
    }
    if (m_service->applyTimelineEditTransaction(m_transaction, true)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin parameter transaction";
    }
}
void SetAudioPluginParamCommand::undo() {
    if (m_service->applyTimelineEditTransaction(m_transaction, false)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin parameter transaction";
    }
}
auto SetAudioPluginParamCommand::id() const -> int { return 1004; }
auto SetAudioPluginParamCommand::mergeWith(const QUndoCommand *other) -> bool {
    if (other->id() != id()) {
        return false;
    }
    const auto *cmd = dynamic_cast<const SetAudioPluginParamCommand *>(other);
    if (cmd->m_clipId != m_clipId || cmd->m_pluginIndex != m_pluginIndex || cmd->m_paramIndex != m_paramIndex) {
        return false;
    }
    if (!m_service->mergeTimelineEditTransactions(m_transaction, cmd->m_transaction)) {
        return false;
    }
    m_newValue = cmd->m_newValue;
    return true;
}

AddAudioPluginCommand::AddAudioPluginCommand(TimelineService *service, int clipId, const AudioPluginState &state, const QString &pluginName)
    : m_service(service), m_clipId(clipId), m_state(state), m_insertedIndex(-1),
      m_snapshotOwner(std::make_unique<QObject>()) {
    static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                             m_snapshotOwner.get()));
    setText(QObject::tr("オーディオプラグイン追加: %1").arg(pluginName));
}
void AddAudioPluginCommand::redo() {
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_insertedIndex = m_service->addAudioPluginStateInternal(m_clipId, m_state);
            }) ||
            m_insertedIndex < 0 ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust audio plugin insertion transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin insertion transaction";
    }
}
void AddAudioPluginCommand::undo() {
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin insertion transaction";
    }
}

RemoveAudioPluginCommand::RemoveAudioPluginCommand(TimelineService *service, int clipId, int index,
                                                   const QString &pluginName,
                                                   QVariantMap document)
    : m_service(service), m_clipId(clipId), m_index(index),
      m_savedDocument(std::move(document)), m_valid(false),
      m_snapshotOwner(std::make_unique<QObject>()) {
    setText(QObject::tr("オーディオプラグイン削除: %1").arg(pluginName));
    const auto *clip = service->findClipById(clipId);
    if (clip != nullptr && index >= 0 && index < clip->audioPlugins.size()) {
        m_savedState = clip->audioPlugins.at(index);
        m_valid = true;
        static_cast<void>(captureClipProjections(service, {clipId}, m_before,
                                                 m_snapshotOwner.get()));
    }
}
void RemoveAudioPluginCommand::redo() {
    if (!m_valid) {
        return;
    }
    if (!m_transaction.isValid()) {
        if (!m_service->captureTimelineEdit(m_transaction, [this]() {
                m_service->removeAudioPluginStateInternal(m_clipId, m_index);
            }) ||
            !captureClipProjections(m_service, {m_clipId}, m_after,
                                    m_snapshotOwner.get())) {
            m_transaction.clear();
            qWarning() << "Failed to capture a Rust audio plugin removal transaction";
        }
        return;
    }
    if (replayClipProjections(m_service, m_transaction, true, m_before, m_after)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to redo a Rust audio plugin removal transaction";
    }
}
void RemoveAudioPluginCommand::undo() {
    if (!m_valid) {
        return;
    }
    if (replayClipProjections(m_service, m_transaction, false, m_before, m_after)) {
        publishAudioPluginChange(m_service, m_clipId);
    } else {
        qWarning() << "Failed to undo a Rust audio plugin removal transaction";
    }
}

} // namespace AviQtl::UI
