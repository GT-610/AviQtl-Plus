#pragma once
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

    // 全てのデコーダーが読み込み完了しているか確認
    bool isReady() const;

    void processFrame(int currentFrame, double fps, int samplesPerFrame);
    void reset();

    // エクスポート用に生データを取得するメソッド
    std::vector<float> mix(int currentFrame, double fps, int samplesPerFrame, std::optional<double> playbackSpeed = std::nullopt);

    // クリップID → プラグインチェーン
    std::shared_ptr<Plugin::AudioPluginChain> getChain(int clipId);
    void clearChain(int clipId);

    void setPlaybackSpeed(double speed);
    void setSampleRate(int sampleRate);

  signals:
    void audioMeterChanged(int clipId, float peakLeft, float peakRight, float rmsLeft, float rmsRight);

  private:
    void fetchRawSamples(AviQtl::Core::AudioDecoder *decoder, double startTime, int sampleCount);

    std::unique_ptr<QAudioSink> m_audioSink;
    QIODevice *m_audioOutput = nullptr;
    QAudioFormat m_format;
    QHash<int, QPointer<AviQtl::Core::AudioDecoder>> m_decoders;
    QHash<int, std::shared_ptr<Plugin::AudioPluginChain>> m_chains;
    int m_lastFrame = -1;
    double m_playbackSpeed = 1.0;
    QHash<int, double> m_clipPhase;
    QHash<int, int> m_clipLastFrame;

    std::vector<float> m_masterBuffer;
    std::vector<float> m_clipSamples;
    std::vector<float> m_rawSamples;
    int m_lastSamplesPerFrame = 0;

    // Mutex to protect shared state between UI and audio threads
    mutable std::shared_mutex m_mutex;
};

} // namespace AviQtl::Engine
