#include "bake_controller.hpp"
#include "core/include/document_model.hpp"
#include "core/include/effect_registry.hpp"
#include "core/include/media_utils.hpp"
#include "core/include/settings_manager.hpp"
#include "ecs.hpp"
#include "keyframe_evaluator.hpp"
#include <algorithm>
#include <bitset>
#include <QSet>

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

float evalFloat(const QVariantMap &params, const QVariantMap &tracks,
                const QString &key, int frame, double fps, int duration) {
    QVariant v = evaluateParam(params, tracks, key, frame, fps, duration);
    return static_cast<float>(v.toDouble());
}

float evalFloatOr(const QVariantMap &params, const QVariantMap &tracks,
                  const QString &key, float fallback, int frame, double fps, int duration) {
    if (!params.contains(key) && !tracks.contains(key)) {
        return fallback;
    }
    return evalFloat(params, tracks, key, frame, fps, duration);
}

void bakeClipEffects(const AviQtl::Core::Clip &clip, int currentFrame, double fps,
                     RenderComponent &render, EffectParamBuffer &paramBuf) {
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

    for (const auto &effect : clip.effects) {
        if (!effect.enabled) {
            ++effectIdx;
            continue;
        }

        const auto &meta = registry.getEffect(effect.id);
        if (meta.id.isEmpty()) {
            ++effectIdx;
            continue;
        }

        QVariantMap tracks;
        int trackDuration = clip.durationFrames;
        QSet<QString> allKeys;
        for (auto it = effect.params.constBegin(); it != effect.params.constEnd(); ++it)
            allKeys.insert(it.key());
        for (auto it = effect.keyframes.begin(); it != effect.keyframes.end(); ++it)
            allKeys.insert(it->first);
        for (const auto &key : std::as_const(allKeys)) {
            const QVariant fallback = effect.params.value(key);
            auto kfIt = effect.keyframes.find(key);
            if (kfIt != effect.keyframes.end()) {
                tracks[key] = keyframesToTrack(kfIt->second, fallback);
                trackDuration = std::max(trackDuration, inferredDurationForTrack(tracks[key]));
            }
        }

        if (effect.id == QStringLiteral("transform")) {
            hasTransform = true;
            render.x = evalFloat(effect.params, tracks, QStringLiteral("x"), relFrame, fps, trackDuration);
            render.y = evalFloat(effect.params, tracks, QStringLiteral("y"), relFrame, fps, trackDuration);
            render.z = evalFloat(effect.params, tracks, QStringLiteral("z"), relFrame, fps, trackDuration);
            render.rotX = evalFloat(effect.params, tracks, QStringLiteral("rotationX"), relFrame, fps, trackDuration);
            render.rotY = evalFloat(effect.params, tracks, QStringLiteral("rotationY"), relFrame, fps, trackDuration);
            render.rotZ = evalFloat(effect.params, tracks, QStringLiteral("rotationZ"), relFrame, fps, trackDuration);
            const float scale = evalFloat(effect.params, tracks, QStringLiteral("scale"), relFrame, fps, trackDuration);
            render.scaleX = scale * 0.01f;
            render.scaleY = scale * 0.01f;
            render.opacity = evalFloat(effect.params, tracks, QStringLiteral("opacity"), relFrame, fps, trackDuration);
        }

        for (const auto &key : std::as_const(allKeys)) {
            EffectParamEntry entry;
            entry.clipId = static_cast<uint32_t>(clip.id);
            entry.effectIndex = effectIdx;

            const QByteArray nameBytes = key.toUtf8();
            const auto copyLen = static_cast<std::size_t>(std::min<qsizetype>(nameBytes.size(), 19));
            std::memcpy(entry.paramName, nameBytes.constData(), copyLen);
            entry.paramName[copyLen] = '\0';

            QVariant evaluated = evaluateParam(effect.params, tracks, key, relFrame, fps, trackDuration);

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
    render.effectStartIndex = 0;

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

AudioComponent bakeAudioState(const AviQtl::Core::Clip &clip, int currentFrame, double fps) {
    const int relFrame = std::max(0, currentFrame - clip.startFrame);

    AudioComponent audio;
    audio.clipId = clip.id;
    audio.startFrame = clip.startFrame;
    audio.durationFrames = clip.durationFrames;

    for (const auto &effect : clip.effects) {
        if (!effect.enabled || effect.id != QStringLiteral("audio")) {
            continue;
        }

        QVariantMap tracks;
        int trackDuration = clip.durationFrames;
        QSet<QString> allKeys;
        for (auto it = effect.params.constBegin(); it != effect.params.constEnd(); ++it) {
            allKeys.insert(it.key());
        }
        for (auto it = effect.keyframes.begin(); it != effect.keyframes.end(); ++it) {
            allKeys.insert(it->first);
        }
        for (const auto &key : std::as_const(allKeys)) {
            const QVariant fallback = effect.params.value(key);
            auto kfIt = effect.keyframes.find(key);
            if (kfIt != effect.keyframes.end()) {
                tracks[key] = keyframesToTrack(kfIt->second, fallback);
                trackDuration = std::max(trackDuration, inferredDurationForTrack(tracks[key]));
            }
        }

        const QString playMode = effect.params.value(QStringLiteral("playMode")).toString();
        audio.directMode = AviQtl::Core::MediaUtils::isDirectAudioMode(playMode);
        audio.sourceStartTime = std::max(0.0f, evalFloatOr(effect.params, tracks, QStringLiteral("startTime"), 0.0f, relFrame, fps, trackDuration));
        audio.playbackSpeed = std::max(0.0f, evalFloatOr(effect.params, tracks, QStringLiteral("speed"), 100.0f, relFrame, fps, trackDuration) / 100.0f);
        audio.directTime = std::max(0.0f, evalFloatOr(effect.params, tracks, QStringLiteral("directTime"), 0.0f, relFrame, fps, trackDuration));
        audio.volume = std::max(0.0f, evalFloatOr(effect.params, tracks, QStringLiteral("volume"), 1.0f, relFrame, fps, trackDuration));
        audio.pan = std::clamp(evalFloatOr(effect.params, tracks, QStringLiteral("pan"), 0.0f, relFrame, fps, trackDuration), -1.0f, 1.0f);
        audio.mute = effect.params.value(QStringLiteral("mute"), false).toBool();
        break;
    }

    return audio;
}

} // namespace

BakeController::BakeController() { connect(&AviQtl::Core::DocumentModel::instance(), &AviQtl::Core::DocumentModel::structureChanged, this, &BakeController::onStructureChanged); }

BakeController &BakeController::instance() {
    static BakeController inst;
    return inst;
}

void BakeController::bake(int sceneId, int currentFrame) {
    const auto *scene = AviQtl::Core::DocumentModel::instance().findScene(sceneId);
    if (!scene)
        return;

    auto &sm = AviQtl::Core::SettingsManager::instance();
    const QString strategy = sm.value(QStringLiteral("bakeStrategy"), QStringLiteral("FullBake")).toString();
    const int prefetch = sm.value(QStringLiteral("onDemandPrefetchFrames"), 30).toInt();
    const double fps = scene->fps;

    const bool isFullBake = (strategy == QStringLiteral("FullBake"));
    std::bitset<MAX_CLIP_ID> aliveFlags;

    auto &ecs = ECS::instance();
    ecs.clearEffectParams();

    for (const auto &clip : scene->clips) {
        if (clip.id < 0 || clip.id >= MAX_CLIP_ID)
            continue;

        bool shouldBake = false;
        if (isFullBake) {
            shouldBake = true;
        } else {
            const int start = clip.startFrame;
            const int end = clip.startFrame + clip.durationFrames;
            const int rangeStart = currentFrame - prefetch;
            const int rangeEnd = currentFrame + prefetch;
            if (start <= rangeEnd && end >= rangeStart) {
                shouldBake = true;
            }
        }

        if (shouldBake) {
            aliveFlags.set(static_cast<std::size_t>(clip.id));

            const double relTime = static_cast<double>(std::max(0, currentFrame - clip.startFrame));
            ecs.updateClipState(clip.id, clip.layer, relTime, clip.startFrame, clip.durationFrames);

            if (clip.type == QStringLiteral("audio") || clip.type == QStringLiteral("video")) {
                const AudioComponent audio = bakeAudioState(clip, currentFrame, fps);
                ecs.updateAudioClipState(clip.id, audio);
            }

            RenderComponent render;
            bakeClipEffects(clip, currentFrame, fps, render, ecs.editState().effectParams);
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
