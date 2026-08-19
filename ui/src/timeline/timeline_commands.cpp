#include "commands.hpp"
#include "effect_registry.hpp"
#include "rust_keyframe_document.hpp"
#include "timeline_service.hpp"
#include <QObject>

namespace AviQtl::UI {

AddClipCommand::AddClipCommand(TimelineService *service, int clipId, QString type, int startFrame, int layer, const QString &clipName, int duration, QString effectId,
                               QVariantMap effectParams) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_type(std::move(type)), m_startFrame(startFrame), m_layer(layer), m_clipName(clipName), m_duration(duration), m_effectId(std::move(effectId)), m_effectParams(std::move(effectParams)) {
    setText(QObject::tr("クリップ追加: %1").arg(clipName));
}
void AddClipCommand::undo() { m_service->deleteClipInternal(m_clipId); }
void AddClipCommand::redo() {
    m_service->createClipInternal(m_clipId, m_type, m_startFrame, m_layer, false, m_duration,
                                  m_effectId, m_effectParams);
    auto *clip = m_service->findClipById(m_clipId);
    if (clip == nullptr) {
        return;
    }
    emit m_service->clipsChanged();
    emit m_service->clipCreated(clip->id, clip->layer, clip->startFrame, clip->durationFrames, clip->type);
}

MoveClipCommand::MoveClipCommand(TimelineService *service, int clipId, int oldLayer, int oldStart, int oldDuration, int newLayer, int newStart, int newDuration, const QString &clipName, bool prevalidated) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_oldLayer(oldLayer), m_oldStart(oldStart), m_oldDuration(oldDuration), m_newLayer(newLayer), m_newStart(newStart), m_newDuration(newDuration), m_clipName(clipName), m_prevalidated(prevalidated) {
    setText(QObject::tr("クリップ移動: %1").arg(clipName));
}
void MoveClipCommand::undo() { m_service->updateClipInternal(m_clipId, m_oldLayer, m_oldStart, m_oldDuration, true, true); }
void MoveClipCommand::redo() { m_service->updateClipInternal(m_clipId, m_newLayer, m_newStart, m_newDuration, true, m_prevalidated); }

MoveClipsCommand::MoveClipsCommand(TimelineService *service, QList<ClipMoveChange> moves,
                                   QString commandText, bool prevalidated)
    : m_service(service), m_moves(std::move(moves)), m_prevalidated(prevalidated) {
    setText(std::move(commandText));
}

void MoveClipsCommand::undo() { apply(false); }
void MoveClipsCommand::redo() { apply(true); }

void MoveClipsCommand::apply(bool forward) {
    m_service->beginTimelineProjectionTransaction();
    for (const auto &move : std::as_const(m_moves)) {
        m_service->updateClipInternal(
            move.clipId, forward ? move.newLayer : move.oldLayer,
            forward ? move.newStart : move.oldStart,
            forward ? move.newDuration : move.oldDuration, false,
            forward ? m_prevalidated : true);
    }
    if (m_service->endTimelineProjectionTransaction()) {
        emit m_service->clipsChanged();
    }
}

SetClipByUpperObjectCommand::SetClipByUpperObjectCommand(TimelineService *service, int clipId, bool enabled) : m_service(service), m_clipId(clipId), m_enabled(enabled) {
    setText(enabled ? QObject::tr("上のオブジェクトでクリッピング") : QObject::tr("上のオブジェクトでクリッピング解除"));
}
void SetClipByUpperObjectCommand::undo() { m_service->setClipByUpperObjectInternal(m_clipId, !m_enabled); }
void SetClipByUpperObjectCommand::redo() { m_service->setClipByUpperObjectInternal(m_clipId, m_enabled); }

UpdateEffectParamCommand::UpdateEffectParamCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, QVariant newValue, QVariant oldValue, const QString &effectName) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_paramName(paramName), m_newValue(std::move(newValue)), m_oldValue(std::move(oldValue)), m_effectName(effectName) {
    setText(QObject::tr("パラメータ変更: %1 - %2").arg(effectName).arg(paramName));
}
void UpdateEffectParamCommand::undo() { m_service->updateEffectParamInternal(m_clipId, m_effectIndex, m_paramName, m_oldValue); }
void UpdateEffectParamCommand::redo() { m_service->updateEffectParamInternal(m_clipId, m_effectIndex, m_paramName, m_newValue); }
auto UpdateEffectParamCommand::id() const -> int { return 1001; } // パラメータ変更コマンドのID
auto UpdateEffectParamCommand::mergeWith(const QUndoCommand *other) -> bool {
    if (other->id() != id()) {
        return false;
    }
    const auto *cmd = dynamic_cast<const UpdateEffectParamCommand *>(other);
    if (cmd->m_clipId != m_clipId || cmd->m_effectIndex != m_effectIndex || cmd->m_paramName != m_paramName) {
        return false;
    }
    m_newValue = cmd->m_newValue; // 連続する同じパラメータの変更はマージする
    redo();                       // モデルを最新値で更新し、シグナルを発火させる
    return true;
}

