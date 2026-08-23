#include "audio_mixer.hpp"
#include "core/include/constants.hpp"
#include "core/include/audio_decoder.hpp"
#include "core/include/settings_manager.hpp"
#include "engine/timeline/ecs.hpp"
#include <QAudioFormat>
#include <QDebug>
#include <QLoggingCategory>
#include <QMediaDevices>
#include <algorithm>
#include <cmath>
#include <vector>

namespace AviQtl::Engine {

Q_LOGGING_CATEGORY(lcAudioMixer, "aviqtl.audio_mixer")

AudioMixer::AudioMixer(QObject *parent) : QObject(parent) {
    int sampleRate = AviQtl::Core::SettingsManager::instance().value(QStringLiteral("_runtime_projectSampleRate"), AviQtl::kDefaultSampleRate).toInt();
    m_format.setSampleRate(sampleRate);
    m_format.setChannelCount(2);
    m_format.setSampleFormat(QAudioFormat::Float);

    const auto state = Timeline::ECS::instance().getSnapshot();
    if (state != nullptr) {
        const auto &audioStates = state->audioStates;
        for (const auto &audio : audioStates) {
            if (!m_chains.contains(audio.clipId)) {
                m_chains.insert(audio.clipId, std::make_shared<Plugin::AudioPluginChain>(m_format.sampleRate(), AviQtl::kAudioMaxBlockSize));
            }
        }
    }

    // m_decoders is always empty at construction; registration happens externally
    QAudioDevice device = QMediaDevices::defaultAudioOutput();
    if (!device.isFormatSupported(m_format)) {
        qWarning() << "Default audio format not supported, using preferred format.";
        m_format = device.preferredFormat();
    }

    m_audioSink = std::make_unique<QAudioSink>(device, m_format);
    // 低レイテンシを目指しつつ、音飛びしない程度のバッファサイズ (例: 100ms)
    m_audioSink->setBufferSize(static_cast<qsizetype>(static_cast<std::size_t>(sampleRate) * 2 * sizeof(float) / 10));
    m_audioOutput = m_audioSink->start();
    if (m_audioOutput == nullptr) {
        qWarning() << "[AudioMixer] Failed to start audio output! Device:" << device.description();
    }
}

void AudioMixer::setPlaybackSpeed(double speed) {
    std::unique_lock lock(m_mutex);
    if (std::abs(m_playbackSpeed - speed) > 0.001) {
        m_playbackSpeed = speed;
        m_clipPhase.clear();
        m_clipLastFrame.clear();
        lock.unlock();
        reset();
    }
}

void AudioMixer::setSampleRate(int sampleRate) {
    std::unique_lock lock(m_mutex);
    if (m_format.sampleRate() == sampleRate) {
        return;
    }

    qCInfo(lcAudioMixer) << "Changing sample rate to" << sampleRate;
    m_format.setSampleRate(sampleRate);
    for (const auto &chain : std::as_const(m_chains)) {
        chain->prepare(sampleRate);
    }

    if (m_audioSink) {
        m_audioSink->stop();
    }

    QAudioDevice device = QMediaDevices::defaultAudioOutput();
    m_audioSink = std::make_unique<QAudioSink>(device, m_format);
    m_audioSink->setBufferSize(static_cast<qsizetype>(static_cast<std::size_t>(sampleRate) * 2 * sizeof(float) / 10));
    m_audioOutput = m_audioSink->start();
}

AudioMixer::~AudioMixer() {
    if (m_audioSink) {
        m_audioSink->stop();
    }
}

void AudioMixer::registerDecoder(int clipId, AviQtl::Core::AudioDecoder *decoder) {
    if (!decoder) {
        qWarning() << "[AudioMixer] Attempted to register null decoder for clip" << clipId;
        return;
    }
    std::unique_lock lock(m_mutex);
    m_decoders[clipId] = decoder;
}

void AudioMixer::unregisterDecoder(int clipId) {
    std::unique_lock lock(m_mutex);
    m_decoders.remove(clipId);
    m_clipBuffers.erase(clipId);
}

void AudioMixer::fetchRawSamples(AviQtl::Core::AudioDecoder *decoder, double startTime,
                                 int sampleCount, std::vector<float> &output) {
    output.resize(static_cast<std::size_t>(std::max(sampleCount, 0)));
    if (sampleCount > 0) {
        decoder->getSamplesInto(startTime, sampleCount, output.data());
    }
}

void AudioMixer::mix(int currentFrame, double fps, int samplesPerFrame, std::vector<float> &output,
                     std::optional<double> playbackSpeed) { // NOLINT(bugprone-easily-swappable-parameters)
    struct MeterUpdate {
        int clipId = -1;
        float peakLeft = 0.0F;
        float peakRight = 0.0F;
        float rmsLeft = 0.0F;
        float rmsRight = 0.0F;
    };
    std::vector<MeterUpdate> meterUpdates;

    // Protect the reusable buffers and per-clip playback state for the entire mix.
    std::unique_lock lock(m_mutex);
    const double mixerPlaybackSpeed = playbackSpeed.value_or(m_playbackSpeed);
    const std::size_t outputSize = static_cast<std::size_t>(std::max(samplesPerFrame, 0)) * 2;
    output.assign(outputSize, 0.0F);
    if (fps <= 0.0) {
        return;
    }
    m_batchTracks.clear();
    m_batchResults.clear();
    m_batchReportMeters.clear();

    const auto state = Timeline::ECS::instance().getSnapshot();
    if (state == nullptr) {
        return;
    }
    const auto &audioStates = state->audioStates;
    bool hasSolo = false;
    for (const auto &audio : audioStates) {
        if (audio.solo && !audio.mute && currentFrame >= audio.startFrame && currentFrame < audio.startFrame + audio.durationFrames) {
            hasSolo = true;
            break;
        }
    }

    for (const auto &audio : audioStates) {
        int clipId = audio.clipId;
        if (audio.mute || (hasSolo && !audio.solo)) {
            m_batchTracks.push_back({
                .samples = nullptr,
                .samples_length = 0,
                .parameters = {},
                .clip_id = clipId,
                .mute = audio.mute ? 1U : 0U,
                .solo = audio.solo ? 1U : 0U,
                .reserved = 0,
            });
            m_batchReportMeters.push_back(1U);
            continue;
        }
        if (currentFrame < audio.startFrame || currentFrame >= audio.startFrame + audio.durationFrames) {
            continue;
        }

        auto decIt = m_decoders.find(clipId);
        if (decIt == m_decoders.end() || decIt.value().isNull()) {
            m_batchTracks.push_back({
                .samples = nullptr,
                .samples_length = 0,
                .parameters = {},
                .clip_id = clipId,
                .mute = 0,
                .solo = audio.solo ? 1U : 0U,
                .reserved = 0,
            });
            m_batchReportMeters.push_back(0U);
            continue;
        }

        const double relTime = static_cast<double>(currentFrame - audio.startFrame) / fps;
        double startTime = audio.directMode ? static_cast<double>(audio.directTime) : static_cast<double>(audio.sourceStartTime) + (relTime * static_cast<double>(audio.playbackSpeed));
        const double sourceRate = std::max(0.0, mixerPlaybackSpeed * (audio.directMode ? 1.0 : static_cast<double>(audio.playbackSpeed)));
        auto lastFrameIt = m_clipLastFrame.find(clipId);
        if (!audio.directMode && lastFrameIt != m_clipLastFrame.end() && currentFrame == lastFrameIt.value() + 1) {
            auto phaseIt = m_clipPhase.find(clipId);
            if (phaseIt != m_clipPhase.end()) {
                startTime = phaseIt.value();
            }
        } else {
            // シークまたは初回再生時
            m_clipPhase[clipId] = startTime;
        }
        m_clipLastFrame[clipId] = currentFrame;

        auto *decoder = decIt.value().data();
        auto &clipSamples = m_clipBuffers[clipId];

        if (std::abs(sourceRate - 1.0) > 0.01) {
            // リサンプリングが必要な場合
            // 必要ソースサンプル数を計算（補間用に2サンプル余分に要求）
            int neededSamples = static_cast<int>(std::ceil(samplesPerFrame * sourceRate)) + 2;
            fetchRawSamples(decoder, startTime, neededSamples * 2, m_rawSamples); // Stereo

            clipSamples.resize(static_cast<std::size_t>(samplesPerFrame) * 2);
            const auto status = AviQtl::RustCore::resampleStereoLinear(m_rawSamples, clipSamples, sourceRate);
            if (status != AviQtl::RustCore::AudioStatus::Ok) {
                qCWarning(lcAudioMixer) << "Rust audio resampling failed with status"
                                        << static_cast<std::uint32_t>(status);
                std::fill(clipSamples.begin(), clipSamples.end(), 0.0F);
            }
            // 次のフレームのための開始位置を進める（m_playbackSpeed 分の秒数）
            m_clipPhase[clipId] = startTime + ((static_cast<double>(samplesPerFrame) / m_format.sampleRate()) * sourceRate);
        } else {
            // 1倍速の場合はそのまま取得
            int neededSamples = samplesPerFrame;
            fetchRawSamples(decoder, startTime, neededSamples * 2, clipSamples);
            m_clipPhase[clipId] = startTime + (static_cast<double>(samplesPerFrame) / m_format.sampleRate());
        }

        auto chainIt = m_chains.find(clipId);
        if (chainIt != m_chains.end()) {
            chainIt.value()->process(clipSamples.data(), samplesPerFrame);
        }

        const double clipDurationSec = fps > 0.0 ? static_cast<double>(audio.durationFrames) / fps : 0.0;
        m_batchTracks.push_back({
            .samples = clipSamples.data(),
            .samples_length = clipSamples.size(),
            .parameters = {
                .relative_time = relTime,
                .duration = clipDurationSec,
                .fade_in_seconds = audio.fadeInSec,
                .fade_out_seconds = audio.fadeOutSec,
                .volume = audio.volume,
                .master_volume = audio.masterVolume,
                .pan = audio.pan,
                .limiter = audio.limiter ? 1U : 0U,
            },
            .clip_id = clipId,
            .mute = 0,
            .solo = audio.solo ? 1U : 0U,
            .reserved = 0,
        });
        m_batchReportMeters.push_back(1U);
    }

    m_batchResults.resize(m_batchTracks.size());
    const auto mixStatus = AviQtl::RustCore::mixStereoBatch(
        m_batchTracks, output, m_batchResults);
    if (mixStatus != AviQtl::RustCore::AudioStatus::Ok) {
        qCWarning(lcAudioMixer) << "Rust audio batch mixing failed with status"
                                << static_cast<std::uint32_t>(mixStatus);
    }
    for (std::size_t index = 0; index < m_batchTracks.size(); ++index) {
        if (m_batchReportMeters[index] == 0U) {
            continue;
        }
        if (mixStatus == AviQtl::RustCore::AudioStatus::Ok && m_batchResults[index].mixed != 0) {
            const auto &meter = m_batchResults[index].meter;
            meterUpdates.push_back({
                .clipId = m_batchResults[index].clip_id,
                .peakLeft = meter.peak_left,
                .peakRight = meter.peak_right,
                .rmsLeft = meter.rms_left,
                .rmsRight = meter.rms_right,
            });
        } else {
            meterUpdates.push_back({.clipId = m_batchTracks[index].clip_id});
        }
    }
    m_batchTracks.clear();
    m_batchResults.clear();
    m_batchReportMeters.clear();
    lock.unlock();
    for (const MeterUpdate &update : meterUpdates) {
        emit audioMeterChanged(update.clipId, update.peakLeft, update.peakRight,
                               update.rmsLeft, update.rmsRight);
    }
}

void AudioMixer::processFrame(int currentFrame, double fps, int samplesPerFrame) { // NOLINT(bugprone-easily-swappable-parameters)
    if (m_audioOutput == nullptr) {
        return;
    }

    // 巻き戻し（ループ）検知: 前回のフレームより戻っていたらバッファをリセット
    if (m_lastFrame != -1 && currentFrame < m_lastFrame) {
        reset();
        if (m_audioOutput == nullptr) {
            return;
        }
    }
    m_lastFrame = currentFrame;

    int outputSamples = samplesPerFrame;
    double playbackSpeed = 1.0;
    {
        std::shared_lock lock(m_mutex);
        playbackSpeed = m_playbackSpeed;
    }
    if (playbackSpeed > 0.0) {
        outputSamples = static_cast<int>(std::clamp(samplesPerFrame / playbackSpeed, 1.0, static_cast<double>(samplesPerFrame) * 16.0));
    }

    mix(currentFrame, fps, outputSamples, m_playbackBuffer, playbackSpeed);
    m_audioOutput->write(reinterpret_cast<const char *>(m_playbackBuffer.data()),
                         static_cast<qint64>(m_playbackBuffer.size() * sizeof(float)));
}

void AudioMixer::reset() {
    if (m_audioSink) {
        m_audioSink->stop();
        m_audioSink->reset();
        m_audioOutput = m_audioSink->start();
    }
    std::unique_lock lock(m_mutex);
    m_clipPhase.clear();
    m_clipLastFrame.clear();
}

auto AudioMixer::getChain(int clipId) -> std::shared_ptr<Plugin::AudioPluginChain> {
    std::unique_lock lock(m_mutex);
    auto it = m_chains.find(clipId);
    if (it == m_chains.end()) {
        it = m_chains.insert(clipId, std::make_shared<Plugin::AudioPluginChain>(m_format.sampleRate(), AviQtl::kAudioMaxBlockSize));
    }
    return it.value();
}

void AudioMixer::replaceChain(int clipId, std::shared_ptr<Plugin::AudioPluginChain> chain) {
    std::unique_lock lock(m_mutex);
    m_chains.insert(clipId, std::move(chain));
}

} // namespace AviQtl::Engine
