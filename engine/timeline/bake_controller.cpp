#include "bake_controller.hpp"
#include "core/include/document_model.hpp"
#include "core/include/effect_registry.hpp"
#include "core/include/keyframe_utils.hpp"
#include "core/include/media_utils.hpp"
#include "core/include/performance_metrics.hpp"
#include "core/include/rust_keyframe_adapter.hpp"
#include "core/include/rust_timeline_bake.hpp"
#include "core/include/settings_manager.hpp"
#include "ecs.hpp"
#include <algorithm>
#include <bitset>
#include <limits>
#include <optional>
#include <QDebug>
#include <QSet>
#include <unordered_map>

namespace AviQtl::Engine::Timeline {

namespace {

QVariantMap keyframesToTrack(const std::vector<AviQtl::Core::Keyframe> &kfs, const QVariant &fallback) {
    QVariantMap track;
    QVariantMap start;
    start[QStringLiteral("frame")] = 0;
    start[QStringLiteral("value")] = fallback;
    start[QStringLiteral("interp")] = QStringLiteral("none");

    QVariantList points;
    for (const auto &kf : kfs) {
        if (kf.frame <= 0) {
            start[QStringLiteral("value")] = kf.value;
            start[QStringLiteral("interp")] = kf.interpolation;
            if (kf.interpolation == QStringLiteral("custom")) {
                QVariantList pts;
                pts << kf.bzx1 << kf.bzy1 << kf.bzx2 << kf.bzy2 << 1.0 << 1.0;
                start[QStringLiteral("points")] = pts;
            }
            continue;
        }
        QVariantMap p;
        p[QStringLiteral("frame")] = kf.frame;
        p[QStringLiteral("value")] = kf.value;
        p[QStringLiteral("interp")] = kf.interpolation;
        if (kf.interpolation == QStringLiteral("custom")) {
            QVariantList pts;
            pts << kf.bzx1 << kf.bzy1 << kf.bzx2 << kf.bzy2 << 1.0 << 1.0;
            p[QStringLiteral("points")] = pts;
        }
        points.append(p);
    }

    track[QStringLiteral("start")] = start;
    track[QStringLiteral("points")] = points;
    return track;
}

struct ResolvedTracks {
    QVariantMap params;
    QHash<QString, QVariantList> resolved;
    QSet<QString> allKeys;
    AviQtl::Core::RustKeyframes::NumericTrackBatch numericBatch;
};

void evaluateNumericTracks(ResolvedTracks &tracks, int frame) {
    const auto result = tracks.numericBatch.evaluate(frame);
    if (result != AviQtl::Core::RustKeyframes::NumericTrackBatch::Evaluation::Evaluated)
        return;
    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustKeyframeBatchCalls);
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustKeyframeTracks,
                tracks.numericBatch.size());
}

QVariant evalValue(const ResolvedTracks &rt, const QString &key, int frame) {
    if (key == QStringLiteral("time"))
        return frame;
    if (const std::optional<double> numeric = rt.numericBatch.value(key))
        return *numeric;
    return AviQtl::Core::KeyframeUtils::evaluateResolvedParam(rt.params, rt.resolved, key, frame);
}

float evalFloat(const ResolvedTracks &rt, const QString &key, int frame) {
    const QVariant v = evalValue(rt, key, frame);
    return static_cast<float>(v.toDouble());
}

float evalFloatOr(const ResolvedTracks &rt, const QString &key, float fallback, int frame) {
    if (!rt.params.contains(key) && !rt.resolved.contains(key)) {
        return fallback;
    }
    return evalFloat(rt, key, frame);
}

ResolvedTracks buildResolvedTracks(const AviQtl::Core::Effect &effect, int clipDuration) {
    ResolvedTracks out;
    out.params = effect.params;
    out.allKeys.insert(QStringLiteral("time"));

    for (auto it = effect.params.constBegin(); it != effect.params.constEnd(); ++it) {
        out.allKeys.insert(it.key());
    }
    for (auto it = effect.keyframes.begin(); it != effect.keyframes.end(); ++it) {
        out.allKeys.insert(it->first);
        const QVariant fallback = effect.params.value(it->first);
        const QVariantMap track = keyframesToTrack(it->second, fallback);
        const auto inspected =
            AviQtl::Core::RustKeyframeDocument::inspect(track, fallback, clipDuration);
        out.resolved.insert(it->first, inspected ? inspected->flat : QVariantList{});
    }

    // Each track crosses the document boundary once; subsequent per-frame evaluations only
    // walk the flattened list and apply easing.
    out.numericBatch.rebuild(out.params, out.resolved);
    return out;
}

