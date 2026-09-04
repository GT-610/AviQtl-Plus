#include "bake_controller.hpp"
#include "core/include/document_model.hpp"
#include "core/include/effect_registry.hpp"
#include "core/include/performance_metrics.hpp"
#include "core/include/rust_timeline_bake.hpp"
#include "core/include/settings_manager.hpp"
#include "ecs.hpp"
#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <algorithm>
#include <bitset>
#include <cstring>
#include <limits>

namespace AviQtl::Engine::Timeline {

namespace {

QByteArray sceneBakeSnapshot(const AviQtl::Core::SceneSettings &scene) {
    QVariantList clips;
    clips.reserve(static_cast<qsizetype>(scene.clips.size()));
    const auto &registry = AviQtl::Core::EffectRegistry::instance();

    for (const auto &clip : scene.clips) {
        QVariantList effects;
        effects.reserve(static_cast<qsizetype>(clip.effects.size()));
        for (const auto &effect : clip.effects) {
            QVariantMap keyframes;
            for (const auto &[name, source] : effect.keyframes) {
                QVariantList points;
                points.reserve(static_cast<qsizetype>(source.size()));
                for (const auto &keyframe : source) {
                    QVariantMap point{
                        {QStringLiteral("frame"), keyframe.frame},
                        {QStringLiteral("value"), keyframe.value},
                        {QStringLiteral("interp"), keyframe.interpolation},
                    };
                    if (keyframe.interpolation == QStringLiteral("custom")) {
                        point.insert(QStringLiteral("bzx1"), keyframe.bzx1);
                        point.insert(QStringLiteral("bzy1"), keyframe.bzy1);
                        point.insert(QStringLiteral("bzx2"), keyframe.bzx2);
                        point.insert(QStringLiteral("bzy2"), keyframe.bzy2);
                    }
                    points.append(std::move(point));
                }
                keyframes.insert(name, std::move(points));
            }

            effects.append(QVariantMap{
                {QStringLiteral("id"), effect.id},
                {QStringLiteral("enabled"), effect.enabled},
                {QStringLiteral("known"), !registry.getEffect(effect.id).id.isEmpty()},
                {QStringLiteral("params"), effect.params},
                {QStringLiteral("keyframes"), std::move(keyframes)},
            });
        }

        clips.append(QVariantMap{
            {QStringLiteral("id"), clip.id},
            {QStringLiteral("layer"), clip.layer},
            {QStringLiteral("startFrame"), clip.startFrame},
            {QStringLiteral("durationFrames"), clip.durationFrames},
            {QStringLiteral("clipByUpperObject"), clip.clipByUpperObject},
            {QStringLiteral("type"), clip.type},
            {QStringLiteral("effects"), std::move(effects)},
        });
    }

    const QVariantMap snapshot{
        {QStringLiteral("fps"), scene.fps},
        {QStringLiteral("maxClipId"), MAX_CLIP_ID},
        {QStringLiteral("clips"), std::move(clips)},
    };
    return QJsonDocument(QJsonObject::fromVariantMap(snapshot)).toJson(QJsonDocument::Compact);
}

AudioComponent projectAudio(const AviQtl::RustCore::AudioBakeOutput &output) {
    return {
        .clipId = output.clip_id,
        .startFrame = output.start_frame,
        .durationFrames = output.duration_frames,
        .sourceStartTime = output.source_start_time,
        .playbackSpeed = output.playback_speed,
        .directTime = output.direct_time,
        .volume = output.volume,
        .masterVolume = output.master_volume,
        .pan = output.pan,
        .fadeInSec = output.fade_in_seconds,
        .fadeOutSec = output.fade_out_seconds,
        .mute = output.mute != 0,
        .solo = output.solo != 0,
        .limiter = output.limiter != 0,
        .directMode = output.direct_mode != 0,
    };
}

RenderComponent projectRender(const AviQtl::RustCore::RenderBakeOutput &output) {
    return {
        .clipId = output.clip_id,
        .layer = output.layer,
        .timePosition = output.time_position,
        .startFrame = output.start_frame,
        .durationFrames = output.duration_frames,
        .x = output.x,
        .y = output.y,
        .z = output.z,
        .rotX = output.rotation_x,
        .rotY = output.rotation_y,
        .rotZ = output.rotation_z,
        .scaleX = output.scale_x,
        .scaleY = output.scale_y,
        .opacity = output.opacity,
        .clipByUpperObject = output.clip_by_upper_object != 0,
        .effectCount = output.effect_count,
        .effectStartIndex = output.effect_start_index,
    };
}

EffectParamEntry projectParameter(const AviQtl::RustCore::EffectParamEntry &source) {
    EffectParamEntry target;
    target.clipId = source.clip_id;
    target.effectIndex = source.effect_index;
    target.paramType = static_cast<ParamType>(source.param_type);
    static_assert(sizeof(target.paramName) == sizeof(source.param_name));
    static_assert(sizeof(target.value) == sizeof(source.value));
    std::memcpy(target.paramName, source.param_name, sizeof(target.paramName));
    std::memcpy(target.value, source.value, sizeof(target.value));
    return target;
}

} // namespace

struct BakeController::CacheState {
    quint64 documentRevision = std::numeric_limits<quint64>::max();
    int sceneId = -1;
    AviQtl::RustCore::TimelineBakePlan plan;