AddEffectCommand::AddEffectCommand(TimelineService *service, int clipId, QString effectId, const QString &effectName)
    : m_service(service), m_clipId(clipId), m_effectId(std::move(effectId)), m_effectName(effectName) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("エフェクト追加: %1").arg(effectName));
}
void AddEffectCommand::undo() { m_service->removeEffectInternal(m_clipId, -1); }
void AddEffectCommand::redo() { m_service->addEffectInternal(m_clipId, m_effectId); }

RemoveEffectCommand::RemoveEffectCommand(TimelineService *service, int clipId, int effectIndex, const QString &effectName)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_effectName(effectName) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("エフェクト削除: %1").arg(effectName));
}
void RemoveEffectCommand::redo() { m_service->removeEffectInternal(m_clipId, m_effectIndex); }
void RemoveEffectCommand::undo() { m_service->restoreEffectInternal(m_clipId, m_removedEffectData); }

RemoveMultipleEffectsCommand::RemoveMultipleEffectsCommand(TimelineService *service, int clipId, const QList<int> &sortedDescIndices, const QString &macroText) : m_service(service), m_clipId(clipId), m_sortedDescIndices(sortedDescIndices) {
    setText(macroText);
}

void RemoveMultipleEffectsCommand::redo() { m_service->removeMultipleEffectsInternal(m_clipId, m_sortedDescIndices, &m_removedEffectsData); }

void RemoveMultipleEffectsCommand::undo() { m_service->restoreMultipleEffectsInternal(m_clipId, m_removedEffectsData); }

ReorderMultipleEffectsCommand::ReorderMultipleEffectsCommand(TimelineService *service, int clipId, QList<int> redoPerm, QList<int> undoPerm, const QString &text)
    : m_service(service), m_clipId(clipId), m_redoPerm(std::move(redoPerm)), m_undoPerm(std::move(undoPerm)) {
    setText(text);
}
void ReorderMultipleEffectsCommand::undo() { m_service->applyPermutationInternal(m_clipId, m_undoPerm); }
void ReorderMultipleEffectsCommand::redo() { m_service->applyPermutationInternal(m_clipId, m_redoPerm); }

ReorderAudioPluginCommand::ReorderAudioPluginCommand(TimelineService *service, int clipId, QList<int> redoPerm, QList<int> undoPerm)
    : m_service(service), m_clipId(clipId), m_redoPerm(std::move(redoPerm)), m_undoPerm(std::move(undoPerm)) {
    setText(QObject::tr("オーディオプラグイン順序変更"));
}
void ReorderAudioPluginCommand::undo() { m_service->applyAudioPluginPermutationInternal(m_clipId, m_undoPerm); }
void ReorderAudioPluginCommand::redo() { m_service->applyAudioPluginPermutationInternal(m_clipId, m_redoPerm); }

SetEffectEnabledCommand::SetEffectEnabledCommand(TimelineService *service, int clipId, int effectIndex, bool enabled) : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_enabled(enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("エフェクト有効/無効切り替え"));
}
void SetEffectEnabledCommand::undo() { m_service->setEffectEnabledInternal(m_clipId, m_effectIndex, !m_enabled); }
void SetEffectEnabledCommand::redo() { m_service->setEffectEnabledInternal(m_clipId, m_effectIndex, m_enabled); }

SetAudioPluginEnabledCommand::SetAudioPluginEnabledCommand(TimelineService *service, int clipId, int index, bool enabled) : m_service(service), m_clipId(clipId), m_index(index), m_enabled(enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("オーディオプラグイン有効/無効切り替え"));
}
void SetAudioPluginEnabledCommand::undo() { m_service->setAudioPluginEnabledInternal(m_clipId, m_index, !m_enabled); }
void SetAudioPluginEnabledCommand::redo() { m_service->setAudioPluginEnabledInternal(m_clipId, m_index, m_enabled); }

