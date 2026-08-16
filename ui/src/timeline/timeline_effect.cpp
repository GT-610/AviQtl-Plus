#include "commands.hpp"
#include "constants.hpp"
#include "core/include/media_utils.hpp"
#include "core/include/rust_core_policy.hpp"
#include "effect_registry.hpp"
#include "rust_keyframe_document.hpp"
#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QDebug>
#include <algorithm>
#include <cstdint>
#include <vector>

extern "C" {
#include <libavutil/avutil.h>
}

namespace AviQtl::UI {

namespace {

double sceneFpsForClip(const TimelineService *timeline, const ClipData &clip) {
    if (timeline == nullptr) {
        return AviQtl::kDefaultFps;
    }
    for (const auto &scene : timeline->getAllScenes()) {
        if (scene.id == clip.sceneId) {
            return scene.fps > 0.0 ? scene.fps : AviQtl::kDefaultFps;
        }
    }
    return AviQtl::kDefaultFps;
}

bool autoAdjustAudioClipDuration(TimelineService *timeline, ClipData &clip, EffectModel *effect, const QString &paramName) {
    if (timeline == nullptr || effect == nullptr || clip.type != QLatin1String("audio") || effect->id() != QLatin1String("audio")) {
        return false;
    }

    if (!AviQtl::RustCore::Policy::audioParameterAffectsDuration(paramName)) {
        return false;
    }

    const QVariantMap params = effect->params();
    const QString source = params.value(QStringLiteral("source")).toString();
    const double totalSec = AviQtl::Core::MediaUtils::mediaDurationSeconds(source, AVMEDIA_TYPE_AUDIO);
    if (totalSec <= 0.0) {
        return false;
    }

    const double fps = sceneFpsForClip(timeline, clip);
    const bool directMode = AviQtl::Core::MediaUtils::isDirectAudioMode(
        params.value(QStringLiteral("playMode")).toString());
    const bool linkedVideo = AviQtl::Core::MediaUtils::isVideoFile(source) &&
                             params.value(QStringLiteral("linkedVideo"), false).toBool();
    const double speed = linkedVideo
                             ? AviQtl::kDefaultSpeed
                             : params.value(QStringLiteral("speed"), AviQtl::kDefaultSpeed)
                                   .toDouble();
    const int newDuration = AviQtl::RustCore::Policy::audioDurationFrames(
        totalSec, directMode, params.value(QStringLiteral("startTime"), 0.0).toDouble(), speed,
        fps);
    if (newDuration <= 0) {
        return false;
    }
    if (newDuration == clip.durationFrames) {
        return false;
    }

    clip.durationFrames = newDuration;
    for (auto *clipEffect : std::as_const(clip.effects)) {
        if (clipEffect != nullptr) {
            clipEffect->syncTrackEndpoints(newDuration);
        }
    }
    return true;
}

bool affectsAudioWaveform(const ClipData &clip, const EffectModel *effect, const QString &paramName) {
    if (effect == nullptr || clip.type != QLatin1String("audio") || effect->id() != QLatin1String("audio")) {
        return false;
    }

    return AviQtl::RustCore::Policy::audioParameterAffectsWaveform(paramName);
}

} // namespace

void TimelineService::addEffect(int clipId, const QString &effectId) {
    auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(effectId);
    if (meta.id.isEmpty()) {
        return;
    }

    m_undoStack->push(new AddEffectCommand(this, clipId, effectId, meta.name));
}

void TimelineService::addEffectInternal(int clipId, const QString &effectId) {
    auto *clip = findClipById(clipId);
    if (clip != nullptr) {
        auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(effectId);
        auto *model = new EffectModel(meta.id, meta.name, meta.kind, meta.categories, meta.defaultParams, meta.qmlSource, meta.uiDefinition, this);
        model->syncTrackEndpoints(clip->durationFrames);
        clip->effects.append(model);
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected effect insertion";
            return;
        }
        emit clipsChanged();
        emit clipEffectsChanged(clipId);
    }
}