struct CachedEffect {
    ResolvedTracks tracks;
};

struct CachedClip {
    std::size_t sceneClipIndex = 0;
    int clipId = -1;
    int startFrame = 0;
    int endFrame = 0;
    std::vector<CachedEffect> effects;
};

bool bakeClipEffects(const AviQtl::Core::Clip &clip, const CachedClip &cachedClip, int currentFrame, double fps,
                     RenderComponent &render, EffectParamBuffer &paramBuf) {
    Q_UNUSED(fps);
    const int relFrame = std::max(0, currentFrame - clip.startFrame);
    const std::size_t initialParamCount = paramBuf.entries.size();
    auto &registry = AviQtl::Core::EffectRegistry::instance();

    AviQtl::RustCore::RenderBakeInput renderInput{
        .clip_id = clip.id,
        .layer = clip.layer,
        .current_frame = currentFrame,
        .start_frame = clip.startFrame,
        .duration_frames = clip.durationFrames,
        .clip_by_upper_object = static_cast<std::uint32_t>(clip.clipByUpperObject),
        .effect_count = 0,
        .reserved = 0,
        .effect_start_index = render.effectStartIndex,
        .has_transform = 0,
        .x = 0.0F,
        .y = 0.0F,
        .z = 0.0F,
        .rotation_x = 0.0F,
        .rotation_y = 0.0F,
        .rotation_z = 0.0F,
        .scale = 100.0F,
        .opacity = 1.0F,
    };
    uint16_t effectIdx = 0;

    for (std::size_t effectIndex = 0; effectIndex < clip.effects.size(); ++effectIndex) {
        const auto &effect = clip.effects[effectIndex];
        if (!effect.enabled) {
            ++effectIdx;
            continue;
        }

        const auto &meta = registry.getEffect(effect.id);
        if (meta.id.isEmpty()) {
            ++effectIdx;
            continue;
        }

        if (effectIndex >= cachedClip.effects.size()) {
            ++effectIdx;
            continue;
        }
        const ResolvedTracks &rt = cachedClip.effects[effectIndex].tracks;

        if (effect.id == QStringLiteral("transform")) {
            renderInput.has_transform = 1;
            renderInput.x = evalFloat(rt, QStringLiteral("x"), relFrame);
            renderInput.y = evalFloat(rt, QStringLiteral("y"), relFrame);
            renderInput.z = evalFloat(rt, QStringLiteral("z"), relFrame);
            renderInput.rotation_x = evalFloat(rt, QStringLiteral("rotationX"), relFrame);
            renderInput.rotation_y = evalFloat(rt, QStringLiteral("rotationY"), relFrame);
            renderInput.rotation_z = evalFloat(rt, QStringLiteral("rotationZ"), relFrame);
            renderInput.scale = evalFloat(rt, QStringLiteral("scale"), relFrame);
            renderInput.opacity = evalFloat(rt, QStringLiteral("opacity"), relFrame);
        }

        for (const auto &key : std::as_const(rt.allKeys)) {
            EffectParamEntry entry;
            entry.clipId = static_cast<uint32_t>(clip.id);
            entry.effectIndex = effectIdx;

            const QByteArray nameBytes = key.toUtf8();
            auto copyLen = static_cast<std::size_t>(std::min<qsizetype>(nameBytes.size(), 19));
            // Avoid truncating in the middle of a multi-byte UTF-8 character
            // Only check when we actually truncated (copyLen < nameBytes.size())
            while (copyLen > 0 && copyLen < static_cast<std::size_t>(nameBytes.size()) &&
                   (static_cast<unsigned char>(nameBytes[static_cast<int>(copyLen)]) & 0xC0) == 0x80) {
                --copyLen;
            }
            std::memcpy(entry.paramName, nameBytes.constData(), copyLen);
            entry.paramName[copyLen] = '\0';

            QVariant evaluated = evalValue(rt, key, relFrame);

            if (evaluated.canConvert<QColor>()) {
                QColor c(evaluated.toString());
                entry.paramType = ParamType::Color;
                entry.value[0] = static_cast<float>(c.redF());
                entry.value[1] = static_cast<float>(c.greenF());
                entry.value[2] = static_cast<float>(c.blueF());
                entry.value[3] = static_cast<float>(c.alphaF());
            } else {
                entry.paramType = ParamType::Float;
                entry.value[0] = static_cast<float>(evaluated.toDouble());
            }

            paramBuf.entries.push_back(entry);
        }

        ++effectIdx;
    }

    renderInput.effect_count = effectIdx;
    AviQtl::RustCore::RenderBakeOutput output{};
    const auto status = AviQtl::RustCore::bakeRender(renderInput, output);
    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    if (status != AviQtl::RustCore::TimelineBakeStatus::Ok) {
        paramBuf.entries.resize(initialParamCount);
        metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineFailures);
        qWarning() << "Rust render timeline bake failed for clip" << clip.id
                   << "with status" << static_cast<std::uint32_t>(status);
        return false;
    }
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineRenderCalls);
    render.clipId = output.clip_id;
    render.layer = output.layer;
    render.timePosition = output.time_position;
    render.startFrame = output.start_frame;
    render.durationFrames = output.duration_frames;
    render.x = output.x;
    render.y = output.y;
    render.z = output.z;
    render.rotX = output.rotation_x;
    render.rotY = output.rotation_y;
    render.rotZ = output.rotation_z;
    render.scaleX = output.scale_x;
    render.scaleY = output.scale_y;
    render.opacity = output.opacity;
    render.clipByUpperObject = output.clip_by_upper_object != 0;
    render.effectCount = output.effect_count;
    render.effectStartIndex = output.effect_start_index;
    return true;
}