PasteEffectCommand::PasteEffectCommand(TimelineService *service, int clipId, int targetIndex, EffectModel *templateEffect)
    : m_service(service), m_clipId(clipId), m_targetIndex(targetIndex), m_effect(templateEffect->clone()) { // NOLINT(bugprone-easily-swappable-parameters)

    setText(QObject::tr("エフェクト貼り付け"));
}
void PasteEffectCommand::undo() { m_service->removeEffectInternal(m_clipId, m_targetIndex); }
void PasteEffectCommand::redo() { m_service->pasteEffectInternal(m_clipId, m_targetIndex, m_effect.get()); }

UpdateLayerStateCommand::UpdateLayerStateCommand(TimelineService *service, int sceneId, int layer, bool value, StateType type)
    : m_service(service), m_sceneId(sceneId), m_layer(layer), m_value(value), m_type(type) { // NOLINT(bugprone-easily-swappable-parameters)
    QString actionName = (type == Lock) ? (value ? QObject::tr("レイヤーロック") : QObject::tr("ロック解除")) : (value ? QObject::tr("レイヤー非表示") : QObject::tr("レイヤー表示"));
    setText(QObject::tr("%1: レイヤー %2").arg(actionName).arg(m_layer));
}
void UpdateLayerStateCommand::undo() { m_service->setLayerStateInternal(m_sceneId, m_layer, !m_value, m_type); }
void UpdateLayerStateCommand::redo() { m_service->setLayerStateInternal(m_sceneId, m_layer, m_value, m_type); }

SplitClipCommand::SplitClipCommand(TimelineService *service, int clipId, int frame, int originalDuration, int firstDuration, int secondDuration, const QString &clipName)
    : m_service(service), m_originalClipId(clipId), m_newClipId(-1), m_splitFrame(frame), m_originalDuration(originalDuration), m_firstDuration(firstDuration), m_secondDuration(secondDuration),
      m_clipName(clipName) { // NOLINT(bugprone-easily-swappable-parameters)
    if (const auto *clip = service->findClipById(clipId); clip != nullptr) {
        m_originalSnapshot = service->deepCopyClip(*clip);
        m_originalSnapshot.id = clipId;
    }
    setText(QObject::tr("クリップ分割: %1").arg(clipName));
}

SplitClipCommand::~SplitClipCommand() {
    for (auto *effect : std::as_const(m_originalSnapshot.effects)) {
        if (effect != nullptr) {
            effect->deleteLater();
        }
    }
    m_originalSnapshot.effects.clear();
}

void SplitClipCommand::undo() {
    m_service->beginTimelineProjectionTransaction();
    if (m_newClipId >= 0) {
        m_service->deleteClipInternal(m_newClipId, false);
    }
    m_service->deleteClipInternal(m_originalClipId, false);
    ClipData restored = m_service->deepCopyClip(m_originalSnapshot);
    restored.id = m_originalClipId;
    m_service->addClipDirectInternal(restored, false);
    if (m_service->endTimelineProjectionTransaction()) {
        emit m_service->clipsChanged();
    } else {
        for (auto *effect : std::as_const(restored.effects)) {
            if (effect != nullptr) {
                effect->deleteLater();
            }
        }
    }
}

