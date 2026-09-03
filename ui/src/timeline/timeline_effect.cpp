#include "commands.hpp"
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
#include <optional>
#include <vector>

extern "C" {
#include <libavutil/avutil.h>
}

namespace AviQtl::UI {

namespace {

std::optional<double> probeAudioDuration(const ClipData &clip, const EffectModel *effect,
                                         const QString &paramName, const QVariant &value) {
    if (effect == nullptr || clip.type != QLatin1String("audio") ||
        effect->id() != QLatin1String("audio")) {
        return std::nullopt;
    }

    if (!AviQtl::RustCore::Policy::audioParameterAffectsDuration(paramName)) {
        return std::nullopt;
    }

    QVariantMap params = effect->params();
    params.insert(paramName, value);
    const QString source = params.value(QStringLiteral("source")).toString();
    const double totalSec = AviQtl::Core::MediaUtils::mediaDurationSeconds(source, AVMEDIA_TYPE_AUDIO);
    return totalSec > 0.0 ? std::optional<double>(totalSec) : std::nullopt;
}

bool affectsAudioWaveform(const ClipData &clip, const EffectModel *effect, const QString &paramName) {
    if (effect == nullptr || clip.type != QLatin1String("audio") || effect->id() != QLatin1String("audio")) {
        return false;
    }

    return AviQtl::RustCore::Policy::audioParameterAffectsWaveform(paramName);
}

QVariantMap effectMutationDocument(const EffectModel *effect) {
    if (effect == nullptr) {
        return {};
    }
    return {
        {QStringLiteral("id"), effect->id()},
        {QStringLiteral("name"), effect->name()},
        {QStringLiteral("enabled"), effect->isEnabled()},
        {QStringLiteral("params"), effect->params()},
        {QStringLiteral("keyframes"), effect->keyframeTracks()},
    };
}

QVariantMap audioPluginMutationDocument(const AudioPluginState &plugin) {
    QVariantMap document{
        {QStringLiteral("id"), plugin.id},
        {QStringLiteral("enabled"), plugin.enabled},
        {QStringLiteral("params"), plugin.params},
    };
    if (!plugin.keyframeTracks.isEmpty()) {
        document.insert(QStringLiteral("keyframes"), plugin.keyframeTracks);
    }
    return document;
}

QVariantMap clipDocumentAt(const TimelineService *timeline, int clipId) {
    if (timeline == nullptr) {
        return {};
    }
    const QVariantList clips =
        timeline->timelineStateSnapshot().value(QStringLiteral("clips")).toList();
    for (const QVariant &clipValue : clips) {
        const QVariantMap clip = clipValue.toMap();
        if (clip.value(QStringLiteral("id")).toInt() == clipId) {
            return clip;
        }
    }
    return {};
}

QVariantMap restoredEffectDocument(const EffectModel *effect, const QVariantMap &document) {
    return document.isEmpty() ? effectMutationDocument(effect) : document;
}

QVariantMap effectDocumentAt(const TimelineService *timeline, int clipId, int effectIndex) {
    if (timeline == nullptr || effectIndex < 0) {
        return {};
    }
    const QVariantList effects =
        clipDocumentAt(timeline, clipId).value(QStringLiteral("effects")).toList();
    return effectIndex < effects.size() ? effects.at(effectIndex).toMap() : QVariantMap{};
}

QVariantMap audioPluginDocumentAt(const TimelineService *timeline, int clipId, int pluginIndex) {
    if (timeline == nullptr || pluginIndex < 0) {
        return {};
    }
    const QVariantList plugins =
        clipDocumentAt(timeline, clipId).value(QStringLiteral("audioPlugins")).toList();
    return pluginIndex < plugins.size() ? plugins.at(pluginIndex).toMap() : QVariantMap{};
}

QVariantMap effectRuntimeData(const EffectModel *effect) {
    if (effect == nullptr) {
        return {};
    }
    return {
        {QStringLiteral("id"), effect->id()},
        {QStringLiteral("name"), effect->name()},
        {QStringLiteral("enabled"), effect->isEnabled()},
        {QStringLiteral("params"), effect->params()},
        {QStringLiteral("qmlSource"), effect->qmlSource()},
        {QStringLiteral("uiDefinition"), effect->uiDefinition()},
        {QStringLiteral("keyframes"), effect->keyframeTracks()},
    };
}

struct RestoredEffect {
    EffectModel *model;
    QVariantMap document;
};

RestoredEffect restoreEffectModel(const QVariantMap &payload, QObject *parent) {
    const QVariantMap runtime = payload.value(QStringLiteral("runtime")).toMap().isEmpty()
                                    ? payload
                                    : payload.value(QStringLiteral("runtime")).toMap();
    const QVariantMap document = payload.value(QStringLiteral("document")).toMap();
    const auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(
        runtime.value(QStringLiteral("id")).toString());
    auto *model = new EffectModel(
        runtime.value(QStringLiteral("id")).toString(),
        runtime.value(QStringLiteral("name")).toString(), meta.kind, meta.categories,
        runtime.value(QStringLiteral("params")).toMap(),
        runtime.value(QStringLiteral("qmlSource")).toString(),
        runtime.value(QStringLiteral("uiDefinition")).toMap(), parent);
    model->setEnabled(runtime.value(QStringLiteral("enabled")).toBool());
    model->setKeyframeTracks(runtime.value(QStringLiteral("keyframes")).toMap());
    return {model, document};
}

QVariantMap effectRestoreData(int index, const EffectModel *effect,
                              const QVariantMap &document) {
    return {
        {QStringLiteral("index"), index},
        {QStringLiteral("runtime"), effectRuntimeData(effect)},
        {QStringLiteral("document"), document},
    };
}

QVariantMap insertEffectsRequest(int clipId, const QVariantList &insertions) {
    return {
        {QStringLiteral("operation"), QStringLiteral("insert_effects")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("insertions"), insertions},
    };
}

QVariantMap removeEffectsRequest(int clipId, const QVariantList &indices) {
    return {
        {QStringLiteral("operation"), QStringLiteral("remove_effects")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("effect_indices"), indices},
    };
}

QVariantMap reorderEffectsRequest(int clipId, const QVariantList &permutation) {
    return {
        {QStringLiteral("operation"), QStringLiteral("reorder_effects")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("permutation"), permutation},
    };
}

QVariantMap setEffectParameterRequest(int clipId, int effectIndex, const QString &paramName,
                                      const QVariant &value,
                                      const std::optional<double> &mediaDurationSeconds) {
    QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("set_effect_parameter")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("effect_index"), effectIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("value"), value},
    };
    if (mediaDurationSeconds) {
        request.insert(QStringLiteral("media_duration_seconds"), *mediaDurationSeconds);
    }
    return request;
}