std::optional<AudioComponent> bakeAudioState(const AviQtl::Core::Clip &clip, const CachedClip &cachedClip,
                                             int currentFrame, double fps) {
    AviQtl::RustCore::AudioBakeInput input{
        .clip_id = clip.id,
        .start_frame = clip.startFrame,
        .duration_frames = clip.durationFrames,
        .has_audio_effect = 0,
        .fps = fps,
        .source_start_time = 0.0F,
        .speed_percent = static_cast<float>(AviQtl::kDefaultSpeed),
        .direct_time = 0.0F,
        .volume = 1.0F,
        .master_volume = 1.0F,
        .pan = 0.0F,
        .fade_in_seconds = 0.0F,
        .fade_out_seconds = 0.0F,
        .direct_mode = 0,
        .mute = 0,
        .solo = 0,
        .limiter = 0,
    };
    if (!(fps <= 0.0)) {
        const int relFrame = std::max(0, currentFrame - clip.startFrame);
        auto it = std::find_if(clip.effects.begin(), clip.effects.end(),
            [](const auto &e) { return e.enabled && e.id == QStringLiteral("audio"); });
        if (it != clip.effects.end()) {
            const auto &effect = *it;
            const std::size_t effectIndex = static_cast<std::size_t>(std::distance(clip.effects.begin(), it));
            if (effectIndex < cachedClip.effects.size()) {
                const ResolvedTracks &rt = cachedClip.effects[effectIndex].tracks;
                input.has_audio_effect = 1;
                const QString playMode = effect.params.value(QStringLiteral("playMode")).toString();
                input.direct_mode = static_cast<std::uint32_t>(
                    AviQtl::Core::MediaUtils::isDirectAudioMode(playMode));
                input.source_start_time = evalFloatOr(rt, QStringLiteral("startTime"), 0.0F, relFrame);
                input.speed_percent = evalFloatOr(
                    rt, QStringLiteral("speed"), static_cast<float>(AviQtl::kDefaultSpeed), relFrame);
                input.direct_time = evalFloatOr(rt, QStringLiteral("directTime"), 0.0F, relFrame);
                input.volume = evalFloatOr(rt, QStringLiteral("volume"), 1.0F, relFrame);
                input.master_volume = evalFloatOr(rt, QStringLiteral("masterVolume"), 1.0F, relFrame);
                input.pan = evalFloatOr(rt, QStringLiteral("pan"), 0.0F, relFrame);
                input.fade_in_seconds = evalFloatOr(rt, QStringLiteral("fadeIn"), 0.0F, relFrame);
                input.fade_out_seconds = evalFloatOr(rt, QStringLiteral("fadeOut"), 0.0F, relFrame);
                input.mute = static_cast<std::uint32_t>(effect.params.value(QStringLiteral("mute"), false).toBool());
                input.solo = static_cast<std::uint32_t>(effect.params.value(QStringLiteral("solo"), false).toBool());
                input.limiter = static_cast<std::uint32_t>(effect.params.value(QStringLiteral("limiter"), true).toBool());
            }
        }
    }

    AviQtl::RustCore::AudioBakeOutput output{};
    AudioComponent audio;
    const auto status = AviQtl::RustCore::bakeAudio(input, output);
    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    if (status != AviQtl::RustCore::TimelineBakeStatus::Ok) {
        metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineFailures);
        qWarning() << "Rust audio timeline bake failed for clip" << clip.id
                   << "with status" << static_cast<std::uint32_t>(status);
        return std::nullopt;
    }
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustTimelineAudioCalls);
    audio.clipId = output.clip_id;
    audio.startFrame = output.start_frame;
    audio.durationFrames = output.duration_frames;
    audio.sourceStartTime = output.source_start_time;
    audio.playbackSpeed = output.playback_speed;
    audio.directTime = output.direct_time;
    audio.volume = output.volume;
    audio.masterVolume = output.master_volume;
    audio.pan = output.pan;
    audio.fadeInSec = output.fade_in_seconds;
    audio.fadeOutSec = output.fade_out_seconds;
    audio.mute = output.mute != 0;
    audio.solo = output.solo != 0;
    audio.limiter = output.limiter != 0;
    audio.directMode = output.direct_mode != 0;
    return audio;
}

} // namespace

