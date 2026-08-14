#include "bake_controller.hpp"
#include "core/include/document_model.hpp"
#include "core/include/effect_registry.hpp"
#include "core/include/keyframe_utils.hpp"
#include "core/include/media_utils.hpp"
#include "core/include/performance_metrics.hpp"
#include "core/include/settings_manager.hpp"
#include "ecs.hpp"
#include <algorithm>
#include <bitset>
#include <limits>
#include <optional>
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

// Per-effect cache of resolved (normalized + flattened) keyframe tracks.
// Building this once per effect avoids repeating the expensive resolve step
// for every parameter evaluated below (e.g. transform reads ~8 params from
// the same set of tracks).
struct NumericTrackStorage {
    std::vector<AviQtl::RustCore::NumericKeyframe> keyframes;
    std::vector<double> customPoints;

    [[nodiscard]] AviQtl::RustCore::NumericTrackView view(double fallback) const {
        return {
            .keyframes = keyframes.data(),
            .keyframes_length = keyframes.size(),
            .custom_points = customPoints.data(),
            .custom_points_length = customPoints.size(),
            .fallback_value = fallback,
        };
    }
};

struct ResolvedTracks {
    QVariantMap params;
    QHash<QString, QVariantList> resolved;
    QSet<QString> allKeys;
    std::vector<NumericTrackStorage> numericTracks;
    std::vector<AviQtl::RustCore::NumericTrackView> numericViews;
    QHash<QString, std::size_t> numericIndices;
    std::vector<double> numericOutput;
    bool numericBatchValid = false;
};

bool isNumericValue(const QVariant &value) {
    return value.isValid() && value.typeId() != QMetaType::QString && value.canConvert<double>();
}

AviQtl::RustCore::NumericInterpolation numericInterpolation(const QString &name) {
    using enum AviQtl::RustCore::NumericInterpolation;
    static const QHash<QString, AviQtl::RustCore::NumericInterpolation> kinds = {
        {QStringLiteral("linear"), Linear},
        {QStringLiteral("ease_in_sine"), EaseInSine},
        {QStringLiteral("ease_out_sine"), EaseOutSine},
        {QStringLiteral("ease_in_out_sine"), EaseInOutSine},
        {QStringLiteral("ease_out_in_sine"), EaseOutInSine},
        {QStringLiteral("ease_in_quad"), EaseInQuad},
        {QStringLiteral("ease_out_quad"), EaseOutQuad},
        {QStringLiteral("ease_in_out_quad"), EaseInOutQuad},
        {QStringLiteral("ease_out_in_quad"), EaseOutInQuad},
        {QStringLiteral("ease_in_cubic"), EaseInCubic},
        {QStringLiteral("ease_out_cubic"), EaseOutCubic},
        {QStringLiteral("ease_in_out_cubic"), EaseInOutCubic},
        {QStringLiteral("ease_out_in_cubic"), EaseOutInCubic},
        {QStringLiteral("ease_in_quart"), EaseInQuart},
        {QStringLiteral("ease_out_quart"), EaseOutQuart},
        {QStringLiteral("ease_in_out_quart"), EaseInOutQuart},
        {QStringLiteral("ease_out_in_quart"), EaseOutInQuart},
        {QStringLiteral("ease_in_quint"), EaseInQuint},
        {QStringLiteral("ease_out_quint"), EaseOutQuint},
        {QStringLiteral("ease_in_out_quint"), EaseInOutQuint},
        {QStringLiteral("ease_out_in_quint"), EaseOutInQuint},
        {QStringLiteral("ease_in_expo"), EaseInExpo},
        {QStringLiteral("ease_out_expo"), EaseOutExpo},
        {QStringLiteral("ease_in_out_expo"), EaseInOutExpo},
        {QStringLiteral("ease_out_in_expo"), EaseOutInExpo},
        {QStringLiteral("ease_in_circ"), EaseInCirc},
        {QStringLiteral("ease_out_circ"), EaseOutCirc},
        {QStringLiteral("ease_in_out_circ"), EaseInOutCirc},
        {QStringLiteral("ease_out_in_circ"), EaseOutInCirc},
        {QStringLiteral("ease_in_back"), EaseInBack},
        {QStringLiteral("ease_out_back"), EaseOutBack},
        {QStringLiteral("ease_in_out_back"), EaseInOutBack},
        {QStringLiteral("ease_out_in_back"), EaseOutInBack},
        {QStringLiteral("ease_in_elastic"), EaseInElastic},
        {QStringLiteral("ease_out_elastic"), EaseOutElastic},
        {QStringLiteral("ease_in_out_elastic"), EaseInOutElastic},
        {QStringLiteral("ease_out_in_elastic"), EaseOutInElastic},
        {QStringLiteral("ease_out_bounce"), EaseOutBounce},
        {QStringLiteral("ease_in_bounce"), EaseInBounce},
        {QStringLiteral("ease_in_out_bounce"), EaseInOutBounce},
        {QStringLiteral("ease_out_in_bounce"), EaseOutInBounce},
        {QStringLiteral("custom"), Custom},
        {QStringLiteral("none"), None},
        {QStringLiteral("random"), Random},
        {QStringLiteral("alternate"), Alternate},
    };
    return kinds.value(name, Linear);
}