    AviQtl::RustCore::TimelineBakeStatus rebuild(const AviQtl::Core::SceneSettings &scene,
                                                 quint64 revision) {
        const auto status = plan.reset(sceneBakeSnapshot(scene));
        if (status == AviQtl::RustCore::TimelineBakeStatus::Ok) {
            documentRevision = revision;
            sceneId = scene.id;
        }
        return status;
    }
};

BakeController::BakeController() : m_cache(std::make_unique<CacheState>()) {
    connect(&AviQtl::Core::DocumentModel::instance(),
            &AviQtl::Core::DocumentModel::structureChanged, this,
            &BakeController::onStructureChanged);
}

BakeController::~BakeController() = default;

BakeController &BakeController::instance() {
    static BakeController inst;
    return inst;
}

void BakeController::bake(int sceneId, int currentFrame) {
    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    metrics.add(AviQtl::Core::PerformanceCounter::BakeCalls);
    AviQtl::Core::ScopedPerformanceTimer bakeTimer(
        AviQtl::Core::PerformanceCounter::BakeNanoseconds);

    const auto &document = AviQtl::Core::DocumentModel::instance();
    const auto *scene = document.findScene(sceneId);
    if (!scene)
        return;

    const bool cacheRebuilt = m_cache->documentRevision != document.revision() ||
                              m_cache->sceneId != sceneId;
    if (cacheRebuilt) {
        const auto status = m_cache->rebuild(*scene, document.revision());
        if (status != AviQtl::RustCore::TimelineBakeStatus::Ok) {
            metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineFailures);
            qWarning() << "Rust scene bake plan rebuild failed with status"
                       << static_cast<std::uint32_t>(status);
            return;
        }
        metrics.add(AviQtl::Core::PerformanceCounter::BakeTrackCacheMisses,
                    m_cache->plan.effectCount());
    }

    auto &settings = AviQtl::Core::SettingsManager::instance();
    const bool fullBake =
        settings.value(QStringLiteral("bakeStrategy"), QStringLiteral("OnDemand")).toString() ==
        QStringLiteral("FullBake");
    const int prefetch = std::max(
        0, settings.value(QStringLiteral("onDemandPrefetchFrames"), 30).toInt());

    AviQtl::RustCore::SceneBakeResult result;
    const auto status = m_cache->plan.evaluate(currentFrame, fullBake, prefetch, result);
    if (status != AviQtl::RustCore::TimelineBakeStatus::Ok) {
        metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineFailures);
        qWarning() << "Rust scene bake plan evaluation failed with status"
                   << static_cast<std::uint32_t>(status);
        return;
    }

    metrics.add(AviQtl::Core::PerformanceCounter::BakeClipsVisited,
                result.counts.clips_visited);
    metrics.add(AviQtl::Core::PerformanceCounter::BakeClipsActive,
                result.counts.render_count);
    if (!cacheRebuilt) {
        metrics.add(AviQtl::Core::PerformanceCounter::BakeTrackCacheHits,
                    result.counts.selected_effect_count);
    }
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustKeyframeBatchCalls,
                result.counts.numeric_batch_calls);
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustKeyframeTracks,
                result.counts.numeric_track_count);
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineRenderCalls,
                result.counts.render_count);
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineAudioCalls,
                result.counts.audio_count);

    auto &ecs = ECS::instance();
    ecs.clearEffectParams();
    auto &parameters = ecs.editState().effectParams.entries;
    parameters.reserve(result.params.size());
    for (const auto &parameter : result.params)
        parameters.push_back(projectParameter(parameter));

    for (const auto &output : result.audio) {
        if (output.clip_id < 0 || output.clip_id >= MAX_CLIP_ID)
            continue;
        ecs.updateAudioClipState(output.clip_id, projectAudio(output));
    }

    std::bitset<MAX_CLIP_ID> aliveFlags;
    for (const auto &output : result.renders) {
        if (output.clip_id < 0 || output.clip_id >= MAX_CLIP_ID)
            continue;
        aliveFlags.set(static_cast<std::size_t>(output.clip_id));
        const RenderComponent render = projectRender(output);
        ecs.updateClipState(output.clip_id, output.layer, output.time_position,
                            output.start_frame, output.duration_frames);
        ecs.updateRenderState(output.clip_id, render);
    }

    ecs.syncClipIds(aliveFlags);
    ecs.commit();

    m_lastSceneId = sceneId;
    m_lastFrame = currentFrame;
}

void BakeController::triggerRebake() {
    if (m_lastSceneId != -1)
        bake(m_lastSceneId, m_lastFrame != -1 ? m_lastFrame : 0);
}

void BakeController::onStructureChanged() { triggerRebake(); }

} // namespace AviQtl::Engine::Timeline