void SplitClipCommand::redo() {
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

        QVariantMap secondHalfTracks = originalEffect->splitTracks(m_firstDuration, m_originalDuration);
        originalEffect->syncTrackEndpoints(m_firstDuration);
        newEffect->setKeyframeTracks(secondHalfTracks);
        newEffect->syncTrackEndpoints(m_secondDuration);
    }

    for (int i = 0; i < original->audioPlugins.size() && i < newClip.audioPlugins.size(); ++i) {
        auto &originalPlugin = original->audioPlugins[i];
        auto &newPlugin = newClip.audioPlugins[i];
        QVariantMap firstTracks = originalPlugin.keyframeTracks;
        QVariantMap secondTracks;
        for (auto it = originalPlugin.params.cbegin(); it != originalPlugin.params.cend(); ++it) {
            const auto result = AviQtl::Core::RustKeyframeDocument::split(
                firstTracks.value(it.key()), it.value(), m_firstDuration, m_originalDuration);
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

    m_service->updateClipInternal(m_originalClipId, original->layer, original->startFrame,
                                  m_firstDuration, false, true, false);
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
        if (auto *restored = m_service->findClipById(m_originalClipId); restored != nullptr) {
            restored->layer = m_originalSnapshot.layer;
            restored->startFrame = m_originalSnapshot.startFrame;
            restored->durationFrames = m_originalSnapshot.durationFrames;
            restored->params = m_originalSnapshot.params;
            restored->audioPlugins = m_originalSnapshot.audioPlugins;
            for (auto &plugin : restored->audioPlugins) {
                plugin.invalidateKeyframeCache();
            }
            for (qsizetype index = 0;
                 index < restored->effects.size() && index < m_originalSnapshot.effects.size();
                 ++index) {
                auto *effect = restored->effects.at(index);
                const auto *snapshotEffect = m_originalSnapshot.effects.at(index);
                if (effect == nullptr || snapshotEffect == nullptr) {
                    continue;
                }
                effect->setEnabled(snapshotEffect->isEnabled());
                effect->setParams(snapshotEffect->params());
                effect->syncTrackEndpoints(m_originalSnapshot.durationFrames);
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
}

DeleteClipsCommand::DeleteClipsCommand(TimelineService *service, const QList<int> &clipIds, const QString &macroText) : m_service(service), m_clipIds(clipIds) {
    setText(macroText);
    for (int id : std::as_const(clipIds)) {
        const auto *clip = service->findClipById(id);
        if (clip != nullptr) {
            ClipData snap = service->deepCopyClip(*clip);
            snap.id = id; // 重要: 削除前の元のIDをスナップショットに保存
            m_snapshots.append(snap);
        }
    }
}
void DeleteClipsCommand::redo() {
    m_service->beginTimelineProjectionTransaction();
    for (int id : std::as_const(m_clipIds)) {
        m_service->deleteClipInternal(id, false);
    }
    if (m_service->endTimelineProjectionTransaction()) {
        emit m_service->clipsChanged();
    }
}
void DeleteClipsCommand::undo() { m_service->addClipsDirectInternal(m_snapshots); }

CutClipCommand::CutClipCommand(TimelineService *service, int clipId, const QString &clipName) : m_service(service), m_clipId(clipId) {
    const auto *clip = service->findClipById(clipId);
    if (clip != nullptr) {
        m_snapshot = service->deepCopyClip(*clip);
    }
    setText(QObject::tr("切り取り: %1").arg(clipName));
}
void CutClipCommand::redo() {
    m_service->setClipboard(m_snapshot);
    m_service->deleteClipInternal(m_clipId);
}
void CutClipCommand::undo() {
    m_snapshot.id = m_clipId;
    m_service->addClipDirectInternal(m_snapshot);
}

PasteClipCommand::PasteClipCommand(TimelineService *service, int newClipId, const ClipData &clipData) : m_service(service), m_newClipId(newClipId), m_clipData(clipData) { setText(QObject::tr("貼り付け: %1").arg(clipData.type)); }
void PasteClipCommand::redo() {
    ClipData c = m_clipData;
    c.id = m_newClipId;
    m_service->addClipDirectInternal(c);
}
void PasteClipCommand::undo() { m_service->deleteClipInternal(m_newClipId); }

SetKeyframeCommand::SetKeyframeCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, int frame, QVariant newValue, QVariantMap options, QVariant oldValue, QVariantMap oldOptions,
                                       bool wasExisting) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_frame(frame), m_paramName(paramName), m_newValue(std::move(newValue)), m_oldValue(std::move(oldValue)), m_newOptions(std::move(options)), m_oldOptions(std::move(oldOptions)),
      m_wasExisting(wasExisting) {
    setText(QObject::tr("キーフレーム設定: %1").arg(paramName));
}
void SetKeyframeCommand::redo() { m_service->setKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_frame, m_newValue, m_newOptions); }
void SetKeyframeCommand::undo() {
    if (m_wasExisting) {
        m_service->setKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_frame, m_oldValue, m_oldOptions);
    } else {
        m_service->removeKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_frame);
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
    m_newValue = cmd->m_newValue;
    m_newOptions = cmd->m_newOptions;
    redo(); // マージ中もモデルを同期
    return true;
}