QVariantMap setEffectKeyframeRequest(int clipId, int effectIndex, const QString &paramName,
                                     int frame, const QVariant &value,
                                     const QVariantMap &options) {
    return {
        {QStringLiteral("operation"), QStringLiteral("set_effect_keyframe")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("effect_index"), effectIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("frame"), frame},
        {QStringLiteral("value"), value},
        {QStringLiteral("options"), options},
    };
}

QVariantMap removeEffectKeyframeRequest(int clipId, int effectIndex,
                                        const QString &paramName, int frame) {
    return {
        {QStringLiteral("operation"), QStringLiteral("remove_effect_keyframe")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("effect_index"), effectIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("frame"), frame},
    };
}

QVariantMap moveEffectKeyframeRequest(int clipId, int effectIndex, const QString &paramName,
                                      int oldFrame, int newFrame) {
    return {
        {QStringLiteral("operation"), QStringLiteral("move_effect_keyframe")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("effect_index"), effectIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("old_frame"), oldFrame},
        {QStringLiteral("new_frame"), newFrame},
    };
}

QVariantMap insertAudioPluginRequest(int clipId, int index, const QVariantMap &plugin) {
    return {
        {QStringLiteral("operation"), QStringLiteral("insert_audio_plugin")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("index"), index},
        {QStringLiteral("plugin"), plugin},
    };
}

QVariantMap removeAudioPluginRequest(int clipId, int pluginIndex) {
    return {
        {QStringLiteral("operation"), QStringLiteral("remove_audio_plugin")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("plugin_index"), pluginIndex},
    };
}

QVariantMap reorderAudioPluginsRequest(int clipId, const QVariantList &permutation) {
    return {
        {QStringLiteral("operation"), QStringLiteral("reorder_audio_plugins")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("permutation"), permutation},
    };
}