void TimelineService::restoreEffectInternal(int clipId, const QVariantMap &data) {
    auto *clip = findClipById(clipId);
    if (clip != nullptr) {
        auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(data.value(QStringLiteral("id")).toString());
        auto *model = new EffectModel(data.value(QStringLiteral("id")).toString(), data.value(QStringLiteral("name")).toString(), meta.kind, meta.categories, data.value(QStringLiteral("params")).toMap(), data.value(QStringLiteral("qmlSource")).toString(),
                                      data.value(QStringLiteral("uiDefinition")).toMap(), this);
        model->setEnabled(data.value(QStringLiteral("enabled")).toBool());
        model->setKeyframeTracks(data.value(QStringLiteral("keyframes")).toMap());
        clip->effects.append(model);
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected effect restoration";
            return;
        }
        emit clipsChanged();
        emit clipEffectsChanged(clipId);
    }
}

void TimelineService::removeEffect(int clipId, int effectIndex) {
    QVariantMap removedData;
    const auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }

    int idx = (effectIndex == -1) ? static_cast<int>(clip->effects.size()) - 1 : effectIndex;
    if (idx >= 0 && idx < clip->effects.size()) {
        auto *eff = clip->effects.value(idx);
        removedData.insert(QStringLiteral("id"), eff->id());
        removedData.insert(QStringLiteral("name"), eff->name());
        removedData.insert(QStringLiteral("enabled"), eff->isEnabled());
        removedData.insert(QStringLiteral("params"), eff->params());
        removedData.insert(QStringLiteral("qmlSource"), eff->qmlSource());
        removedData.insert(QStringLiteral("uiDefinition"), eff->uiDefinition());
        removedData.insert(QStringLiteral("keyframes"), eff->keyframeTracks());

        auto *cmd = new RemoveEffectCommand(this, clipId, effectIndex, eff->name());
        cmd->setRemovedEffect(removedData);
        m_undoStack->push(cmd);
    }
}

void TimelineService::removeEffectInternal(int clipId, int effectIndex) { // NOLINT(bugprone-easily-swappable-parameters)
    for (auto &clip : clipsMutable()) {
        if (clip.id == clipId) {
            if (effectIndex == -1) {
                effectIndex = static_cast<int>(clip.effects.size()) - 1;
            }
            if (effectIndex >= 0 && effectIndex < clip.effects.size()) {
                if (effectIndex == 0 && clip.effects.value(0)->id() == QStringLiteral("transform")) {
                    return;
                }
                auto *eff = clip.effects.takeAt(effectIndex);
                eff->deleteLater();
                if (!commitTimelineProjection()) {
                    qWarning() << "Rust rejected effect removal";
                    return;
                }
                emit clipsChanged();
                emit clipEffectsChanged(clipId);
            }
            break;
        }
    }
}

void TimelineService::removeMultipleEffects(int clipId, const QList<int> &indices) {
    const auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }

    std::vector<std::int32_t> requested;
    requested.reserve(static_cast<std::size_t>(indices.size()));
    for (int idx : indices) {
        requested.push_back(idx);
    }
    const int minimumIndex = !clip->effects.isEmpty() &&
                                     clip->effects.first()->id() == QLatin1String("transform")
                                 ? 1
                                 : 0;
    std::vector<std::int32_t> normalized;
    if (AviQtl::RustCore::normalizeRemovalIndices(
            static_cast<std::size_t>(clip->effects.size()), requested, minimumIndex, normalized) !=
            AviQtl::RustCore::TimelineDomainStatus::Ok ||
        normalized.empty()) {
        return;
    }
    const QList<int> sorted(normalized.cbegin(), normalized.cend());

    auto *cmd = new RemoveMultipleEffectsCommand(this, clipId, sorted, QObject::tr("エフェクト削除 (%1件)").arg(sorted.size()));
    m_undoStack->push(cmd);
}