std::optional<NumericTrackStorage> buildNumericTrack(const QVariantList &track) {
    NumericTrackStorage storage;
    storage.keyframes.reserve(static_cast<std::size_t>(track.size()));
    for (const QVariant &pointValue : track) {
        const QVariantMap point = pointValue.toMap();
        const QVariant value = point.value(QStringLiteral("value"));
        if (!isNumericValue(value))
            return std::nullopt;

        const QString interpolationName = point.value(QStringLiteral("interp")).toString();
        const auto interpolation = numericInterpolation(interpolationName);
        const QVariantMap modeParams = point.value(QStringLiteral("modeParams")).toMap();
        const std::size_t customOffset = storage.customPoints.size();
        if (interpolation == AviQtl::RustCore::NumericInterpolation::Custom) {
            QVariantList customValues = point.value(QStringLiteral("points")).toList();
            if (customValues.isEmpty()) {
                customValues = {
                    point.value(QStringLiteral("bzx1"), 0.33),
                    point.value(QStringLiteral("bzy1"), 0.0),
                    point.value(QStringLiteral("bzx2"), 0.66),
                    point.value(QStringLiteral("bzy2"), 1.0),
                    1.0,
                    1.0,
                };
            }
            if (customOffset > std::numeric_limits<std::uint32_t>::max() ||
                static_cast<std::size_t>(customValues.size()) >
                    std::numeric_limits<std::uint32_t>::max() - customOffset) {
                return std::nullopt;
            }
            storage.customPoints.reserve(customOffset + static_cast<std::size_t>(customValues.size()));
            for (const QVariant &customValue : std::as_const(customValues))
                storage.customPoints.push_back(customValue.toDouble());
        }
        const std::size_t customLength = storage.customPoints.size() - customOffset;
        storage.keyframes.push_back({
            .frame = point.value(QStringLiteral("frame")).toInt(),
            .interpolation = static_cast<std::uint32_t>(interpolation),
            .step_frames = static_cast<std::uint32_t>(
                std::max(1, modeParams.value(QStringLiteral("stepFrames"), 1).toInt())),
            .custom_points_offset = static_cast<std::uint32_t>(customOffset),
            .custom_points_length = static_cast<std::uint32_t>(customLength),
            .reserved = 0,
            .value = value.toDouble(),
            .amplitude = modeParams.value(QStringLiteral("amplitude"), 1.0).toDouble(),
            .period = modeParams.value(QStringLiteral("period"), 0.3).toDouble(),
        });
    }
    return storage;
}