struct BakeController::CacheState {
    static constexpr int kFrameBucketSize = 120;

    quint64 documentRevision = std::numeric_limits<quint64>::max();
    int sceneId = -1;
    std::vector<CachedClip> clips;
    std::unordered_map<int, std::vector<std::size_t>> timeBuckets;

    void rebuild(const AviQtl::Core::SceneSettings &scene, quint64 revision) {
        documentRevision = revision;
        sceneId = scene.id;
        clips.clear();
        timeBuckets.clear();
        clips.reserve(scene.clips.size());

        quint64 resolvedEffectCount = 0;
        for (std::size_t sceneIndex = 0; sceneIndex < scene.clips.size(); ++sceneIndex) {
            const auto &clip = scene.clips[sceneIndex];
            if (clip.id < 0 || clip.id >= MAX_CLIP_ID)
                continue;

            CachedClip cached;
            cached.sceneClipIndex = sceneIndex;
            cached.clipId = clip.id;
            cached.startFrame = clip.startFrame;
            cached.endFrame = clip.startFrame + std::max(0, clip.durationFrames);
            cached.effects.reserve(clip.effects.size());
            for (const auto &effect : clip.effects) {
                cached.effects.push_back({
                    .tracks = buildResolvedTracks(effect, clip.durationFrames),
                });
                ++resolvedEffectCount;
            }

            const std::size_t cachedIndex = clips.size();
            clips.push_back(std::move(cached));

            const int firstBucket = clip.startFrame / kFrameBucketSize;
            const int lastBucket = std::max(clip.startFrame, clip.startFrame + std::max(0, clip.durationFrames)) / kFrameBucketSize;
            for (int bucket = firstBucket; bucket <= lastBucket; ++bucket)
                timeBuckets[bucket].push_back(cachedIndex);
        }

        AviQtl::Core::PerformanceMetrics::instance().add(AviQtl::Core::PerformanceCounter::BakeTrackCacheMisses, resolvedEffectCount);
    }
};

BakeController::BakeController() : m_cache(std::make_unique<CacheState>()) {
    connect(&AviQtl::Core::DocumentModel::instance(), &AviQtl::Core::DocumentModel::structureChanged, this, &BakeController::onStructureChanged);
}

BakeController::~BakeController() = default;

BakeController &BakeController::instance() {
    static BakeController inst;
    return inst;
}

