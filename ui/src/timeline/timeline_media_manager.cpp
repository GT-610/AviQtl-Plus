#include "timeline_media_manager.hpp"
#include "audio_decoder.hpp"
#include "constants.hpp"
#include "core/include/media_utils.hpp"
#include "effect_registry.hpp"
#include "engine/audio_mixer.hpp"
#include "image_decoder.hpp"
#include "media_decoder.hpp"
#include "timeline_controller.hpp"
#include "video_decoder.hpp"
#include "video_frame_store.hpp"
#include "engine/plugin/audio_plugin_manager.hpp"
#include <algorithm>
#include <cmath>

namespace AviQtl::UI {

TimelineMediaManager::TimelineMediaManager(TimelineController *controller, QObject *parent) : QObject(parent), m_controller(controller), m_audioMixer(new AviQtl::Engine::AudioMixer(this)) {}

void TimelineMediaManager::setVideoFrameStore(AviQtl::Core::VideoFrameStore *store) {
    m_videoFrameStore = store;
    // Force a rebuild on next updateMediaDecoders() since video/image
    // decoders depend on the store and may have been skipped earlier.
    m_decoderFingerprint = 0;
    updateMediaDecoders();
}

void TimelineMediaManager::onPlayingChanged() {
    bool playing = m_controller->transport()->isPlaying();
    for (const auto &decoder : std::as_const(m_decoders)) {
        if (!decoder) {
            continue;
        }
        decoder->setPlaying(playing);
    }
}

void TimelineMediaManager::onCurrentFrameChanged() {
    if (!m_controller || !m_audioMixer) {
        return;
    }
    auto *transport = m_controller->transport();
    auto *project = m_controller->project();
    auto *timeline = m_controller->timeline();
    if (!transport || !project || !timeline) {
        return;
    }

    int nextFrame = transport->currentFrame();
    double fps = project->fps();
    if (transport->isPlaying()) {
        int sampleRate = project->sampleRate();
        m_audioMixer->processFrame(nextFrame, fps, static_cast<int>(std::round(static_cast<double>(sampleRate) / fps)));
    }

    for (auto it = m_decoders.begin(); it != m_decoders.end(); ++it) {
        const auto *clip = timeline->findClipById(it.key());
        if ((clip == nullptr) || nextFrame < clip->startFrame || nextFrame >= clip->startFrame + clip->durationFrames) {
            continue;
        }

        if (auto *vid = qobject_cast<AviQtl::Core::VideoDecoder *>(it.value())) {
            updateVideoClipFrame(vid, clip, nextFrame - clip->startFrame);
        }

        if (auto *img = qobject_cast<AviQtl::Core::ImageDecoder *>(it.value())) {
            img->seek(0); // 描画を強制
        }

        if (auto *aud = qobject_cast<AviQtl::Core::AudioDecoder *>(it.value())) {
            const int relFrame = nextFrame - clip->startFrame;
            const double relTime = static_cast<double>(relFrame) / fps;
            double audioTime = 0.0;

            for (const auto *eff : clip->effects) {
                if (eff->id() != QStringLiteral("audio")) {
                    continue;
                }

                const QString playMode = eff->params().value(QStringLiteral("playMode"), "開始時間＋再生速度").toString();
                const bool isDirect = AviQtl::Core::MediaUtils::isDirectAudioMode(playMode);
                const double directTime = eff->evaluatedParam(QStringLiteral("directTime"), relFrame, fps).toDouble();
                const double startTime = eff->evaluatedParam(QStringLiteral("startTime"), relFrame, fps).toDouble();
                const QString source = eff->params().value(QStringLiteral("source")).toString();
                const bool sourceIsVideo = AviQtl::Core::MediaUtils::isVideoFile(source);
                const bool linkedVideo = sourceIsVideo && eff->evaluatedParam(QStringLiteral("linkedVideo"), relFrame, fps).toBool();
                const double speed = linkedVideo ? AviQtl::kDefaultSpeed : eff->evaluatedParam(QStringLiteral("speed"), relFrame, fps).toDouble();

                audioTime = AviQtl::Core::MediaUtils::resolveAudioTime(relTime, isDirect, directTime, startTime, speed);
                break;
            }
            aud->seek(static_cast<qint64>(audioTime * 1000.0));
        }
    }
}

void TimelineMediaManager::syncPlaybackSpeed() {
    double speed = m_controller->transport()->playbackSpeed();
    for (const auto &decoder : std::as_const(m_decoders)) {
        if (!decoder) {
            continue;
        }
        decoder->setPlaybackRate(speed);
    }
    if (m_audioMixer) {
        m_audioMixer->setPlaybackSpeed(speed);
    }
}

void TimelineMediaManager::updateAudioSampleRate() {
    int rate = m_controller->project()->sampleRate();
    if (m_audioMixer) {
        m_audioMixer->setSampleRate(rate);
    }
    for (const auto &decoder : std::as_const(m_decoders)) {
        if (!decoder) {
            continue;
        }
        decoder->setSampleRate(rate);
    }
}

auto TimelineMediaManager::getClipSourceUrl(const ClipData &clip) -> QUrl {
    const EffectModel *effModel = nullptr;
    for (const auto *eff : std::as_const(clip.effects)) {
        if (eff->id() == clip.type) {
            effModel = eff;
            break;
        }
    }
    if (effModel == nullptr) {
        return {};
    }
    // 音声以外は通常 "path" パラメータにファイルパスが入っている
    const bool audioLike = clip.type == QStringLiteral("audio");
    QString path = effModel->params().value(audioLike ? QLatin1String("source") : QLatin1String("path")).toString();
    return QUrl::fromLocalFile(path);
}

void TimelineMediaManager::removeDecoder(int clipId) {
    auto it = m_decoders.find(clipId);
    if (it == m_decoders.end()) {
        return;
    }

    auto decoder = it.value();
    if (qobject_cast<AviQtl::Core::AudioDecoder *>(decoder) != nullptr && m_audioMixer != nullptr) {
        m_audioMixer->unregisterDecoder(clipId);
    }
    if (m_videoFrameStore != nullptr) {
        m_videoFrameStore->invalidateFrame(QString::number(clipId));
    }
    if (decoder != nullptr) {
        decoder->deleteLater();
    }
    m_decoders.erase(it);
}

void TimelineMediaManager::registerDecoder(int clipId, AviQtl::Core::MediaDecoder *decoder) {
    if (decoder == nullptr) {
        return;
    }
    m_decoders.insert(clipId, decoder);
    connect(decoder, &AviQtl::Core::MediaDecoder::ready, this, [this, clipId]() -> void { emit frameUpdated(clipId); });
}

void TimelineMediaManager::updateMediaDecoders() {
    // 巨大な QList<ClipData> のコピー作成を避け、元のデータ構造を直接走査する
    const auto &scenes = m_controller->timeline()->getAllScenes();
    QSet<int> currentClipIds;
    QHash<int, int> clipToScene;

    // Compute a fingerprint over the (sceneId, clipId, type, source) tuples
    // that determine which decoders to instantiate. If nothing relevant
    // changed since the last call, skip the rebuild walk entirely. This
    // makes clipsChanged() effectively free for non-structural edits.
    quint64 fingerprint = 1469598103934665603ULL; // FNV-1a offset basis
    auto mix = [&fingerprint](quint64 v) {
        fingerprint ^= v;
        fingerprint *= 1099511628211ULL;
    };
    for (const auto &scene : std::as_const(scenes)) {
        for (const auto &clip : std::as_const(scene.clips)) {
            if (clip.type != QStringLiteral("video") && clip.type != QStringLiteral("audio") && clip.type != QStringLiteral("image")) {
                continue;
            }
            mix(static_cast<quint64>(scene.id));
            mix(static_cast<quint64>(clip.id));
            mix(static_cast<quint64>(qHash(clip.type)));
            mix(static_cast<quint64>(qHash(getClipSourceUrl(clip))));
        }
    }
    if (fingerprint == m_decoderFingerprint) {
        return;
    }
    m_decoderFingerprint = fingerprint;

    for (const auto &scene : std::as_const(scenes)) {
        for (const auto &clip : std::as_const(scene.clips)) {
            if (clip.type != QStringLiteral("video") && clip.type != QStringLiteral("audio") && clip.type != QStringLiteral("image")) {
                continue;
            }

            currentClipIds.insert(clip.id);
            clipToScene.insert(clip.id, scene.id);

            QUrl sourceUrl = getClipSourceUrl(clip);
            if (!sourceUrl.isValid() || sourceUrl.isEmpty()) {
                removeDecoder(clip.id);
                continue;
            }

            auto itExisting = m_decoders.find(clip.id);
            if (itExisting != m_decoders.end()) {
                AviQtl::Core::MediaDecoder *existingDecoder = itExisting.value();
                if (existingDecoder != nullptr) {
                    // If the source has changed, we must recreate the decoder
                    if (existingDecoder->source() != sourceUrl) {
                        removeDecoder(clip.id);
                    } else {
                        continue;
                    }
                } else {
                    m_decoders.erase(itExisting);
                }
            }

            AviQtl::Core::MediaDecoder *decoder = nullptr;
            if (clip.type == QStringLiteral("video")) {
                if (m_videoFrameStore == nullptr) {
                    continue;
                }
                decoder = new AviQtl::Core::VideoDecoder(clip.id, sourceUrl, m_videoFrameStore, this);
            } else if (clip.type == QStringLiteral("image")) {
                if (m_videoFrameStore == nullptr) {
                    continue;
                }
                decoder = new AviQtl::Core::ImageDecoder(clip.id, sourceUrl, m_videoFrameStore, this);
            } else if (clip.type == QStringLiteral("audio")) {
                decoder = new AviQtl::Core::AudioDecoder(clip.id, sourceUrl, this);
                if (auto *audioDecoder = qobject_cast<AviQtl::Core::AudioDecoder *>(decoder)) {
                    m_audioMixer->registerDecoder(clip.id, audioDecoder);
                }
            }

            if (decoder != nullptr) {
                registerDecoder(clip.id, decoder);
                if (clip.type == QStringLiteral("audio")) {
                    syncAudioPluginChain(clip);
                }
                int cid = clip.id;

                if (auto *vid = qobject_cast<AviQtl::Core::VideoDecoder *>(decoder)) {
                    connect(decoder, &AviQtl::Core::MediaDecoder::frameReady, this, [this, cid](int) -> void { emit frameUpdated(cid); });
                    connect(vid, &AviQtl::Core::VideoDecoder::videoMetaReady, this, [this, cid](int totalFrameCount, double sourceFps) -> void {
                        const auto *clip = m_controller->timeline()->findClipById(cid);
                        if (!clip || clip->type != QStringLiteral("video")) {
                            return;
                        }

                        int startVideoFrame = 0;
                        double speed = AviQtl::kDefaultSpeed;
                        for (const auto *eff : clip->effects) {
                            if (eff->id() != QStringLiteral("video")) {
                                continue;
                            }
                            const QString playMode = eff->params().value(QStringLiteral("playMode"), "開始フレーム＋再生速度").toString();
                            if (playMode == QStringLiteral("フレーム直接指定")) {
                                return;
                            }
                            startVideoFrame = eff->params().value(QStringLiteral("startFrame"), 0).toInt();
                            speed = eff->params().value(QStringLiteral("speed"), AviQtl::kDefaultSpeed).toDouble();
                            break;
                        }

                        const int projectFps = static_cast<int>(m_controller->project()->fps());
                        const int maxDuration = AviQtl::Core::MediaUtils::maxVideoDurationFrames(
                            totalFrameCount, sourceFps, speed, startVideoFrame, projectFps);
                        if (maxDuration > 0 && clip->durationFrames > maxDuration) {
                            m_controller->updateClip(clip->id, clip->layer, clip->startFrame, maxDuration);
                        }
                    });
                }
                decoder->scheduleStart(); // 非同期起動
            }
        }
    }

    const QList<int> decoderIds = m_decoders.keys();
    for (int clipId : decoderIds) {
        if (!currentClipIds.contains(clipId)) {
            removeDecoder(clipId);
        }
    }
}

void TimelineMediaManager::syncAudioPluginChain(int clipId) {
    const auto *clip = m_controller->timeline()->findClipById(clipId);
    if (clip != nullptr) {
        syncAudioPluginChain(*clip);
    }
}

void TimelineMediaManager::syncAudioPluginChains() {
    const auto &scenes = m_controller->timeline()->getAllScenes();
    for (const auto &scene : std::as_const(scenes)) {
        for (const auto &clip : std::as_const(scene.clips)) {
            syncAudioPluginChain(clip);
        }
    }
}

void TimelineMediaManager::syncAudioPluginChain(const ClipData &clip) {
    if (clip.type != QStringLiteral("audio")) {
        return;
    }

    auto *audioMixer = m_audioMixer.data();
    if (audioMixer == nullptr) {
        return;
    }

    auto chain = audioMixer->getChain(clip.id);
    chain->clear();
    for (const auto &pluginState : clip.audioPlugins) {
        auto plugin = AviQtl::Engine::Plugin::AudioPluginManager::instance().createPlugin(pluginState.id);
        if (!plugin) {
            qWarning() << "Failed to restore audio plugin:" << pluginState.id << "for clip" << clip.id;
            continue;
        }
        for (auto it = pluginState.params.cbegin(); it != pluginState.params.cend(); ++it) {
            bool ok = false;
            const int paramIndex = it.key().toInt(&ok);
            if (ok) {
                plugin->setParam(paramIndex, it.value().toFloat());
            }
        }
        chain->add(std::move(plugin), pluginState.enabled);
    }
}

void TimelineMediaManager::updateVideoClipFrame(AviQtl::Core::VideoDecoder *vid, const ClipData *clip, int relFrame) {
    if ((vid == nullptr) || (clip == nullptr) || (m_controller == nullptr) || (m_controller->project() == nullptr)) {
        return;
    }

    relFrame = std::max(relFrame, 0);
    const double fps = [&]() -> double {
        const double f = m_controller->project()->fps();
        return f > 0.0 ? f : 30.0;
    }();

    for (const auto *eff : clip->effects) {
        if ((eff == nullptr) || eff->id() != QStringLiteral("video")) {
            continue;
        }

        const QString playMode = eff->params().value(QStringLiteral("playMode"), "開始フレーム＋再生速度").toString();
        const bool isDirect = (playMode == QStringLiteral("フレーム直接指定"));

        if (isDirect) {
            const int absFrame = eff->evaluatedParam(QStringLiteral("directFrame"), relFrame, fps).toInt();
            vid->seekToFrame(absFrame, vid->sourceFps());
        } else {
            const int startFrame = eff->evaluatedParam(QStringLiteral("startFrame"), 0, fps).toInt();
            const double speed = eff->evaluatedParam(QStringLiteral("speed"), 100, fps).toDouble();

            double vfps = vid->sourceFps();
            if (vfps <= 0.0) {
                vfps = fps;
            }

            const double targetSec = AviQtl::Core::MediaUtils::resolveVideoTime(relFrame, vfps, false, 0.0, startFrame, speed);
            vid->seekToTime(targetSec);
        }
        return;
    }
}

void TimelineMediaManager::requestVideoFrame(int clipId, int relFrame) { // NOLINT(bugprone-easily-swappable-parameters)
    if ((m_controller == nullptr) || (m_controller->timeline() == nullptr)) {
        return;
    }

    const ClipData *targetClip = m_controller->timeline()->findClipById(clipId);
    if (targetClip == nullptr) {
        return;
    }

    auto *vid = qobject_cast<AviQtl::Core::VideoDecoder *>(decoderForClip(clipId));
    if (vid == nullptr) {
        return;
    }

    updateVideoClipFrame(vid, targetClip, relFrame);
}

void TimelineMediaManager::requestImageLoad(int clipId, const QString &path) {
    if ((m_videoFrameStore == nullptr) || path.isEmpty() || clipId <= 0) {
        return;
    }

    const QUrl url = QUrl::fromLocalFile(path);

    if (auto *existing = qobject_cast<AviQtl::Core::ImageDecoder *>(decoderForClip(clipId))) {
        if (existing->source() == url) {
            existing->seek(0);
            return;
        }
    }

    removeDecoder(clipId);
    auto *decoder = new AviQtl::Core::ImageDecoder(clipId, url, m_videoFrameStore, this);
    registerDecoder(clipId, decoder);
    decoder->load();
}

} // namespace AviQtl::UI