void TimelineService::removeMultipleEffectsInternal(int clipId, const QList<int> &sortedDescIndices, QList<QVariantMap> *outData) {
    for (auto &clip : clipsMutable()) {
        if (clip.id == clipId) {
            if (outData != nullptr) {
                outData->clear();
            }
            for (int idx : sortedDescIndices) {
                if (idx < 0 || idx >= static_cast<int>(clip.effects.size())) {
                    continue;
                }
                if (idx == 0 && clip.effects.value(0)->id() == QStringLiteral("transform")) {
                    continue;
                }
                auto *eff = clip.effects.takeAt(idx);
                if (outData != nullptr) {
                    QVariantMap d;
                    d.insert(QStringLiteral("id"), eff->id());
                    d.insert(QStringLiteral("name"), eff->name());
                    d.insert(QStringLiteral("enabled"), eff->isEnabled());
                    d.insert(QStringLiteral("params"), eff->params());
                    d.insert(QStringLiteral("qmlSource"), eff->qmlSource());
                    d.insert(QStringLiteral("uiDefinition"), eff->uiDefinition());
                    d.insert(QStringLiteral("keyframes"), eff->keyframeTracks());
                    outData->prepend(d);
                }
                eff->deleteLater();
            }
            if (!commitTimelineProjection()) {
                qWarning() << "Rust rejected multiple effect removal";
                return;
            }
            emit clipsChanged();
            emit clipEffectsChanged(clipId);
            break;
        }
    }
}

void TimelineService::restoreMultipleEffectsInternal(int clipId, const QList<QVariantMap> &ascData) {
    for (auto &clip : clipsMutable()) {
        if (clip.id == clipId) {
            for (const auto &d : ascData) {
                auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(d.value(QStringLiteral("id")).toString());
                auto *model = new EffectModel(d.value(QStringLiteral("id")).toString(), d.value(QStringLiteral("name")).toString(), meta.kind, meta.categories, d.value(QStringLiteral("params")).toMap(), d.value(QStringLiteral("qmlSource")).toString(),
                                              d.value(QStringLiteral("uiDefinition")).toMap(), this);
                model->setEnabled(d.value(QStringLiteral("enabled")).toBool());
                model->setKeyframeTracks(d.value(QStringLiteral("keyframes")).toMap());
                clip.effects.append(model);
            }
            if (!commitTimelineProjection()) {
                qWarning() << "Rust rejected multiple effect restoration";
                return;
            }
            emit clipsChanged();
            emit clipEffectsChanged(clipId);
            break;
        }
    }
}

void TimelineService::setEffectEnabled(int clipId, int effectIndex, bool enabled) { m_undoStack->push(new SetEffectEnabledCommand(this, clipId, effectIndex, enabled)); }

void TimelineService::setAudioPluginEnabled(int clipId, int index, bool enabled) { m_undoStack->push(new SetAudioPluginEnabledCommand(this, clipId, index, enabled)); }

void TimelineService::addAudioPlugin(int clipId, const AudioPluginState &state, const QString &pluginName) {
    m_undoStack->push(new AddAudioPluginCommand(this, clipId, state, pluginName));
}

void TimelineService::removeAudioPlugin(int clipId, int index, const QString &pluginName) {
    m_undoStack->push(new RemoveAudioPluginCommand(this, clipId, index, pluginName));
}

void TimelineService::reorderEffects(int clipId, int oldIndex, int newIndex) {
    if (oldIndex == newIndex) {
        return;
    }
    const auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }
    const int minimumIndex = !clip->effects.isEmpty() &&
                                     clip->effects.first()->id() == QLatin1String("transform")
                                 ? 1
                                 : 0;
    std::vector<std::int32_t> redo;
    std::vector<std::int32_t> undo;
    if (AviQtl::RustCore::planIndexMove(
            static_cast<std::size_t>(clip->effects.size()), oldIndex, newIndex, minimumIndex,
            redo, undo) != AviQtl::RustCore::TimelineDomainStatus::Ok) {
        return;
    }
    m_undoStack->push(new ReorderMultipleEffectsCommand(
        this, clipId, QList<int>(redo.cbegin(), redo.cend()),
        QList<int>(undo.cbegin(), undo.cend()), QObject::tr("エフェクト順序変更")));
}