QVariantMap setAudioPluginEnabledRequest(int clipId, int pluginIndex, bool enabled) {
    return {
        {QStringLiteral("operation"), QStringLiteral("set_audio_plugin_enabled")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("plugin_index"), pluginIndex},
        {QStringLiteral("enabled"), enabled},
    };
}

QVariantMap setAudioPluginParameterRequest(int clipId, int pluginIndex,
                                           const QString &paramName, const QVariant &value) {
    return {
        {QStringLiteral("operation"), QStringLiteral("set_audio_plugin_parameter")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("plugin_index"), pluginIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("value"), value},
    };
}

QVariantMap setAudioPluginKeyframeRequest(int clipId, int pluginIndex,
                                          const QString &paramName, int frame,
                                          const QVariant &value,
                                          const QVariantMap &options) {
    return {
        {QStringLiteral("operation"), QStringLiteral("set_audio_plugin_keyframe")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("plugin_index"), pluginIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("frame"), frame},
        {QStringLiteral("value"), value},
        {QStringLiteral("options"), options},
    };
}

QVariantMap removeAudioPluginKeyframeRequest(int clipId, int pluginIndex,
                                             const QString &paramName, int frame) {
    return {
        {QStringLiteral("operation"), QStringLiteral("remove_audio_plugin_keyframe")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("plugin_index"), pluginIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("frame"), frame},
    };
}

QVariantMap moveAudioPluginKeyframeRequest(int clipId, int pluginIndex,
                                           const QString &paramName, int oldFrame,
                                           int newFrame) {
    return {
        {QStringLiteral("operation"), QStringLiteral("move_audio_plugin_keyframe")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("plugin_index"), pluginIndex},
        {QStringLiteral("param_name"), paramName},
        {QStringLiteral("old_frame"), oldFrame},
        {QStringLiteral("new_frame"), newFrame},
    };
}