void BakeController::bake(int sceneId, int currentFrame) {
    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    metrics.add(AviQtl::Core::PerformanceCounter::BakeCalls);
    AviQtl::Core::ScopedPerformanceTimer bakeTimer(AviQtl::Core::PerformanceCounter::BakeNanoseconds);

    const auto &document = AviQtl::Core::DocumentModel::instance();
    const auto *scene = document.findScene(sceneId);
    if (!scene)
        return;

    const bool cacheRebuilt = m_cache->documentRevision != document.revision() || m_cache->sceneId != sceneId;
    if (cacheRebuilt)
        m_cache->rebuild(*scene, document.revision());

    auto &sm = AviQtl::Core::SettingsManager::instance();
    const QString strategy = sm.value(QStringLiteral("bakeStrategy"), QStringLiteral("OnDemand")).toString();
    const int prefetch = std::max(0, sm.value(QStringLiteral("onDemandPrefetchFrames"), 30).toInt());
    const double fps = scene->fps;

    const bool isFullBake = (strategy == QStringLiteral("FullBake"));
    std::bitset<MAX_CLIP_ID> aliveFlags;
    std::bitset<MAX_CLIP_ID> selectedFlags;
    std::vector<std::size_t> selectedClips;

    if (isFullBake) {
        selectedClips.resize(m_cache->clips.size());
        for (std::size_t i = 0; i < selectedClips.size(); ++i)
            selectedClips[i] = i;
        metrics.add(AviQtl::Core::PerformanceCounter::BakeClipsVisited, selectedClips.size());
    } else {
        const int rangeStart = currentFrame - prefetch;
        const int rangeEnd = currentFrame + prefetch;
        const int firstBucket = rangeStart / CacheState::kFrameBucketSize;
        const int lastBucket = rangeEnd / CacheState::kFrameBucketSize;
        for (int bucket = firstBucket; bucket <= lastBucket; ++bucket) {
            const auto bucketIt = m_cache->timeBuckets.find(bucket);
            if (bucketIt == m_cache->timeBuckets.end())
                continue;
            for (const std::size_t cachedIndex : bucketIt->second) {
                const auto &cached = m_cache->clips[cachedIndex];
                if (selectedFlags.test(static_cast<std::size_t>(cached.clipId)))
                    continue;
                selectedFlags.set(static_cast<std::size_t>(cached.clipId));
                metrics.add(AviQtl::Core::PerformanceCounter::BakeClipsVisited);
                if (cached.startFrame <= rangeEnd && cached.endFrame >= rangeStart)
                    selectedClips.push_back(cachedIndex);
            }
        }
    }

    metrics.add(AviQtl::Core::PerformanceCounter::BakeClipsActive, selectedClips.size());

    auto &ecs = ECS::instance();
    ecs.clearEffectParams();

    for (const std::size_t cachedIndex : selectedClips) {
        if (cachedIndex >= m_cache->clips.size())
            continue;
        auto &cachedClip = m_cache->clips[cachedIndex];
        if (cachedClip.sceneClipIndex >= scene->clips.size())
            continue;
        const auto &clip = scene->clips[cachedClip.sceneClipIndex];

        if (!cacheRebuilt)
            metrics.add(AviQtl::Core::PerformanceCounter::BakeTrackCacheHits, cachedClip.effects.size());
        aliveFlags.set(static_cast<std::size_t>(clip.id));

        const double relTime = static_cast<double>(std::max(0, currentFrame - clip.startFrame));
        const int relFrame = static_cast<int>(relTime);
        for (CachedEffect &effect : cachedClip.effects)
            evaluateNumericTracks(effect.tracks, relFrame);
        if (clip.type == QStringLiteral("audio") || clip.type == QStringLiteral("video")) {
            if (const std::optional<AudioComponent> audio =
                    bakeAudioState(clip, cachedClip, currentFrame, fps)) {
                ecs.updateAudioClipState(clip.id, *audio);
            }
        }

        RenderComponent render;
        render.effectStartIndex = static_cast<uint32_t>(ecs.editState().effectParams.entries.size());
        if (bakeClipEffects(clip, cachedClip, currentFrame, fps, render,
                            ecs.editState().effectParams)) {
            ecs.updateClipState(clip.id, clip.layer, relTime, clip.startFrame, clip.durationFrames);
            ecs.updateRenderState(clip.id, render);
        }
    }

    ecs.syncClipIds(aliveFlags);

    ecs.commit();

    m_lastSceneId = sceneId;
    m_lastFrame = currentFrame;
}

void BakeController::triggerRebake() {
    if (m_lastSceneId != -1) {
        bake(m_lastSceneId, m_lastFrame != -1 ? m_lastFrame : 0);
    }
}

void BakeController::onStructureChanged() { triggerRebake(); }

} // namespace AviQtl::Engine::Timeline
