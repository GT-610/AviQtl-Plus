#pragma once
#include "ffmpeg_video_buffer.hpp"
#include "media_decoder.hpp"
#include <QCache>
#include <QFuture>
#include <QVideoFrame>

extern "C" {
struct AVFormatContext;
struct AVCodecContext;
struct AVStream;
struct AVFrame;
struct AVPacket;
struct AVBufferRef;
struct SwsContext;
}
#include <libavutil/pixfmt.h>
#include <libavutil/rational.h>

namespace AviQtl::Core {

class VideoFrameStore;

class VideoDecoder : public AviQtl::Core::MediaDecoder {
    Q_OBJECT
  public:
    explicit VideoDecoder(int clipId, const QUrl &source, VideoFrameStore *store, QObject *parent = nullptr);
    ~VideoDecoder() override;

  public:
    void seekToFrame(int frame, double fps);
    void seekToTime(double seconds);
    double sourceFps() const;
    int totalFrameCount() const;
    void seek(qint64 ms) override;
    void setPlaying(bool playing) override;

  signals:
    void videoMetaReady(int totalFrameCount, double sourceFps);

  protected:
    void startDecoding() override;

  private:
    bool buildIndex();
    int frameIndexFromSeconds(double seconds) const;

    struct FrameIndexEntry {
        int64_t pts;
        int64_t dts;
        bool isKeyframe;
    };

    void decodeTask(int targetFrame, double fps);
    bool tryCacheHit(int targetFrame, const QString &clipKey);
    bool open(const QString &path);
    int findGopEndIndex(int startFrame) const;
    void close();
    void updateCacheSize();

    VideoFrameStore *m_store = nullptr;

    // MLT風：GOP単位のリングバッファ
    struct GopCacheBlock {
        int keyframeIndex = -1;
        int startFrame = -1;
        int endFrame = -1;
        QHash<int, QVideoFrame> frames;
    };
    static constexpr int MAX_GOP_CACHE_SIZE = 3;
    std::mutex m_gopCacheMutex;

    AVFormatContext *m_fmtCtx = nullptr;
    AVCodecContext *m_decCtx = nullptr;
    AVStream *m_stream = nullptr;
    int m_streamIndex = -1;
    AVFrame *m_frame = nullptr;
    SwsContext *m_swsCtx = nullptr;
    AVBufferRef *m_hwDeviceCtx = nullptr;
    int m_hwPixFmt = -1; // AV_PIX_FMT_NONE

    int m_lastDecodedFrame = -1;
    std::vector<FrameIndexEntry> m_index;
    std::vector<int> m_prevKeyframe; ///< m_prevKeyframe[i] = index of the closest keyframe before or at frame i
    QCache<int, QVideoFrame> m_frameCache;
    std::atomic<int> m_lastRequestedFrame{-1};
    QVideoFrame m_lastGoodFrame; ///< MLT-style last valid frame for error concealment
    std::atomic<bool> m_closing{false};
    std::atomic<bool> m_isPlaying{false};

    int m_gopCacheCount = 0;
    GopCacheBlock m_gopCacheA[MAX_GOP_CACHE_SIZE];
    GopCacheBlock m_gopCacheB[MAX_GOP_CACHE_SIZE];
    GopCacheBlock *m_currentGopCache = m_gopCacheA;
    bool getFrameFromGopCache(int frameIndex, QVideoFrame &outFrame);
    void putGopCacheBlock(GopCacheBlock &&block);

    std::atomic<bool> m_isDecoding{false};
    QFuture<void> m_initFuture;
    QFuture<void> m_decodeFuture;
    double m_sourceFps = 0.0;
    AVRational m_timeBase{0, 1};
    int m_displayRotationCcw = 0;

    AVPacket *m_pkt = nullptr;

    static enum AVPixelFormat getHwFormat(AVCodecContext *ctx, const enum AVPixelFormat *pixfmts);
};

} // namespace AviQtl::Core
