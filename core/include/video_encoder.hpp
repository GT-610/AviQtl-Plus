#pragma once

#include "constants.hpp"
#include <QImage>
#include <QLoggingCategory>
#include <QMap>
#include <QObject>
#include <QSize>
#include <QString>
#include <QStringList>
#include <atomic>
#include <condition_variable>
#include <mutex>
#include <queue>

Q_DECLARE_LOGGING_CATEGORY(lcVideoEncoder)
#include <thread>
#include <vector>

// FFmpeg forward declarations to keep header clean
extern "C" {
struct AVFormatContext;
struct AVCodecContext;
struct AVStream;
struct AVFrame;
struct SwsContext;
struct SwrContext;
struct AVAudioFifo;
struct AVBufferRef;
}

namespace AviQtl::Core {

class VideoEncoder : public QObject {
    Q_OBJECT
  public:
    struct Config {
        int width;
        int height;
        int fps_num;
        int fps_den;
        int64_t bitrate = 15'000'000;
        int crf = -1;                                     // -1 = bitrate mode, 0~51 = CRF mode
        QString codecName = QStringLiteral("libx264");    // software fallback, works on all platforms
        QString audioCodecName = QStringLiteral("aac");
        int64_t audioBitrate = 192'000;
        QString outputUrl;
        int startFrame = 0;
        int endFrame = -1; // -1 = タイムライン末尾まで
    };

    explicit VideoEncoder(QObject *parent = nullptr);
    ~VideoEncoder() override;

    static QStringList availableVideoEncoders();
    static QStringList availableAudioEncoders();
    static QString fallbackEncoder(const QString &hwEncoder);

    bool open(const Config &config);
    bool pushFrame(const QImage &img, int64_t pts); // CPU -> HW Upload
    bool addAudioStream(int sampleRate = AviQtl::kDefaultSampleRate, int channels = 2);
    bool pushAudio(const float *samples, int sampleCount);
    void close();

  private:
    Config m_config;
    AVFormatContext *m_fmtCtx = nullptr;
    AVCodecContext *m_encCtx = nullptr;
    AVStream *m_stream = nullptr;
    AVFrame *m_hwFrame = nullptr; // For VA-API
    AVFrame *m_swFrame = nullptr; // For CPU staging
    SwsContext *m_swsCtx = nullptr;
    int m_swsSrcFmt = -1; // AVPixelFormat cached source format (-1 = none)
    AVBufferRef *m_hwDeviceCtx = nullptr;

    // Audio
    AVStream *m_audioStream = nullptr;
    AVCodecContext *m_audioEncCtx = nullptr;
    SwrContext *m_swrCtx = nullptr;
    AVAudioFifo *m_audioFifo = nullptr;
    AVFrame *m_audioFrame = nullptr;
    int64_t m_encodedFrameCount = 0;
    int64_t m_audioPts = 0;
    bool m_headerWritten = false;
    std::mutex m_mutex;

    // Async Encoding Support
    struct EncodeTask {
        enum Type { Video, Audio } type;
        QImage videoImg;
        int64_t videoPts = 0;
        std::vector<float> audioSamples;
    };

    void encodingLoop();
    bool processVideo(const QImage &img, int64_t pts);
    bool processAudio(const std::vector<float> &samples);

    std::thread m_workerThread;
    std::queue<EncodeTask> m_taskQueue;
    std::mutex m_queueMutex;
    std::condition_variable m_queueCv;
    std::condition_variable m_queuePushCv;
    std::atomic<bool> m_stopEncoding{false};
    std::atomic<bool> m_errorOccurred{false};

    bool initHardware(const QString &codecName);
    bool writeHeaderIfNeeded();
    void cleanup();
};

} // namespace AviQtl::Core