void TimelineService::reorderMultipleEffects(int clipId, const QVariantList &indicesList, int targetIndex) {
    const auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }

    const std::size_t length = static_cast<std::size_t>(clip->effects.size());
    std::vector<std::int32_t> indices;
    indices.reserve(static_cast<std::size_t>(indicesList.size()));
    for (const QVariant &v : indicesList) {
        bool ok = false;
        const int val = v.toInt(&ok);
        if (ok) {
            indices.push_back(val);
        }
    }
    const int minimumIndex = !clip->effects.isEmpty() &&
                                     clip->effects.first()->id() == QLatin1String("transform")
                                 ? 1
                                 : 0;
    std::vector<std::int32_t> redo;
    std::vector<std::int32_t> undo;
    std::size_t selectedCount = 0;
    if (AviQtl::RustCore::planMultiReorder(
            length, indices, targetIndex, minimumIndex, redo, undo, selectedCount) !=
        AviQtl::RustCore::TimelineDomainStatus::Ok) {
        return;
    }
    bool unchanged = true;
    for (std::size_t index = 0; index < redo.size(); ++index) {
        if (redo[index] != static_cast<std::int32_t>(index)) {
            unchanged = false;
            break;
        }
    }
    if (unchanged) {
        return;
    }
    m_undoStack->push(new ReorderMultipleEffectsCommand(
        this, clipId, QList<int>(redo.cbegin(), redo.cend()),
        QList<int>(undo.cbegin(), undo.cend()),
        QObject::tr("エフェクト順序変更 (%1件)")
            .arg(static_cast<qsizetype>(selectedCount))));
}

void TimelineService::applyPermutationInternal(int clipId, const QList<int> &perm) {
    auto *clip = findClipById(clipId);
    if (clip != nullptr) {
        if (perm.size() != clip->effects.size())
            return;
        QList<EffectModel *> reordered;
        reordered.reserve(perm.size());
        for (int idx : perm)
            reordered.append(clip->effects.at(idx));
        clip->effects = std::move(reordered);
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected effect permutation";
            return;
        }
        emit clipEffectsChanged(clipId);
        emit clipsChanged();
    }
}

void TimelineService::reorderAudioPlugins(int clipId, int oldIndex, int newIndex) {
    if (oldIndex == newIndex) {
        return;
    }
    const auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }
    std::vector<std::int32_t> redo;
    std::vector<std::int32_t> undo;
    if (AviQtl::RustCore::planIndexMove(
            static_cast<std::size_t>(clip->audioPlugins.size()), oldIndex, newIndex, 0, redo,
            undo) != AviQtl::RustCore::TimelineDomainStatus::Ok) {
        return;
    }
    m_undoStack->push(new ReorderAudioPluginCommand(
        this, clipId, QList<int>(redo.cbegin(), redo.cend()),
        QList<int>(undo.cbegin(), undo.cend())));
}

void TimelineService::reorderEffectsInternal(int clipId, int oldIndex, int newIndex) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || oldIndex < 0 || oldIndex >= static_cast<int>(clip->effects.size()) || newIndex < 0 || newIndex >= static_cast<int>(clip->effects.size())) {
        return;
    }

    clip->effects.move(oldIndex, newIndex);
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected effect reorder";
        return;
    }

    // UI更新通知
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::setEffectEnabledInternal(int clipId, int effectIndex, bool enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || effectIndex < 0 || effectIndex >= static_cast<int>(clip->effects.size())) {
        return;
    }

    clip->effects.value(effectIndex)->setEnabled(enabled);
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected effect enabled-state update";
        return;
    }
    emit clipEffectsChanged(clipId);
}