RemoveKeyframeCommand::RemoveKeyframeCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, int frame, QVariant savedValue, QVariantMap savedOptions) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_frame(frame), m_paramName(paramName), m_savedValue(std::move(savedValue)), m_savedOptions(std::move(savedOptions)) {
    setText(QObject::tr("キーフレーム削除: %1 [%2]").arg(paramName).arg(frame));
}
void RemoveKeyframeCommand::redo() { m_service->removeKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_frame); }
void RemoveKeyframeCommand::undo() { m_service->setKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_frame, m_savedValue, m_savedOptions); }

MoveKeyframeCommand::MoveKeyframeCommand(TimelineService *service, int clipId, int effectIndex, const QString &paramName, int oldFrame, int newFrame) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_effectIndex(effectIndex), m_oldFrame(oldFrame), m_newFrame(newFrame), m_paramName(paramName) {
    setText(QObject::tr("キーフレーム移動: %1 [%2 -> %3]").arg(paramName).arg(oldFrame).arg(newFrame));
}
void MoveKeyframeCommand::redo() { m_service->moveKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_oldFrame, m_newFrame); }
void MoveKeyframeCommand::undo() { m_service->moveKeyframeInternal(m_clipId, m_effectIndex, m_paramName, m_newFrame, m_oldFrame); }

AddSceneCommand::AddSceneCommand(TimelineService *service, int sceneId, const QString &name) : m_service(service), m_sceneId(sceneId), m_name(name) { setText(QObject::tr("シーン追加: %1").arg(name)); }
void AddSceneCommand::redo() { m_service->createSceneInternal(m_sceneId, m_name); }
void AddSceneCommand::undo() { m_service->removeSceneInternal(m_sceneId); }

RemoveSceneCommand::RemoveSceneCommand(TimelineService *service, int sceneId, const QString &name) : m_service(service), m_sceneId(sceneId) {
    const auto &scenes = service->getAllScenes();
    for (qsizetype index = 0; index < scenes.size(); ++index) {
        if (scenes.at(index).id == sceneId) {
            m_sceneIndex = index;
            m_snapshot = scenes.at(index);
            break;
        }
    }
    setText(QObject::tr("シーン削除: %1").arg(name));
}
void RemoveSceneCommand::redo() { m_service->removeSceneInternal(m_sceneId); }
void RemoveSceneCommand::undo() { m_service->restoreSceneInternal(m_snapshot, m_sceneIndex); }

UpdateSceneSettingsCommand::UpdateSceneSettingsCommand(TimelineService *service, int sceneId, SceneData oldData, const SceneData &newData)
    : m_service(service), m_sceneId(sceneId), m_oldData(std::move(oldData)), m_newData(newData) { // NOLINT(bugprone-easily-swappable-parameters)
    setText(QObject::tr("シーン設定変更: %1").arg(newData.name));
}
void UpdateSceneSettingsCommand::redo() { m_service->applySceneSettingsInternal(m_sceneId, m_newData); }
void UpdateSceneSettingsCommand::undo() { m_service->applySceneSettingsInternal(m_sceneId, m_oldData); }

SetAudioPluginKeyframeCommand::SetAudioPluginKeyframeCommand(TimelineService *service, int clipId, int pluginIndex, const QString &paramKey, int frame, QVariant newValue, QVariantMap options, QVariant oldValue, QVariantMap oldOptions,
                                                             bool wasExisting) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_frame(frame), m_paramKey(paramKey), m_newValue(std::move(newValue)), m_oldValue(std::move(oldValue)), m_newOptions(std::move(options)), m_oldOptions(std::move(oldOptions)),
      m_wasExisting(wasExisting) {
    setText(QObject::tr("オーディオプラグインキーフレーム設定: %1").arg(paramKey));
}
void SetAudioPluginKeyframeCommand::redo() { m_service->setAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_frame, m_newValue, m_newOptions); }
void SetAudioPluginKeyframeCommand::undo() {
    if (m_wasExisting) {
        m_service->setAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_frame, m_oldValue, m_oldOptions);
    } else {
        m_service->removeAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_frame);
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
    m_newValue = cmd->m_newValue;
    m_newOptions = cmd->m_newOptions;
    redo();
    return true;
}

