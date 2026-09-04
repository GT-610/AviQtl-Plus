#pragma once
#include "core/include/rust_audio_dsp.hpp"
#include "plugin/audio_plugin_chain.hpp"
#include <QAudioFormat>
#include <QAudioSink>
#include <QHash>
#include <QIODevice>
#include <QLoggingCategory>
#include <QObject>
#include <QPointer>
#include <memory>
#include <mutex>
#include <optional>
#include <shared_mutex>
#include <unordered_map>
#include <vector>

Q_DECLARE_LOGGING_CATEGORY(lcAudioMixer)

namespace AviQtl::Core {
class AudioDecoder;
}

namespace AviQtl::Engine {

class AudioMixer : public QObject {
    Q_OBJECT
  public:
    explicit AudioMixer(QObject *parent = nullptr);
    ~AudioMixer() override;

    void registerDecoder(int clipId, AviQtl::Core::AudioDecoder *decoder);
    void unregisterDecoder(int clipId);

    void processFrame(int currentFrame, double fps, int samplesPerFrame);
    void reset();

    // エクスポート用に生データを取得するメソッド
    void mix(int currentFrame, double fps, int samplesPerFrame, std::vector<float> &output,
             std::optional<double> playbackSpeed = std::nullopt);

    // クリップID → プラグインチェーン
    std::shared_ptr<Plugin::AudioPluginChain> getChain(int clipId);
    void replaceChain(int clipId, std::shared_ptr<Plugin::AudioPluginChain> chain);

    void setPlaybackSpeed(double speed);
    void setSampleRate(int sampleRate);

  signals:
    void audioMeterChanged(int clipId, float peakLeft, float peakRight, float rmsLeft, float rmsRight);

  private:
    static void fetchRawSamples(AviQtl::Core::AudioDecoder *decoder, double startTime,
                                int sampleCount, std::vector<float> &output);

    std::unique_ptr<QAudioSink> m_audioSink;
    QIODevice *m_audioOutput = nullptr;
    QAudioFormat m_format;
    QHash<int, QPointer<AviQtl::Core::AudioDecoder>> m_decoders;
    QHash<int, std::shared_ptr<Plugin::AudioPluginChain>> m_chains;
    int m_lastFrame = -1;
    double m_playbackSpeed = 1.0;
    QHash<int, double> m_clipPhase;
    QHash<int, int> m_clipLastFrame;

    std::vector<float> m_playbackBuffer;
    std::vector<float> m_rawSamples;
    std::unordered_map<int, std::vector<float>> m_clipBuffers;
    std::vector<AviQtl::RustCore::AudioPlaybackInput> m_playbackInputs;
    std::vector<AviQtl::RustCore::AudioPlaybackPlan> m_playbackPlans;
    std::vector<AviQtl::RustCore::AudioBatchTrack> m_batchTracks;
    std::vector<AviQtl::RustCore::AudioBatchResult> m_batchResults;
    std::vector<std::uint8_t> m_batchReportMeters;
    // Mutex to protect shared state between UI and audio threads
    mutable std::shared_mutex m_mutex;
};

} // namespace AviQtl::Engine
