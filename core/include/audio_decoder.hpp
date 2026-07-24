#pragma once

#include "media_decoder.hpp"
#include <QFuture>
#include <QHash>
#include <QList>
#include <QLoggingCategory>
#include <atomic>
#include <memory>
#include <mutex>
#include <vector>

Q_DECLARE_LOGGING_CATEGORY(lcAudioDecoder)

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/opt.h>
#include <libswresample/swresample.h>
}

namespace AviQtl::Core {

class AudioDecoder : public MediaDecoder {
    Q_OBJECT
  public:
    explicit AudioDecoder(int clipId, const QUrl &source, QObject *parent = nullptr);
    ~AudioDecoder() override;

    void setSampleRate(int sampleRate) override;
    void seek(qint64 ms) override;
    void setPlaying(bool playing) override;

    int getSamplesInto(double startTime, int count, float *out) override;
    std::vector<float> getPeaks(double startSec, double durationSec, int pixelWidth);
    double totalDurationSec() const;

    struct CacheStats {
        quint64 chunkHits = 0;
        quint64 chunkMisses = 0;
        quint64 decodedChunks = 0;
        quint64 chunkEvictions = 0;
        qsizetype chunkEntries = 0;
        qsizetype cachedSamples = 0;
        qsizetype maxChunkEntries = 0;
        qsizetype samplesPerChunk = 0;
        qsizetype peakLevels = 0;
        qsizetype peakEntries = 0;
        bool peakCacheComplete = false;
    };
    CacheStats cacheStats() const;

  protected:
    void startDecoding() override;

  private:
    void closeFFmpeg();
    void buildPeakCache();
    bool openFile();
    void rebuildSwrContext();

    // Chunk-based streaming decode
    struct AudioChunk {
        int64_t index = 0;
        std::vector<float> data; // interleaved stereo float32
        bool fullyDecoded = false;
    };

    bool decodeChunk(int64_t chunkIdx);
    bool ensureChunk(int64_t chunkIdx);
    void evictChunks();
    auto chunkStartSample(int64_t chunkIdx) const -> int64_t;
    auto chunkSampleCount() const -> int;

    static constexpr double kChunkDurationSec = 4.0;
    static constexpr int kMaxCachedChunks = 10;

    struct PeakEntry {
        float min;
        float max;
    };
    struct PeakLevel {
        int samplesPerEntry;
        std::vector<PeakEntry> peaks;
    };

    std::vector<PeakEntry> buildBasePeaks(const std::vector<float> &samples) const;
    std::vector<PeakEntry> silentBasePeaksForChunk(int64_t chunkIdx, double totalDurationSec) const;
    void rebuildPeakPyramidFromBase();

    // FFmpeg contexts (protected by m_ffmpegMutex during decode)
    AVFormatContext *m_fmtCtx = nullptr;
    AVCodecContext *m_decCtx = nullptr;
    AVStream *m_stream = nullptr;
    int m_streamIdx = -1;
    SwrContext *m_swrCtx = nullptr;
    AVFrame *m_frame = nullptr;
    AVPacket *m_pkt = nullptr;

    // Chunk cache (protected by m_mutex)
    QHash<int64_t, AudioChunk> m_chunkCache;
    QList<int64_t> m_chunkOrder;

    // Peak pyramid (protected by m_mutex)
    std::vector<PeakLevel> m_peakPyramid;

    // Full PCM for peak building only (transient, freed after buildPeakCache)
    std::shared_ptr<std::vector<float>> m_peakBuildData;

    double m_totalDurationSec = 0.0;
    QFuture<void> m_decodeFuture;
    QFuture<void> m_peakFuture;
    QFuture<void> m_prefetchFuture;
    std::atomic<bool> m_closing{false};
    std::atomic<int> m_peakGeneration{0};
    std::atomic<quint64> m_chunkHits{0};
    std::atomic<quint64> m_chunkMisses{0};
    std::atomic<quint64> m_decodedChunks{0};
    std::atomic<quint64> m_chunkEvictions{0};
    std::atomic<bool> m_peakCacheComplete{false};
    QString m_lastError;

    // Separate mutex for FFmpeg seek/decode operations
    QMutex m_ffmpegMutex;

    // Seek state
    std::atomic<qint64> m_seekTargetMs{-1};
};

} // namespace AviQtl::Core