RemoveAudioPluginKeyframeCommand::RemoveAudioPluginKeyframeCommand(TimelineService *service, int clipId, int pluginIndex, const QString &paramKey, int frame, QVariant savedValue, QVariantMap savedOptions) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_frame(frame), m_paramKey(paramKey), m_savedValue(std::move(savedValue)), m_savedOptions(std::move(savedOptions)) {
    setText(QObject::tr("オーディオプラグインキーフレーム削除: %1 [%2]").arg(paramKey).arg(frame));
}
void RemoveAudioPluginKeyframeCommand::redo() { m_service->removeAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_frame); }
void RemoveAudioPluginKeyframeCommand::undo() { m_service->setAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_frame, m_savedValue, m_savedOptions); }

MoveAudioPluginKeyframeCommand::MoveAudioPluginKeyframeCommand(TimelineService *service, int clipId, int pluginIndex, const QString &paramKey, int oldFrame, int newFrame) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_oldFrame(oldFrame), m_newFrame(newFrame), m_paramKey(paramKey) {
    setText(QObject::tr("オーディオプラグインキーフレーム移動: %1 [%2 -> %3]").arg(paramKey).arg(oldFrame).arg(newFrame));
}
void MoveAudioPluginKeyframeCommand::redo() { m_service->moveAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_oldFrame, m_newFrame); }
void MoveAudioPluginKeyframeCommand::undo() { m_service->moveAudioPluginKeyframeInternal(m_clipId, m_pluginIndex, m_paramKey, m_newFrame, m_oldFrame); }

SetAudioPluginParamCommand::SetAudioPluginParamCommand(TimelineService *service, int clipId, int pluginIndex, int paramIndex, float newValue, float oldValue, const QString &pluginName) // NOLINT(bugprone-easily-swappable-parameters)
    : m_service(service), m_clipId(clipId), m_pluginIndex(pluginIndex), m_paramIndex(paramIndex), m_newValue(newValue), m_oldValue(oldValue), m_pluginName(pluginName) {
    setText(QObject::tr("オーディオプラグインパラメータ変更: %1").arg(pluginName));
}
void SetAudioPluginParamCommand::redo() { m_service->setAudioPluginParamInternal(m_clipId, m_pluginIndex, m_paramIndex, m_newValue); }
void SetAudioPluginParamCommand::undo() { m_service->setAudioPluginParamInternal(m_clipId, m_pluginIndex, m_paramIndex, m_oldValue); }
auto SetAudioPluginParamCommand::id() const -> int { return 1004; }
auto SetAudioPluginParamCommand::mergeWith(const QUndoCommand *other) -> bool {
    if (other->id() != id()) {
        return false;
    }
    const auto *cmd = dynamic_cast<const SetAudioPluginParamCommand *>(other);
    if (cmd->m_clipId != m_clipId || cmd->m_pluginIndex != m_pluginIndex || cmd->m_paramIndex != m_paramIndex) {
        return false;
    }
    m_newValue = cmd->m_newValue;
    redo();
    return true;
}

AddAudioPluginCommand::AddAudioPluginCommand(TimelineService *service, int clipId, const AudioPluginState &state, const QString &pluginName) : m_service(service), m_clipId(clipId), m_state(state), m_insertedIndex(-1) {
    setText(QObject::tr("オーディオプラグイン追加: %1").arg(pluginName));
}
void AddAudioPluginCommand::redo() { m_insertedIndex = m_service->addAudioPluginStateInternal(m_clipId, m_state); }
void AddAudioPluginCommand::undo() { m_service->removeAudioPluginStateInternal(m_clipId, m_insertedIndex); }

RemoveAudioPluginCommand::RemoveAudioPluginCommand(TimelineService *service, int clipId, int index,
                                                   const QString &pluginName,
                                                   QVariantMap document)
    : m_service(service), m_clipId(clipId), m_index(index),
      m_savedDocument(std::move(document)), m_valid(false) {
    setText(QObject::tr("オーディオプラグイン削除: %1").arg(pluginName));
    const auto *clip = service->findClipById(clipId);
    if (clip != nullptr && index >= 0 && index < clip->audioPlugins.size()) {
        m_savedState = clip->audioPlugins.at(index);
        m_valid = true;
    }
}
void RemoveAudioPluginCommand::redo() {
    if (m_valid) {
        m_service->removeAudioPluginStateInternal(m_clipId, m_index);
    }
}
void RemoveAudioPluginCommand::undo() {
    if (m_valid) {
        m_service->restoreAudioPluginStateInternal(m_clipId, m_index, m_savedState,
                                                   m_savedDocument);
    }
}

} // namespace AviQtl::UI