void deleteDetachedEffects(const TimelineService *timeline,
                           const QList<EffectModel *> &effects) {
    for (auto *effect : effects) {
        const bool retained = std::ranges::any_of(
            timeline->getAllScenes(), [effect](const SceneData &scene) {
                return std::ranges::any_of(scene.clips, [effect](const ClipData &clip) {
                    return clip.effects.contains(effect);
                });
            });
        if (effect != nullptr && !retained) {
            effect->deleteLater();
        }
    }
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
        const qsizetype insertedIndex = clip->effects.size();
        const QVariantMap request = insertEffectsRequest(
            clipId,
            {QVariantMap{{QStringLiteral("index"), insertedIndex},
                         {QStringLiteral("effect"), effectMutationDocument(model)}}});
        if (!commitTimelineStructureMutation(
                request,
                [this, clipId, insertedIndex, model]() {
                    auto *projected = findClipById(clipId);
                    if (projected == nullptr || insertedIndex > projected->effects.size() ||
                        projected->effects.contains(model)) {
                        return false;
                    }
                    projected->effects.insert(insertedIndex, model);
                    return true;
                },
                [this, clipId, model]() {
                    auto *projected = findClipById(clipId);
                    return projected != nullptr && projected->effects.removeOne(model);
                },
                {},
                [this, model]() { deleteDetachedEffects(this, {model}); })) {
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
        const int index = std::clamp(data.value(QStringLiteral("index"), clip->effects.size()).toInt(),
                                     0, static_cast<int>(clip->effects.size()));
        const RestoredEffect restoredEffect = restoreEffectModel(data, this);
        auto *model = restoredEffect.model;
        const QVariantMap request = insertEffectsRequest(
            clipId,
            {QVariantMap{{QStringLiteral("index"), index},
                         {QStringLiteral("effect"),
                          restoredEffectDocument(model, restoredEffect.document)}}});
        if (!commitTimelineStructureMutation(
                request,
                [this, clipId, index, model]() {
                    auto *projected = findClipById(clipId);
                    if (projected == nullptr || index > projected->effects.size() ||
                        projected->effects.contains(model)) {
                        return false;
                    }
                    projected->effects.insert(index, model);
                    return true;
                },
                [this, clipId, model]() {
                    auto *projected = findClipById(clipId);
                    return projected != nullptr && projected->effects.removeOne(model);
                },
                {},
                [this, model]() { deleteDetachedEffects(this, {model}); })) {
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
        removedData = effectRestoreData(idx, eff, effectDocumentAt(this, clipId, idx));

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
                auto *eff = clip.effects.at(effectIndex);
                const QVariantMap request =
                    removeEffectsRequest(clipId, QVariantList{effectIndex});
                if (!commitTimelineStructureMutation(
                        request,
                        [this, clipId, effectIndex, eff]() {
                            auto *projected = findClipById(clipId);
                            if (projected == nullptr || effectIndex >= projected->effects.size() ||
                                projected->effects.at(effectIndex) != eff) {
                                return false;
                            }
                            projected->effects.removeAt(effectIndex);
                            return true;
                        },
                        [this, clipId, effectIndex, eff]() {
                            auto *projected = findClipById(clipId);
                            if (projected == nullptr || projected->effects.contains(eff)) {
                                return false;
                            }
                            projected->effects.insert(
                                std::min<qsizetype>(effectIndex, projected->effects.size()), eff);
                            return true;
                        },
                        [eff]() { eff->deleteLater(); })) {
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
            QList<std::pair<int, EffectModel *>> removedEffects;
            for (int idx : sortedDescIndices) {
                if (idx < 0 || idx >= static_cast<int>(clip.effects.size())) {
                    continue;
                }
                if (idx == 0 && clip.effects.value(0)->id() == QStringLiteral("transform")) {
                    continue;
                }
                auto *eff = clip.effects.at(idx);
                removedEffects.append({idx, eff});
                if (outData != nullptr) {
                    outData->prepend(
                        effectRestoreData(idx, eff, effectDocumentAt(this, clipId, idx)));
                }
            }
            if (removedEffects.isEmpty()) {
                return;
            }
            QVariantList effectIndices;
            effectIndices.reserve(removedEffects.size());
            for (const auto &[index, effect] : std::as_const(removedEffects)) {
                Q_UNUSED(effect)
                effectIndices.append(index);
            }
            const QVariantMap request = removeEffectsRequest(clipId, effectIndices);
            if (!commitTimelineStructureMutation(
                    request,
                    [this, clipId, removedEffects]() {
                        auto *projected = findClipById(clipId);
                        if (projected == nullptr) {
                            return false;
                        }
                        for (const auto &[index, effect] : removedEffects) {
                            if (index < 0 || index >= projected->effects.size() ||
                                projected->effects.at(index) != effect) {
                                return false;
                            }
                        }
                        for (const auto &[index, effect] : removedEffects) {
                            Q_UNUSED(effect)
                            projected->effects.removeAt(index);
                        }
                        return true;
                    },
                    [this, clipId, removedEffects]() {
                        auto *projected = findClipById(clipId);
                        if (projected == nullptr) {
                            return false;
                        }
                        auto ascending = removedEffects;
                        std::sort(ascending.begin(), ascending.end(), [](const auto &left,
                                                                        const auto &right) {
                            return left.first < right.first;
                        });
                        for (const auto &[index, effect] : ascending) {
                            if (projected->effects.contains(effect)) {
                                return false;
                            }
                            projected->effects.insert(
                                std::min<qsizetype>(index, projected->effects.size()), effect);
                        }
                        return true;
                    },
                    [removedEffects]() {
                        for (const auto &[index, effect] : removedEffects) {
                            Q_UNUSED(index)
                            effect->deleteLater();
                        }
                    })) {
                if (outData != nullptr) {
                    outData->clear();
                }
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
            QList<EffectModel *> restoredEffects;
            QList<std::pair<int, EffectModel *>> projectedInsertions;
            QVariantList insertions;
            insertions.reserve(ascData.size());
            int projectedSize = clip.effects.size();
            for (const auto &d : ascData) {
                const int index = std::clamp(
                    d.value(QStringLiteral("index"), projectedSize).toInt(), 0,
                    projectedSize);
                const RestoredEffect restoredEffect = restoreEffectModel(d, this);
                auto *model = restoredEffect.model;
                restoredEffects.append(model);
                projectedInsertions.append({index, model});
                insertions.append(
                    QVariantMap{{QStringLiteral("index"), index},
                                {QStringLiteral("effect"),
                                 restoredEffectDocument(model, restoredEffect.document)}});
                ++projectedSize;
            }
            if (insertions.isEmpty()) {
                return;
            }
            const QVariantMap request = insertEffectsRequest(clipId, insertions);
            if (!commitTimelineStructureMutation(
                    request,
                    [this, clipId, projectedInsertions]() {
                        auto *projected = findClipById(clipId);
                        if (projected == nullptr) {
                            return false;
                        }
                        qsizetype size = projected->effects.size();
                        for (const auto &[index, effect] : projectedInsertions) {
                            if (index < 0 || index > size ||
                                projected->effects.contains(effect)) {
                                return false;
                            }
                            ++size;
                        }
                        for (const auto &[index, effect] : projectedInsertions) {
                            projected->effects.insert(index, effect);
                        }
                        return true;
                    },
                    [this, clipId, restoredEffects]() {
                        auto *projected = findClipById(clipId);
                        if (projected == nullptr) {
                            return false;
                        }
                        bool restored = true;
                        for (auto *effect : restoredEffects) {
                            restored = projected->effects.removeOne(effect) && restored;
                        }
                        return restored;
                    },
                    {},
                    [this, restoredEffects]() {
                        deleteDetachedEffects(this, restoredEffects);
                    })) {
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
    m_undoStack->push(new RemoveAudioPluginCommand(
        this, clipId, index, pluginName, audioPluginDocumentAt(this, clipId, index)));
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
        if (perm.size() != clip->effects.size()) {
            return;
        }
        QList<EffectModel *> reordered;
        reordered.reserve(perm.size());
        for (int idx : perm) {
            if (idx < 0 || idx >= clip->effects.size()) {
                return;
            }
            reordered.append(clip->effects.at(idx));
        }
        const QList<EffectModel *> previous = clip->effects;
        QVariantList permutation;
        permutation.reserve(perm.size());
        for (int index : perm) {
            permutation.append(index);
        }
        const QVariantMap request = reorderEffectsRequest(clipId, permutation);
        if (!commitTimelineStructureMutation(
                request,
                [this, clipId, previous, reordered]() {
                    auto *projected = findClipById(clipId);
                    if (projected == nullptr || projected->effects != previous) {
                        return false;
                    }
                    projected->effects = reordered;
                    return true;
                },
                [this, clipId, previous]() {
                    auto *projected = findClipById(clipId);
                    if (projected == nullptr) {
                        return false;
                    }
                    projected->effects = previous;
                    return true;
                })) {
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

void TimelineService::setEffectEnabledInternal(int clipId, int effectIndex, bool enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || effectIndex < 0 || effectIndex >= static_cast<int>(clip->effects.size())) {
        return;
    }

    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("set_effect_enabled")},
        {QStringLiteral("clip_id"), clipId},
        {QStringLiteral("effect_index"), effectIndex},
        {QStringLiteral("enabled"), enabled},
    };
    if (!commitTimelineStateMutation(request)) {
        qWarning() << "Rust rejected effect enabled-state update";
        return;
    }
    emit clipEffectsChanged(clipId);
}

void TimelineService::setAudioPluginEnabledInternal(int clipId, int index, bool enabled) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || index < 0 || index >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    if (!commitTimelineStateMutation(setAudioPluginEnabledRequest(clipId, index, enabled))) {
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

    const qsizetype insertedIndex = clip->audioPlugins.size();
    if (!commitTimelineStructureMutation(
            insertAudioPluginRequest(clipId, insertedIndex, audioPluginMutationDocument(state)),
            [this, clipId, insertedIndex, state]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr || insertedIndex > projected->audioPlugins.size()) {
                    return false;
                }
                projected->audioPlugins.insert(insertedIndex, state);
                return true;
            },
            [this, clipId, insertedIndex, pluginId = state.id]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr || insertedIndex >= projected->audioPlugins.size() ||
                    projected->audioPlugins.at(insertedIndex).id != pluginId) {
                    return false;
                }
                projected->audioPlugins.removeAt(insertedIndex);
                return true;
            })) {
        qWarning() << "Rust rejected audio plugin insertion";
        return -1;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
    return insertedIndex;
}

void TimelineService::removeAudioPluginStateInternal(int clipId, int index) {
    auto *clip = findClipById(clipId);
    if ((clip == nullptr) || index < 0 || index >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    const AudioPluginState removed = clip->audioPlugins.at(index);
    if (!commitTimelineStructureMutation(
            removeAudioPluginRequest(clipId, index),
            [this, clipId, index, pluginId = removed.id]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr || index >= projected->audioPlugins.size() ||
                    projected->audioPlugins.at(index).id != pluginId) {
                    return false;
                }
                projected->audioPlugins.removeAt(index);
                return true;
            },
            [this, clipId, index, removed]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr) {
                    return false;
                }
                projected->audioPlugins.insert(
                    std::min<qsizetype>(index, projected->audioPlugins.size()), removed);
                return true;
            })) {
        qWarning() << "Rust rejected audio plugin removal";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::restoreAudioPluginStateInternal(int clipId, int index,
                                                      const AudioPluginState &state,
                                                      const QVariantMap &document) {
    auto *clip = findClipById(clipId);
    if (clip == nullptr) {
        return;
    }

    if (index < 0 || index > static_cast<int>(clip->audioPlugins.size())) {
        index = clip->audioPlugins.size();
    }
    const QVariantMap restoredDocument =
        document.isEmpty() ? audioPluginMutationDocument(state) : document;
    if (!commitTimelineStructureMutation(
            insertAudioPluginRequest(clipId, index, restoredDocument),
            [this, clipId, index, state]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr || index > projected->audioPlugins.size()) {
                    return false;
                }
                projected->audioPlugins.insert(index, state);
                return true;
            },
            [this, clipId, index, pluginId = state.id]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr || index >= projected->audioPlugins.size() ||
                    projected->audioPlugins.at(index).id != pluginId) {
                    return false;
                }
                projected->audioPlugins.removeAt(index);
                return true;
            })) {
        qWarning() << "Rust rejected audio plugin restoration";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::setAudioPluginParamInternal(int clipId, int index, int paramIndex, float value) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || index < 0 || index >= static_cast<int>(clip->audioPlugins.size())) {
        return;
    }

    const QString key = QString::number(paramIndex);
    if (!commitTimelineStateMutation(
            setAudioPluginParameterRequest(clipId, index, key, value))) {
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
    const QList<AudioPluginState> previous = clip->audioPlugins;
    QVariantList permutation;
    permutation.reserve(perm.size());
    for (int index : perm) {
        permutation.append(index);
    }
    if (!commitTimelineStructureMutation(
            reorderAudioPluginsRequest(clipId, permutation),
            [this, clipId, previous, reordered]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr ||
                    projected->audioPlugins.size() != previous.size()) {
                    return false;
                }
                for (qsizetype index = 0; index < previous.size(); ++index) {
                    if (projected->audioPlugins.at(index).id != previous.at(index).id) {
                        return false;
                    }
                }
                projected->audioPlugins = reordered;
                return true;
            },
            [this, clipId, previous]() {
                auto *projected = findClipById(clipId);
                if (projected == nullptr) {
                    return false;
                }
                projected->audioPlugins = previous;
                return true;
            })) {
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
        auto *pasted = effect->clone();
        const QVariantMap request = insertEffectsRequest(
            clipId,
            {QVariantMap{{QStringLiteral("index"), idx},
                         {QStringLiteral("effect"), effectMutationDocument(pasted)}}});
        if (!commitTimelineStructureMutation(
                request,
                [this, clipId, idx, pasted]() {
                    auto *projected = findClipById(clipId);
                    if (projected == nullptr || idx > projected->effects.size() ||
                        projected->effects.contains(pasted)) {
                        return false;
                    }
                    projected->effects.insert(idx, pasted);
                    return true;
                },
                [this, clipId, pasted]() {
                    auto *projected = findClipById(clipId);
                    return projected != nullptr && projected->effects.removeOne(pasted);
                },
                {},
                [this, pasted]() { deleteDetachedEffects(this, {pasted}); })) {
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
    const auto *clip = findClipById(clipId);
    if (clip != nullptr) {
        if (effectIndex >= 0 && effectIndex < static_cast<int>(clip->effects.size())) {
            const auto *effect = clip->effects.value(effectIndex);
            const int oldDuration = clip->durationFrames;
            const std::optional<double> mediaDurationSeconds =
                probeAudioDuration(*clip, effect, paramName, value);

            if (!commitTimelineStateMutation(
                    setEffectParameterRequest(clipId, effectIndex, paramName, value,
                                              mediaDurationSeconds))) {
                qWarning() << "Rust rejected effect parameter update";
                return;
            }
            publishEffectParameterChange(clipId, effectIndex, paramName, oldDuration);
        }
    }
}

void TimelineService::publishEffectParameterChange(int clipId, int effectIndex,
                                                   const QString &paramName,
                                                   int previousDuration) {
    const auto *clip = findClipById(clipId);
    if (clip == nullptr || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *effect = clip->effects.at(effectIndex);
    if (effect == nullptr) {
        return;
    }
    const QVariant value = effect->params().value(paramName);
    const bool durationChanged = clip->durationFrames != previousDuration;
    const bool waveformChanged = affectsAudioWaveform(*clip, effect, paramName);

    emit effectParamChanged(clipId, effectIndex, paramName, value);

    // layerCount only changes the camera's layer range, so effectParamChanged is enough.
    if (durationChanged || paramName == QLatin1String("path") ||
        paramName == QLatin1String("source") ||
        paramName == QStringLiteral("targetSceneId")) {
        emit clipsChanged();
    }
    if (durationChanged || waveformChanged) {
        emit clipEffectsChanged(clipId);
    }

    if (m_selection != nullptr && m_selection->selectedClipId() == clipId) {
        QVariantMap data = m_selection->selectedClipData();
        data.insert(paramName, value);
        if (durationChanged) {
            data.insert(QStringLiteral("durationFrames"), clip->durationFrames);
        }
        m_selection->refreshSelectionData(clipId, data);
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
    if (clip == nullptr || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *effect = clip->effects.at(effectIndex);
    const auto projection = AviQtl::Core::RustKeyframeDocument::set(
        effect->keyframeTracks().value(paramName), effect->params().value(paramName), 0,
        frame, value, options);
    if (!projection || !projection->accepted) {
        return;
    }
    if (!commitTimelineStateMutation(
            setEffectKeyframeRequest(clipId, effectIndex, paramName, frame, value, options),
            [this, clipId, effectIndex, paramName, projection]() {
                auto *committedClip = findClipById(clipId);
                if (committedClip == nullptr || effectIndex >= committedClip->effects.size()) {
                    return;
                }
                auto *committedEffect = committedClip->effects.at(effectIndex);
                if (projection->baseValue) {
                    QVariantMap params = committedEffect->params();
                    params.insert(paramName, *projection->baseValue);
                    committedEffect->setParams(params);
                }
                QVariantMap tracks = committedEffect->keyframeTracks();
                tracks.insert(paramName, projection->track);
                committedEffect->setKeyframeTracks(tracks, committedClip->durationFrames);
            })) {
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

void TimelineService::removeKeyframeInternal(int clipId, int effectIndex, const QString &paramName, int frame) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if (clip == nullptr || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *effect = clip->effects.at(effectIndex);
    const auto projection = AviQtl::Core::RustKeyframeDocument::remove(
        effect->keyframeTracks().value(paramName), effect->params().value(paramName), 0,
        frame);
    if (!projection || !projection->accepted || !projection->changed) {
        return;
    }
    if (!commitTimelineStateMutation(
            removeEffectKeyframeRequest(clipId, effectIndex, paramName, frame),
            [this, clipId, effectIndex, paramName, projection]() {
                auto *committedClip = findClipById(clipId);
                if (committedClip == nullptr || effectIndex >= committedClip->effects.size()) {
                    return;
                }
                auto *committedEffect = committedClip->effects.at(effectIndex);
                QVariantMap tracks = committedEffect->keyframeTracks();
                tracks.insert(paramName, projection->track);
                committedEffect->setKeyframeTracks(tracks, committedClip->durationFrames);
            })) {
        qWarning() << "Rust rejected effect keyframe removal";
        return;
    }

    emit effectParamChanged(clipId, effectIndex, paramName, QVariant());
    if (paramName == QLatin1String("path") || paramName == QLatin1String("source") || paramName == QStringLiteral("targetSceneId")) {
        emit clipsChanged();
    }
}

void TimelineService::moveKeyframeInternal(int clipId, int effectIndex, const QString &paramName, int oldFrame, int newFrame) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if (clip == nullptr || effectIndex < 0 || effectIndex >= clip->effects.size()) {
        return;
    }
    const auto *effect = clip->effects.at(effectIndex);
    const auto projection = AviQtl::Core::RustKeyframeDocument::move(
        effect->keyframeTracks().value(paramName), effect->params().value(paramName), 0,
        oldFrame, newFrame);
    if (!projection || !projection->accepted || !projection->changed) {
        return;
    }
    if (!commitTimelineStateMutation(
            moveEffectKeyframeRequest(clipId, effectIndex, paramName, oldFrame, newFrame),
            [this, clipId, effectIndex, paramName, projection]() {
                auto *committedClip = findClipById(clipId);
                if (committedClip == nullptr || effectIndex >= committedClip->effects.size()) {
                    return;
                }
                auto *committedEffect = committedClip->effects.at(effectIndex);
                QVariantMap tracks = committedEffect->keyframeTracks();
                tracks.insert(paramName, projection->track);
                committedEffect->setKeyframeTracks(tracks, committedClip->durationFrames);
            })) {
        qWarning() << "Rust rejected effect keyframe move";
        return;
    }

    emit effectParamChanged(clipId, effectIndex, paramName, QVariant());
    if (paramName == QLatin1String("path") || paramName == QLatin1String("source") || paramName == QStringLiteral("targetSceneId")) {
        emit clipsChanged();
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
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);
    const auto projection = AviQtl::Core::RustKeyframeDocument::set(
        plugin.keyframeTracks.value(paramKey), plugin.params.value(paramKey), 0, frame,
        value, options);
    if (!projection || !projection->accepted) {
        return;
    }
    if (!commitTimelineStateMutation(
            setAudioPluginKeyframeRequest(clipId, pluginIndex, paramKey, frame, value,
                                          options),
            [this, clipId, pluginIndex, paramKey, projection]() {
                auto *committedClip = findClipById(clipId);
                if (committedClip == nullptr || pluginIndex >= committedClip->audioPlugins.size()) {
                    return;
                }
                auto &committedPlugin = committedClip->audioPlugins[pluginIndex];
                if (projection->baseValue) {
                    committedPlugin.params.insert(paramKey, *projection->baseValue);
                }
                committedPlugin.keyframeTracks.insert(paramKey, projection->track);
                committedPlugin.invalidateKeyframeCache();
            })) {
        qWarning() << "Rust rejected audio plugin keyframe update";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::removeAudioPluginKeyframeInternal(int clipId, int pluginIndex, const QString &paramKey, int frame) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);
    const auto projection = AviQtl::Core::RustKeyframeDocument::remove(
        plugin.keyframeTracks.value(paramKey), plugin.params.value(paramKey), 0, frame);
    if (!projection || !projection->accepted || !projection->changed) {
        return;
    }
    if (!commitTimelineStateMutation(
            removeAudioPluginKeyframeRequest(clipId, pluginIndex, paramKey, frame),
            [this, clipId, pluginIndex, paramKey, projection]() {
                auto *committedClip = findClipById(clipId);
                if (committedClip == nullptr || pluginIndex >= committedClip->audioPlugins.size()) {
                    return;
                }
                auto &committedPlugin = committedClip->audioPlugins[pluginIndex];
                committedPlugin.keyframeTracks.insert(paramKey, projection->track);
                committedPlugin.invalidateKeyframeCache();
            })) {
        qWarning() << "Rust rejected audio plugin keyframe removal";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

void TimelineService::moveAudioPluginKeyframeInternal(int clipId, int pluginIndex, const QString &paramKey, int oldFrame, int newFrame) { // NOLINT(bugprone-easily-swappable-parameters)
    const auto *clip = findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return;
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);
    const auto projection = AviQtl::Core::RustKeyframeDocument::move(
        plugin.keyframeTracks.value(paramKey), plugin.params.value(paramKey), 0, oldFrame,
        newFrame);
    if (!projection || !projection->accepted || !projection->changed) {
        return;
    }
    if (!commitTimelineStateMutation(
            moveAudioPluginKeyframeRequest(clipId, pluginIndex, paramKey, oldFrame, newFrame),
            [this, clipId, pluginIndex, paramKey, projection]() {
                auto *committedClip = findClipById(clipId);
                if (committedClip == nullptr || pluginIndex >= committedClip->audioPlugins.size()) {
                    return;
                }
                auto &committedPlugin = committedClip->audioPlugins[pluginIndex];
                committedPlugin.keyframeTracks.insert(paramKey, projection->track);
                committedPlugin.invalidateKeyframeCache();
            })) {
        qWarning() << "Rust rejected audio plugin keyframe move";
        return;
    }
    emit clipEffectsChanged(clipId);
    emit clipsChanged();
}

} // namespace AviQtl::UI