void TimelineService::setAudioPluginEnabledInternal(int clipId, int index, bool enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || index < 0 || index >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    clip->audioPlugins[index].enabled = enabled;
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected audio plugin enabled-state update";
        return;
    }

    emit clipEffectsChanged(clipId);
    emit clipsChanged(); // エンジン側の同期を促す
}

int TimelineService::addAudioPluginStateInternal(int clipId, const AudioPluginState &state) {
    auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return -1;
    }

    clip->audioPlugins.append(state);
    if (!commitTimelineProjection()) {
        clip->audioPlugins.removeLast();
        qWarning() << "Rust rejected audio plugin insertion";
        return -1;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
    return clip->audioPlugins.size() - 1;
}

void TimelineService::removeAudioPluginStateInternal(int clipId, int index) {
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || index < 0 || index >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    const AudioPluginState removed = clip->audioPlugins.takeAt(index);
    if (!commitTimelineProjection()) {
        clip->audioPlugins.insert(index, removed);
        qWarning() << "Rust rejected audio plugin removal";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::restoreAudioPluginStateInternal(int clipId, int index, const AudioPluginState &state) {
    auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }

    if (index < 0 || index > static_cast<int>(clip->audioPlugins.size())) {
        index = clip->audioPlugins.size();
    }
    clip->audioPlugins.insert(index, state);
    if (!commitTimelineProjection()) {
        clip->audioPlugins.removeAt(index);
        qWarning() << "Rust rejected audio plugin restoration";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::setAudioPluginParamInternal(int clipId, int index, int paramIndex, float value) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || index < 0 || index >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    clip->audioPlugins[index].params.insert(QString::number(paramIndex), value);
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected audio plugin parameter update";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::applyAudioPluginPermutationInternal(int clipId, const QList<int> &perm) {
    auto *clip = findClipById(clipId);
    if (clip == nullptr || perm.size() != clip->audioPlugins.size()) {
        return;
    }
    QList<AudioPluginState> reordered;
    reordered.reserve(perm.size());
    for (int index : perm) {
        if (index < 0 || index >= clip->audioPlugins.size()) {
            return;
        }
        reordered.append(clip->audioPlugins.at(index));
    }
    clip->audioPlugins = std::move(reordered);
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected audio plugin permutation";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::pasteEffect(int clipId, int targetIndex) {
    if (!m_effectClipboard) {
        return;
    }
    m_undoStack->push(new PasteEffectCommand(this, clipId, targetIndex, m_effectClipboard.get()));
}

void TimelineService::pasteEffectInternal(int clipId, int targetIndex, EffectModel *effect) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if (clip != nullptr) {
        int idx = std::clamp(targetIndex, 0, static_cast<int>(clip->effects.size()));
        clip->effects.insert(idx, effect->clone());
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected pasted effect";
            return;
        }
        emit clipEffectsChanged(clipId);
        emit clipsChanged();
    }
}

void TimelineService::updateEffectParam(int clipId, int effectIndex, const QString &paramName, const QVariant &value) {
    QVariant oldValue;
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || effectIndex >= static_cast<int>(clip->effects.size())) {
        return;
    }

    const auto *eff = clip->effects.value(effectIndex);
    oldValue = eff->params().value(paramName);

    m_undoStack->push(new UpdateEffectParamCommand(this, clipId, effectIndex, paramName, value, oldValue, eff->name()));
}

void TimelineService::setAudioPluginParam(int clipId, int pluginIndex, int paramIndex, float value) {
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    const auto &plugin = clip->audioPlugins.at(pluginIndex);
    const float oldValue = plugin.params.value(QString::number(paramIndex)).toFloat();

    m_undoStack->push(new SetAudioPluginParamCommand(this, clipId, pluginIndex, paramIndex, value, oldValue, plugin.id));
}