void evaluateNumericTracks(ResolvedTracks &tracks, int frame) {
    tracks.numericBatchValid = tracks.numericViews.empty();
    if (tracks.numericViews.empty())
        return;

    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    metrics.add(AviQtl::Core::PerformanceCounter::BakeRustKeyframeBatchCalls);
    const auto status = AviQtl::RustCore::evaluateNumericTracks(
        tracks.numericViews, static_cast<std::int32_t>(frame), tracks.numericOutput);
    tracks.numericBatchValid = status == AviQtl::RustCore::NumericBatchStatus::Ok;
    if (tracks.numericBatchValid) {
        metrics.add(AviQtl::Core::PerformanceCounter::BakeRustKeyframeTracks,
                    tracks.numericViews.size());
    }
}

QVariant evalValue(const ResolvedTracks &rt, const QString &key, int frame) {
    if (key == QStringLiteral("time"))
        return frame;
    const auto numeric = rt.numericIndices.constFind(key);
    if (rt.numericBatchValid && numeric != rt.numericIndices.constEnd())
        return rt.numericOutput[*numeric];
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

    int trackDuration = clipDuration;
    QVariantMap tracks;
    for (auto it = effect.params.constBegin(); it != effect.params.constEnd(); ++it) {
        out.allKeys.insert(it.key());
    }
    for (auto it = effect.keyframes.begin(); it != effect.keyframes.end(); ++it) {
        out.allKeys.insert(it->first);
        const QVariant fallback = effect.params.value(it->first);
        QVariantMap track = keyframesToTrack(it->second, fallback);
        trackDuration = std::max(trackDuration, AviQtl::Core::KeyframeUtils::inferredDurationForTrack(track));
        tracks.insert(it->first, track);
    }

    // Resolve every track once (normalize + flatten); subsequent per-frame
    // evaluations only walk the flattened list and apply easing.
    out.resolved = AviQtl::Core::KeyframeUtils::resolveAllTracks(out.params, tracks, trackDuration);
    out.numericTracks.reserve(static_cast<std::size_t>(out.resolved.size()));
    for (auto it = out.resolved.constBegin(); it != out.resolved.constEnd(); ++it) {
        const QVariant fallback = out.params.value(it.key());
        if (!isNumericValue(fallback))
            continue;
        std::optional<NumericTrackStorage> numericTrack = buildNumericTrack(it.value());
        if (!numericTrack)
            continue;
        out.numericIndices.insert(it.key(), out.numericTracks.size());
        out.numericTracks.push_back(std::move(*numericTrack));
    }
    out.numericViews.resize(out.numericTracks.size());
    out.numericOutput.resize(out.numericTracks.size());
    for (auto it = out.numericIndices.constBegin(); it != out.numericIndices.constEnd(); ++it) {
        const std::size_t index = *it;
        if (index >= out.numericTracks.size())
            continue;
        out.numericViews[index] = out.numericTracks[index].view(out.params.value(it.key()).toDouble());
    }
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

void bakeClipEffects(const AviQtl::Core::Clip &clip, const CachedClip &cachedClip, int currentFrame, double fps,
                      RenderComponent &render, EffectParamBuffer &paramBuf) {
    Q_UNUSED(fps);
    const int relFrame = std::max(0, currentFrame - clip.startFrame);
    const double relTime = static_cast<double>(relFrame);
    auto &registry = AviQtl::Core::EffectRegistry::instance();

    render.clipId = clip.id;
    render.layer = clip.layer;
    render.startFrame = clip.startFrame;
    render.durationFrames = clip.durationFrames;
    render.clipByUpperObject = clip.clipByUpperObject;
    render.timePosition = relTime;

    bool hasTransform = false;
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
            hasTransform = true;
            render.x = evalFloat(rt, QStringLiteral("x"), relFrame);
            render.y = evalFloat(rt, QStringLiteral("y"), relFrame);
            render.z = evalFloat(rt, QStringLiteral("z"), relFrame);
            render.rotX = evalFloat(rt, QStringLiteral("rotationX"), relFrame);
            render.rotY = evalFloat(rt, QStringLiteral("rotationY"), relFrame);
            render.rotZ = evalFloat(rt, QStringLiteral("rotationZ"), relFrame);
            const float scale = evalFloat(rt, QStringLiteral("scale"), relFrame);
            render.scaleX = scale * 0.01f;
            render.scaleY = scale * 0.01f;
            render.opacity = evalFloat(rt, QStringLiteral("opacity"), relFrame);
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

    render.effectCount = effectIdx;

    if (!hasTransform) {
        render.x = 0;
        render.y = 0;
        render.z = 0;
        render.rotX = 0;
        render.rotY = 0;
        render.rotZ = 0;
        render.scaleX = 1;
        render.scaleY = 1;
        render.opacity = 1;
    }
}

AudioComponent bakeAudioState(const AviQtl::Core::Clip &clip, const CachedClip &cachedClip, int currentFrame, double fps) {
    if (fps <= 0.0) {
        return {};
    }

    const int relFrame = std::max(0, currentFrame - clip.startFrame);

    AudioComponent audio;
    audio.clipId = clip.id;
    audio.startFrame = clip.startFrame;
    audio.durationFrames = clip.durationFrames;
    auto it = std::find_if(clip.effects.begin(), clip.effects.end(),
        [](const auto &e) { return e.enabled && e.id == QStringLiteral("audio"); });
    if (it != clip.effects.end()) {
        const auto &effect = *it;
        const std::size_t effectIndex = static_cast<std::size_t>(std::distance(clip.effects.begin(), it));
        if (effectIndex >= cachedClip.effects.size())
            return audio;
        const ResolvedTracks &rt = cachedClip.effects[effectIndex].tracks;

        const QString playMode = effect.params.value(QStringLiteral("playMode")).toString();
        audio.directMode = AviQtl::Core::MediaUtils::isDirectAudioMode(playMode);
        audio.sourceStartTime = std::max(0.0f, evalFloatOr(rt, QStringLiteral("startTime"), 0.0f, relFrame));
        audio.playbackSpeed = std::max(0.0f, evalFloatOr(rt, QStringLiteral("speed"), AviQtl::kDefaultSpeed, relFrame) / static_cast<float>(AviQtl::kDefaultSpeed));
        audio.directTime = std::max(0.0f, evalFloatOr(rt, QStringLiteral("directTime"), 0.0f, relFrame));
        audio.volume = std::max(0.0f, evalFloatOr(rt, QStringLiteral("volume"), 1.0f, relFrame));
        audio.masterVolume = std::max(0.0f, evalFloatOr(rt, QStringLiteral("masterVolume"), 1.0f, relFrame));
        audio.pan = std::clamp(evalFloatOr(rt, QStringLiteral("pan"), 0.0f, relFrame), -1.0f, 1.0f);
        audio.fadeInSec = std::max(0.0f, evalFloatOr(rt, QStringLiteral("fadeIn"), 0.0f, relFrame));
        audio.fadeOutSec = std::max(0.0f, evalFloatOr(rt, QStringLiteral("fadeOut"), 0.0f, relFrame));
        audio.mute = effect.params.value(QStringLiteral("mute"), false).toBool();
        audio.solo = effect.params.value(QStringLiteral("solo"), false).toBool();
        audio.limiter = effect.params.value(QStringLiteral("limiter"), true).toBool();
    }

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
        ecs.updateClipState(clip.id, clip.layer, relTime, clip.startFrame, clip.durationFrames);

        if (clip.type == QStringLiteral("audio") || clip.type == QStringLiteral("video")) {
            const AudioComponent audio = bakeAudioState(clip, cachedClip, currentFrame, fps);
            ecs.updateAudioClipState(clip.id, audio);
        }

        RenderComponent render;
        render.effectStartIndex = static_cast<uint32_t>(ecs.editState().effectParams.entries.size());
        bakeClipEffects(clip, cachedClip, currentFrame, fps, render, ecs.editState().effectParams);
        ecs.updateRenderState(clip.id, render);
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
