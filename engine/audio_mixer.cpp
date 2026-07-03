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

namespace {

auto fadeGainForTime(double relTime, double duration, float fadeInSec, float fadeOutSec) -> float {
    double gain = 1.0;
    if (fadeInSec > 0.0F) {
        gain = std::min(gain, std::clamp(relTime / static_cast<double>(fadeInSec), 0.0, 1.0));
    }
    if (fadeOutSec > 0.0F) {
        gain = std::min(gain, std::clamp((duration - relTime) / static_cast<double>(fadeOutSec), 0.0, 1.0));
    }
    return static_cast<float>(gain);
}

} // namespace

Q_LOGGING_CATEGORY(lcAudioMixer, "aviqtl.audio_mixer")

AudioMixer::AudioMixer(QObject *parent) : QObject(parent) {
    int sampleRate = AviQtl::Core::SettingsManager::instance().value(QStringLiteral("_runtime_projectSampleRate"), AviQtl::kDefaultSampleRate).toInt();
    m_format.setSampleRate(sampleRate);
    m_format.setChannelCount(2);
    m_format.setSampleFormat(QAudioFormat::Float);

    const auto *state = Timeline::ECS::instance().getSnapshot();
    if (state != nullptr) {
        const auto &audioStates = state->audioStates;
        for (const auto &audio : audioStates) {
            if (!m_chains.contains(audio.clipId)) {
                m_chains.insert(audio.clipId, std::make_shared<Plugin::AudioPluginChain>());
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
}

auto AudioMixer::isReady() const -> bool {
    std::shared_lock lock(m_mutex);
    for (auto it = m_decoders.constBegin(); it != m_decoders.constEnd(); ++it) {
        if (it.value().isNull() || !it.value()->isReady()) {
            return false;
        }
    }
    return true;
}

auto AudioMixer::mix(int currentFrame, double fps, int samplesPerFrame) -> const std::vector<float> & { // NOLINT(bugprone-easily-swappable-parameters)
    if (fps <= 0.0) {
        m_masterBuffer.assign(static_cast<std::size_t>(samplesPerFrame) * 2, 0.0F);
        m_lastSamplesPerFrame = samplesPerFrame;
        return m_masterBuffer;
    }
    std::size_t newSize = static_cast<std::size_t>(samplesPerFrame) * 2;
    if (newSize != static_cast<std::size_t>(m_lastSamplesPerFrame) * 2) {
        m_masterBuffer.assign(newSize, 0.0F);
        m_lastSamplesPerFrame = samplesPerFrame;
    } else {
        std::fill(m_masterBuffer.begin(), m_masterBuffer.end(), 0.0F);
    }
    auto &masterBuffer = m_masterBuffer;

    const auto *state = Timeline::ECS::instance().getSnapshot();
    if (state == nullptr) {
        return masterBuffer;
    }
    const auto &audioStates = state->audioStates;
    bool hasSolo = false;
    for (const auto &audio : audioStates) {
        if (audio.solo && !audio.mute && currentFrame >= audio.startFrame && currentFrame < audio.startFrame + audio.durationFrames) {
            hasSolo = true;
            break;
        }
    }

    // Take unique lock because we modify m_clipPhase and m_clipLastFrame
    std::unique_lock lock(m_mutex);

    for (const auto &audio : audioStates) {
        int clipId = audio.clipId;
        if (audio.mute || (hasSolo && !audio.solo)) {
            emit audioMeterChanged(clipId, 0.0f, 0.0f, 0.0f, 0.0f);
            continue;
        }
        auto decIt = m_decoders.find(clipId);
        if (decIt == m_decoders.end() || decIt.value().isNull()) {
            continue;
        }

        if (currentFrame < audio.startFrame || currentFrame >= audio.startFrame + audio.durationFrames) {
            continue;
        }

        const double relTime = static_cast<double>(currentFrame - audio.startFrame) / fps;
        double startTime = audio.directMode ? static_cast<double>(audio.directTime) : static_cast<double>(audio.sourceStartTime) + (relTime * static_cast<double>(audio.playbackSpeed));
        const double sourceRate = std::max(0.0, m_playbackSpeed * (audio.directMode ? 1.0 : static_cast<double>(audio.playbackSpeed)));
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

        if (std::abs(sourceRate - 1.0) > 0.01) {
            // リサンプリングが必要な場合
            // 必要ソースサンプル数を計算（補間用に2サンプル余分に要求）
            int neededSamples = static_cast<int>(std::ceil(samplesPerFrame * sourceRate)) + 2;
            m_rawSamples = decoder->getSamples(startTime, neededSamples * 2); // Stereo

            if (!m_rawSamples.empty()) {
                m_clipSamples.resize(static_cast<std::size_t>(samplesPerFrame) * 2);
                int availableSrcSamples = static_cast<int>(m_rawSamples.size() / 2);

                for (int i = 0; i < samplesPerFrame; ++i) {
                    double srcIdx = i * sourceRate;
                    int idx0 = static_cast<int>(srcIdx);
                    int idx1 = idx0 + 1;

                    // クランプして範囲外アクセス（SIGSEGV）を防止
                    if (idx0 >= availableSrcSamples) {
                        idx0 = availableSrcSamples - 1;
                    }
                    if (idx1 >= availableSrcSamples) {
                        idx1 = availableSrcSamples - 1;
                    }
                    idx0 = std::max(idx0, 0);
                    idx1 = std::max(idx1, 0);

                    double t = srcIdx - idx0;

                    // L ch
                    m_clipSamples[static_cast<std::size_t>(i) * 2] = static_cast<float>((m_rawSamples[static_cast<std::size_t>(idx0) * 2] * (1.0 - t)) + (m_rawSamples[static_cast<std::size_t>(idx1) * 2] * t));
                    // R ch
                    m_clipSamples[(static_cast<std::size_t>(i) * 2) + 1] = static_cast<float>((m_rawSamples[(static_cast<std::size_t>(idx0) * 2) + 1] * (1.0 - t)) + (m_rawSamples[(static_cast<std::size_t>(idx1) * 2) + 1] * t));
                }
            } else {
                m_clipSamples.assign(static_cast<std::size_t>(samplesPerFrame) * 2, 0.0F);
            }
            // 次のフレームのための開始位置を進める（m_playbackSpeed 分の秒数）
            m_clipPhase[clipId] = startTime + ((static_cast<double>(samplesPerFrame) / m_format.sampleRate()) * sourceRate);
        } else {
            // 1倍速の場合はそのまま取得
            int neededSamples = samplesPerFrame;
            m_rawSamples = decoder->getSamples(startTime, neededSamples * 2);
            m_clipSamples.swap(m_rawSamples);
            m_clipPhase[clipId] = startTime + (static_cast<double>(samplesPerFrame) / m_format.sampleRate());
        }

        auto chainIt = m_chains.find(clipId);
        if (chainIt != m_chains.end()) {
            chainIt.value()->process(m_clipSamples.data(), samplesPerFrame);
        }

        const double clipDurationSec = fps > 0.0 ? static_cast<double>(audio.durationFrames) / fps : 0.0;
        const float fadeGain = fadeGainForTime(relTime, clipDurationSec, audio.fadeInSec, audio.fadeOutSec);
        const float outputVolume = audio.volume * audio.masterVolume * fadeGain;
        float leftVol = outputVolume * (audio.pan <= 0 ? 1.0F : 1.0F - audio.pan);
        float rightVol = outputVolume * (audio.pan >= 0 ? 1.0F : 1.0F + audio.pan);
        float peakLeft = 0.0F;
        float peakRight = 0.0F;
        double squareLeft = 0.0;
        double squareRight = 0.0;

        for (size_t i = 0; i < m_clipSamples.size() && i < masterBuffer.size(); i += 2) {
            float left = m_clipSamples[i] * leftVol;
            if (audio.limiter) {
                left = std::clamp(left, -1.0F, 1.0F);
            }
            const float absLeft = std::abs(left);
            peakLeft = std::max(peakLeft, absLeft);
            squareLeft += static_cast<double>(left) * static_cast<double>(left);
            masterBuffer[i] += left;
            float right = m_clipSamples[i + 1] * rightVol;
            if (audio.limiter) {
                right = std::clamp(right, -1.0F, 1.0F);
            }
            const float absRight = std::abs(right);
            peakRight = std::max(peakRight, absRight);
            squareRight += static_cast<double>(right) * static_cast<double>(right);
            masterBuffer[i + 1] += right;
        }

        if (!m_clipSamples.empty()) {
            const auto frames = static_cast<double>(samplesPerFrame);
            const float rmsLeft = static_cast<float>(std::sqrt(squareLeft / frames));
            const float rmsRight = static_cast<float>(std::sqrt(squareRight / frames));
            emit audioMeterChanged(clipId, peakLeft, peakRight, rmsLeft, rmsRight);
        } else {
            emit audioMeterChanged(clipId, 0.0f, 0.0f, 0.0f, 0.0f);
        }
    }
    return masterBuffer;
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
    if (m_playbackSpeed > 0.0) {
        outputSamples = static_cast<int>(std::clamp(samplesPerFrame / m_playbackSpeed, 1.0, static_cast<double>(samplesPerFrame) * 16.0));
    }

    std::vector<float> buffer = mix(currentFrame, fps, outputSamples);
    m_audioOutput->write(reinterpret_cast<const char *>(buffer.data()), static_cast<qint64>(buffer.size() * sizeof(float)));
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
        it = m_chains.insert(clipId, std::make_shared<Plugin::AudioPluginChain>());
    }
    return it.value();
}

void AudioMixer::clearChain(int clipId) {
    std::unique_lock lock(m_mutex);
    m_chains.remove(clipId);
}

} // namespace AviQtl::Engine