void TimelineService::updateEffectParamInternal(int clipId, int effectIndex, const QString &paramName, const QVariant &value) {
    auto *clip = findClipById(clipId);
    if (clip != nullptr) {
        if (effectIndex >= 0 && effectIndex < static_cast<int>(clip->effects.size())) {
            auto *effect = clip->effects.value(effectIndex);
            effect->setParam(paramName, value);
            const bool durationChanged = autoAdjustAudioClipDuration(this, *clip, effect, paramName);
            const bool waveformChanged = affectsAudioWaveform(*clip, effect, paramName);

            if (!commitTimelineProjection()) {
                qWarning() << "Rust rejected effect parameter update";
                return;
            }

            emit effectParamChanged(clipId, effectIndex, paramName, value);

            // [FIX-21] layerCount の変更は clipsChanged() を発行しない。
            //
            // 旧実装では layerCount パラメータ変更のたびに clipsChanged() を
            // emit していたため、CompositeView の clipModel が全再構築され、
            // CameraControlObject の destroy→recreate サイクルが毎回発生していた。
            // これが SIGSEGV の主要トリガーの一つであった。
            //
            // layerCount の変更はカメラが参照するレイヤー範囲の変更であり、
            // clipModel の再構築は不要。effectParamChanged シグナルだけで
            // CameraControlObject.qml 側の evalParam() がリアクティブに再評価される。
            //
            // path / source / targetSceneId は実際にメディアや構造の変更を伴うため
            // 引き続き clipsChanged() を発行する。
            if (durationChanged || paramName == QLatin1String("path") || paramName == QLatin1String("source") || paramName == QStringLiteral("targetSceneId")) {
                emit clipsChanged();
            }
            if (durationChanged || waveformChanged) {
                emit clipEffectsChanged(clipId);
            }

            if (m_selection->selectedClipId() == clipId) {
                QVariantMap data = m_selection->selectedClipData();
                data.insert(paramName, value);
                if (durationChanged) {
                    data.insert(QStringLiteral("durationFrames"), clip->durationFrames);
                }
                m_selection->refreshSelectionData(clipId, data);
            }
        }
    }
}

void TimelineService::setKeyframe(int clipId, int effectIndex, const QString &paramName, int frame, const QVariant &value, const QVariantMap &options) {
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *eff = clip->effects.value(effectIndex);
    if (eff == nullptr) {
        return;
    }

    bool wasExisting = false;
    QVariant oldValue;
    QVariantMap oldOptions;
    const auto track = eff->keyframeListForUi(paramName);
    for (const auto &v : std::as_const(track)) {
        const auto m = v.toMap();
        if (m.value(QStringLiteral("frame")).toInt() == frame) {
            wasExisting = true;
            oldValue = m.value(QStringLiteral("value"));
            oldOptions = m;
            break;
        }
    }
    m_undoStack->push(new SetKeyframeCommand(this, clipId, effectIndex, paramName, frame, value, options, oldValue, oldOptions, wasExisting));
}

void TimelineService::removeKeyframe(int clipId, int effectIndex, const QString &paramName, int frame) {
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *eff = clip->effects.value(effectIndex);
    if (eff == nullptr) {
        return;
    }

    QVariant savedValue;
    QVariantMap savedOptions;
    bool foundKeyframe = false;
    const auto track = eff->keyframeListForUi(paramName);
    for (const auto &v : std::as_const(track)) {
        const auto m = v.toMap();
        if (m.value(QStringLiteral("frame")).toInt() == frame) {
            if (eff->isEndpointFrame(paramName, frame)) {
                return;
            }
            savedValue = m.value(QStringLiteral("value"));
            savedOptions = m;
            foundKeyframe = true;
            break;
        }
    }
    if (!foundKeyframe) {
        return;
    }
    m_undoStack->push(new RemoveKeyframeCommand(this, clipId, effectIndex, paramName, frame, savedValue, savedOptions));
}

void TimelineService::moveKeyframe(int clipId, int effectIndex, const QString &paramName, int oldFrame, int newFrame) {
    if (oldFrame == newFrame || newFrame <= 0) {
        return;
    }
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *eff = clip->effects.value(effectIndex);
    if (eff == nullptr) {
        return;
    }
    bool foundSource = false;
    const auto track = eff->keyframeListForUi(paramName);
    for (const auto &v : std::as_const(track)) {
        const int frame = v.toMap().value(QStringLiteral("frame")).toInt();
        if (frame == oldFrame && !eff->isEndpointFrame(paramName, oldFrame)) {
            foundSource = true;
        }
        if (frame == newFrame) {
            return;
        }
    }
    if (!foundSource) {
        return;
    }

    m_undoStack->push(new MoveKeyframeCommand(this, clipId, effectIndex, paramName, oldFrame, newFrame));
}

void TimelineService::setKeyframeInternal(int clipId, int effectIndex, const QString &paramName, int frame, const QVariant &value, const QVariantMap &options) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip != nullptr) && effectIndex < clip->effects.size()) {
        clip->effects.value(effectIndex)->setKeyframe(paramName, frame, value, options);
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected effect keyframe update";
            return;
        }

        // ECSエンジンの更新を促す
        emit effectParamChanged(clipId, effectIndex, paramName, value);

        // [FIX-22] キーフレーム設定でも layerCount は clipsChanged() を発行しない。
        // effectParamChanged でエンジン側は更新される。
        if (paramName == QLatin1String("path") || paramName == QLatin1String("source") || paramName == QStringLiteral("targetSceneId")) {
            emit clipsChanged();
        }
    }
}

void TimelineService::removeKeyframeInternal(int clipId, int effectIndex, const QString &paramName, int frame) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip != nullptr) && effectIndex < clip->effects.size()) {
        clip->effects.value(effectIndex)->removeKeyframe(paramName, frame);
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected effect keyframe removal";
            return;
        }

        emit effectParamChanged(clipId, effectIndex, paramName, QVariant());
        if (paramName == QLatin1String("path") || paramName == QLatin1String("source") || paramName == QStringLiteral("targetSceneId")) {
            emit clipsChanged();
        }
    }
}

void TimelineService::moveKeyframeInternal(int clipId, int effectIndex, const QString &paramName, int oldFrame, int newFrame) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip != nullptr) && effectIndex >= 0 && effectIndex < clip->effects.size()) {
        if (!clip->effects.value(effectIndex)->moveKeyframe(paramName, oldFrame, newFrame)) {
            return;
        }
        if (!commitTimelineProjection()) {
            qWarning() << "Rust rejected effect keyframe move";
            return;
        }

        emit effectParamChanged(clipId, effectIndex, paramName, QVariant());
        if (paramName == QLatin1String("path") || paramName == QLatin1String("source") || paramName == QStringLiteral("targetSceneId")) {
            emit clipsChanged();
        }
    }
}

void TimelineService::setAudioPluginKeyframe(int clipId, int pluginIndex, const QString &paramKey, int frame, const QVariant &value, const QVariantMap &options) {
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);

    bool wasExisting = false;
    QVariant oldValue;
    QVariantMap oldOptions;
    const QVariant fallback = plugin.params.value(paramKey);
    const auto inspected = AviQtl::Core::RustKeyframeDocument::inspect(
        plugin.keyframeTracks.value(paramKey), fallback);
    if (!inspected) {
        return;
    }
    for (const auto &v : inspected->flat) {
        const auto m = v.toMap();
        if (m.value(QStringLiteral("frame")).toInt() == frame) {
            wasExisting = true;
            oldValue = m.value(QStringLiteral("value"));
            oldOptions = m;
            break;
        }
    }
    m_undoStack->push(new SetAudioPluginKeyframeCommand(this, clipId, pluginIndex, paramKey, frame, value, options, oldValue, oldOptions, wasExisting));
}

void TimelineService::removeAudioPluginKeyframe(int clipId, int pluginIndex, const QString &paramKey, int frame) {
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);

    QVariant savedValue;
    QVariantMap savedOptions;
    bool foundKeyframe = false;
    const QVariant fallback = plugin.params.value(paramKey);
    const auto inspected = AviQtl::Core::RustKeyframeDocument::inspect(
        plugin.keyframeTracks.value(paramKey), fallback);
    if (!inspected) {
        return;
    }
    for (const auto &v : inspected->flat) {
        const auto m = v.toMap();
        if (m.value(QStringLiteral("frame")).toInt() == frame) {
            const int startFrame = inspected->track.value(QStringLiteral("start")).toMap().value(QStringLiteral("frame")).toInt();
            if (frame == startFrame) {
                return;
            }
            savedValue = m.value(QStringLiteral("value"));
            savedOptions = m;
            foundKeyframe = true;
            break;
        }
    }
    if (!foundKeyframe) {
        return;
    }
    m_undoStack->push(new RemoveAudioPluginKeyframeCommand(this, clipId, pluginIndex, paramKey, frame, savedValue, savedOptions));
}

void TimelineService::moveAudioPluginKeyframe(int clipId, int pluginIndex, const QString &paramKey, int oldFrame, int newFrame) {
    if (oldFrame == newFrame || newFrame <= 0) {
        return;
    }
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);

    bool foundSource = false;
    const QVariant fallback = plugin.params.value(paramKey);
    const auto inspected = AviQtl::Core::RustKeyframeDocument::inspect(
        plugin.keyframeTracks.value(paramKey), fallback);
    if (!inspected) {
        return;
    }
    const int startFrame = inspected->track.value(QStringLiteral("start")).toMap().value(QStringLiteral("frame")).toInt();
    for (const auto &v : inspected->flat) {
        const int frame = v.toMap().value(QStringLiteral("frame")).toInt();
        if (frame == oldFrame && frame != startFrame) {
            foundSource = true;
        }
        if (frame == newFrame) {
            return;
        }
    }
    if (!foundSource) {
        return;
    }
    m_undoStack->push(new MoveAudioPluginKeyframeCommand(this, clipId, pluginIndex, paramKey, oldFrame, newFrame));
}

void TimelineService::setAudioPluginKeyframeInternal(int clipId, int pluginIndex, const QString &paramKey, int frame, const QVariant &value, const QVariantMap &options) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    auto &plugin = clip->audioPlugins[pluginIndex];
    const QVariant fallback = plugin.params.value(paramKey);
    const auto result = AviQtl::Core::RustKeyframeDocument::set(
        plugin.keyframeTracks.value(paramKey), fallback, 0, frame, value, options);
    if (!result || !result->accepted) {
        return;
    }
    if (result->baseValue) {
        plugin.params[paramKey] = *result->baseValue;
    }
    plugin.keyframeTracks[paramKey] = result->track;
    plugin.invalidateKeyframeCache();
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected audio plugin keyframe update";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::removeAudioPluginKeyframeInternal(int clipId, int pluginIndex, const QString &paramKey, int frame) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    auto &plugin = clip->audioPlugins[pluginIndex];
    const QVariant fallback = plugin.params.value(paramKey);
    const auto result = AviQtl::Core::RustKeyframeDocument::remove(
        plugin.keyframeTracks.value(paramKey), fallback, 0, frame);
    if (!result || !result->accepted) {
        return;
    }
    plugin.keyframeTracks[paramKey] = result->track;
    plugin.invalidateKeyframeCache();
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected audio plugin keyframe removal";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::moveAudioPluginKeyframeInternal(int clipId, int pluginIndex, const QString &paramKey, int oldFrame, int newFrame) { // NOLINT(bugprone-easily-swappable-parameters)
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    auto &plugin = clip->audioPlugins[pluginIndex];
    const QVariant fallback = plugin.params.value(paramKey);
    const auto result = AviQtl::Core::RustKeyframeDocument::move(
        plugin.keyframeTracks.value(paramKey), fallback, 0, oldFrame, newFrame);
    if (!result || !result->accepted) {
        return;
    }
    plugin.keyframeTracks[paramKey] = result->track;
    plugin.invalidateKeyframeCache();
    if (!commitTimelineProjection()) {
        qWarning() << "Rust rejected audio plugin keyframe move";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

} // namespace AviQtl::UI
