#include "video_decoder.hpp"
#include "document_model.hpp"
#include "settings_manager.hpp"
#include "video_frame_store.hpp"
#include <QDebug>
#include <QPointer>
#include <QVideoFrame>
#include <QVideoFrameFormat>
#include <QtConcurrent>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <utility>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/display.h>
#include <libavutil/hwcontext.h>
#include <libavutil/imgutils.h>
#include <libavutil/time.h>
#include <libswscale/swscale.h>
}

namespace AviQtl::Core {

static int displayRotationCcwFromDisplayMatrix(const AVStream *stream) {
    if (stream == nullptr) {
        return 0;
    }

    const AVPacketSideData *sideData = stream->codecpar != nullptr ? av_packet_side_data_get(stream->codecpar->coded_side_data, stream->codecpar->nb_coded_side_data, AV_PKT_DATA_DISPLAYMATRIX) : nullptr;
    if (sideData == nullptr || sideData->size < 9 * static_cast<int>(sizeof(int32_t))) {
        return 0;
    }

    const double ccwDegrees = av_display_rotation_get(reinterpret_cast<const int32_t *>(sideData->data));
    if (!std::isfinite(ccwDegrees)) {
        return 0;
    }

    int ccw = static_cast<int>(std::llround(ccwDegrees));
    ccw %= 360;
    if (ccw < 0) {
        ccw += 360;
    }

    switch (ccw) {
    case 90:
    case 180:
    case 270:
        return ccw;
    default:
        return 0;
    }
}

static AVFrame *rotatePackedRgbaFrame(const AVFrame *srcFrame, int ccwDegrees) {
    if (srcFrame == nullptr || (srcFrame->format != AV_PIX_FMT_RGBA && srcFrame->format != AV_PIX_FMT_RGBA64LE)) {
        return nullptr;
    }

    ccwDegrees %= 360;
    if (ccwDegrees < 0) {
        ccwDegrees += 360;
    }
    if (ccwDegrees != 90 && ccwDegrees != 180 && ccwDegrees != 270) {
        return nullptr;
    }

    const int srcWidth = srcFrame->width;
    const int srcHeight = srcFrame->height;
    const int bytesPerPixel = srcFrame->format == AV_PIX_FMT_RGBA64LE ? 8 : 4;
    const int dstWidth = ccwDegrees == 180 ? srcWidth : srcHeight;
    const int dstHeight = ccwDegrees == 180 ? srcHeight : srcWidth;

    AVFrame *dstFrame = av_frame_alloc();
    if (dstFrame == nullptr) {
        return nullptr;
    }

    dstFrame->format = srcFrame->format;
    dstFrame->width = dstWidth;
    dstFrame->height = dstHeight;
    if (av_frame_get_buffer(dstFrame, 32) < 0) {
        av_frame_free(&dstFrame);
        return nullptr;
    }

    for (int y = 0; y < srcHeight; ++y) {
        const uint8_t *src = srcFrame->data[0] + y * srcFrame->linesize[0];
        for (int x = 0; x < srcWidth; ++x) {
            int dstX = 0;
            int dstY = 0;
            switch (ccwDegrees) {
            case 90:
                dstX = y;
                dstY = srcWidth - 1 - x;
                break;
            case 180:
                dstX = srcWidth - 1 - x;
                dstY = srcHeight - 1 - y;
                break;
            case 270:
                dstX = srcHeight - 1 - y;
                dstY = x;
                break;
            default:
                break;
            }
            memcpy(dstFrame->data[0] + dstY * dstFrame->linesize[0] + dstX * bytesPerPixel, src + x * bytesPerPixel, bytesPerPixel);
        }
    }

    dstFrame->pts = srcFrame->pts;
    dstFrame->best_effort_timestamp = srcFrame->best_effort_timestamp;
    dstFrame->duration = srcFrame->duration;
    dstFrame->time_base = srcFrame->time_base;
    return dstFrame;
}

auto VideoDecoder::getHwFormat(AVCodecContext *ctx, const enum AVPixelFormat *pixfmts) -> enum AVPixelFormat {
    const enum AVPixelFormat *p = nullptr;
    auto *decoder = reinterpret_cast<VideoDecoder *>(ctx->opaque);
    for (p = pixfmts; *p != -1; p++) {
        if (*p == decoder->m_hwPixFmt) {
            return *p;
        }
    }
    return avcodec_default_get_format(ctx, pixfmts);
}

VideoDecoder::VideoDecoder(int clipId, const QUrl &source, VideoFrameStore *store, QObject *parent) : MediaDecoder(clipId, source, parent), m_store(store), m_frame(av_frame_alloc()), m_pkt(av_packet_alloc()) {

    updateCacheSize();
    connect(&SettingsManager::instance(), &SettingsManager::settingsChanged, this, &VideoDecoder::updateCacheSize);
}

VideoDecoder::~VideoDecoder() {
    m_closing.store(true, std::memory_order_release);

    // Wait for async init and decode workers to finish. Use waitForFinished()
    // unconditionally so already-completed futures do not leave dangling
    // references to this object in queued QMetaObject::invokeMethod calls.
    m_initFuture.waitForFinished();
    m_decodeFuture.waitForFinished();

    close();
    if (m_swsCtx != nullptr) {
        sws_freeContext(m_swsCtx);
    }
    if (m_frame != nullptr) {
        av_frame_free(&m_frame);
    }
    if (m_pkt != nullptr) {
        av_packet_free(&m_pkt);
    }
}

void VideoDecoder::startDecoding() {
    m_initFuture = QtConcurrent::run([this]() -> void {
        QString path = m_source.toLocalFile();
        if (path.isEmpty()) {
            path = m_source.toString();
        }
        if (open(path)) {
            m_isReady = true;
            QMetaObject::invokeMethod(
                this,
                [this]() -> void {
                    emit ready();
                    emit videoMetaReady(static_cast<int>(m_index.size()), m_sourceFps);
                },
                Qt::QueuedConnection);
        }
    });
}

auto VideoDecoder::open(const QString &path) -> bool {
    QMutexLocker locker(&m_mutex);
    close();
    m_lastDecodedFrame = -1;
    m_index.clear();
    m_prevKeyframe.clear();

    if (avformat_open_input(&m_fmtCtx, path.toUtf8().constData(), nullptr, nullptr) != 0) {
        return false;
    }
    if (avformat_find_stream_info(m_fmtCtx, nullptr) < 0) {
        close();
        return false;
    }

    m_streamIndex = av_find_best_stream(m_fmtCtx, AVMEDIA_TYPE_VIDEO, -1, -1, nullptr, 0);
    if (m_streamIndex < 0) {
        close();
        return false;
    }

    m_stream = m_fmtCtx->streams[m_streamIndex];
    m_timeBase = m_stream->time_base;
    m_displayRotationCcw = displayRotationCcwFromDisplayMatrix(m_stream);
    double fps = av_q2d(m_stream->avg_frame_rate);
    if (fps <= 0.0) {
        fps = av_q2d(m_stream->r_frame_rate);
    }
    m_sourceFps = fps;

    const AVCodec *codec = avcodec_find_decoder(m_stream->codecpar->codec_id);
    if (codec == nullptr) {
        close();
        return false;
    }

    m_decCtx = avcodec_alloc_context3(codec);
    if (m_decCtx == nullptr) {
        close();
        return false;
    }
    if (avcodec_parameters_to_context(m_decCtx, m_stream->codecpar) < 0) {
        close();
        return false;
    }

    m_hwPixFmt = -1;
    const char *hwtypenames[] = {"cuda", "vaapi", "d3d11va", "dxva2", "videotoolbox", nullptr};
    for (const char **name = hwtypenames; (*name) != nullptr; ++name) {
        enum AVHWDeviceType type = av_hwdevice_find_type_by_name(*name);
        if (type == AV_HWDEVICE_TYPE_NONE) {
            continue;
        }
        for (int i = 0;; i++) {
            const AVCodecHWConfig *config = avcodec_get_hw_config(codec, i);
            if (config == nullptr) {
                break;
            }
            if (((config->methods & AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX) != 0) && config->device_type == type) {
                if (av_hwdevice_ctx_create(&m_hwDeviceCtx, type, nullptr, nullptr, 0) == 0) {
                    m_hwPixFmt = config->pix_fmt;
                    m_decCtx->hw_device_ctx = av_buffer_ref(m_hwDeviceCtx);
                    m_decCtx->get_format = getHwFormat;
                    m_decCtx->opaque = this;
                    goto hwinitdone;
                }
            }
        }
    }
hwinitdone:
    if (m_hwDeviceCtx == nullptr) {
        if ((codec->capabilities & AV_CODEC_CAP_FRAME_THREADS) != 0) {
            m_decCtx->thread_type = FF_THREAD_FRAME;
            m_decCtx->thread_count = AviQtl::Core::SettingsManager::instance().value(QStringLiteral("videoDecoderThreads"), 0).toInt();
        } else if ((codec->capabilities & AV_CODEC_CAP_SLICE_THREADS) != 0) {
            m_decCtx->thread_type = FF_THREAD_SLICE;
            m_decCtx->thread_count = AviQtl::Core::SettingsManager::instance().value(QStringLiteral("videoDecoderThreads"), 0).toInt();
        }
    }

    if (avcodec_open2(m_decCtx, codec, nullptr) != 0) {
        close();
        return false;
    }
    qInfo() << "[VideoDecoder][rotation]" << path << "coded" << m_decCtx->width << "x" << m_decCtx->height << "displayRotationCcw" << m_displayRotationCcw;
    if (!buildIndex()) {
        close();
        return false;
    }
    return true;
}

bool VideoDecoder::getFrameFromGopCache(int frameIndex, QVideoFrame &outFrame) {
    std::lock_guard<std::mutex> locker(m_gopCacheMutex);
    int hitIndex = -1;
    for (int i = 0; i < m_gopCacheCount; ++i) {
        if (frameIndex >= m_currentGopCache[i].startFrame && frameIndex <= m_currentGopCache[i].endFrame) {
            if (m_currentGopCache[i].frames.contains(frameIndex)) {
                hitIndex = i;
                break;
            }
        }
    }
    if (hitIndex == -1)
        return false;

    // MLT式ポインタシャッフルによるLRU更新
    GopCacheBlock *alt = (m_currentGopCache == m_gopCacheA) ? m_gopCacheB : m_gopCacheA;
    int j = 0;
    for (int i = 0; i < m_gopCacheCount; ++i) {
        if (i != hitIndex)
            alt[j++] = std::move(m_currentGopCache[i]);
    }
    alt[j++] = std::move(m_currentGopCache[hitIndex]);
    outFrame = alt[j - 1].frames.value(frameIndex);
    m_currentGopCache = alt;
    m_gopCacheCount = j;
    return true;
}

void VideoDecoder::storeGopCacheBlock(GopCacheBlock block) {
    if (block.frames.isEmpty()) {
        return;
    }

    std::lock_guard<std::mutex> locker(m_gopCacheMutex);
    GopCacheBlock *alt = (m_currentGopCache == m_gopCacheA) ? m_gopCacheB : m_gopCacheA;
    const int sourceStart = m_gopCacheCount == MAX_GOP_CACHE_SIZE ? 1 : 0;
    if (sourceStart != 0) {
        m_gopCacheEvictions.fetch_add(1, std::memory_order_relaxed);
    }

    int nextCount = 0;
    for (int i = sourceStart; i < m_gopCacheCount; ++i) {
        alt[nextCount++] = std::move(m_currentGopCache[i]);
    }
    alt[nextCount++] = std::move(block);
    m_currentGopCache = alt;
    m_gopCacheCount = nextCount;
}

int VideoDecoder::findGopEndIndex(int startFrame) const {
    if (m_index.empty())
        return 0;
    for (size_t i = static_cast<size_t>(startFrame) + 1; i < m_index.size(); ++i) {
        if (m_index[i].isKeyframe)
            return static_cast<int>(i) - 1;
    }
    return static_cast<int>(m_index.size()) - 1;
}

auto VideoDecoder::buildIndex() -> bool {
    if (av_seek_frame(m_fmtCtx, m_streamIndex, 0, AVSEEK_FLAG_BACKWARD) < 0) {
        av_seek_frame(m_fmtCtx, -1, 0, AVSEEK_FLAG_BACKWARD);
    }

    if (m_stream->nb_frames > 0) {
        m_index.reserve(m_stream->nb_frames);
    } else {
        m_index.reserve(SettingsManager::instance().value(QStringLiteral("videoDecoderIndexReserve"), 108000).toInt());
    }

    AVPacket *pkt = av_packet_alloc();
    while (av_read_frame(m_fmtCtx, pkt) >= 0) {
        if (pkt->stream_index == m_streamIndex) {
            m_index.push_back({.pts = pkt->pts, .dts = pkt->dts, .isKeyframe = static_cast<bool>(pkt->flags & AV_PKT_FLAG_KEY)});
        }
        av_packet_unref(pkt);
    }
    av_packet_free(&pkt);

    std::ranges::sort(m_index, [](const auto &a, const auto &b) -> auto {
        if (a.pts != AV_NOPTS_VALUE && b.pts != AV_NOPTS_VALUE) {
            return a.pts < b.pts;
        }
        return a.dts < b.dts;
    });

    // Build O(1) keyframe lookup table
    m_prevKeyframe.resize(m_index.size());
    int lastKey = 0;
    for (size_t i = 0; i < m_index.size(); ++i) {
        if (m_index[i].isKeyframe) {
            lastKey = static_cast<int>(i);
        }
        m_prevKeyframe[i] = lastKey;
    }
    return true;
}

void VideoDecoder::close() {
    if (m_decCtx != nullptr) {
        avcodec_free_context(&m_decCtx);
    }
    m_decCtx = nullptr;
    if (m_fmtCtx != nullptr) {
        avformat_close_input(&m_fmtCtx);
    }
    m_fmtCtx = nullptr;
    if (m_hwDeviceCtx != nullptr) {
        av_buffer_unref(&m_hwDeviceCtx);
    }
    m_hwDeviceCtx = nullptr;
    m_lastGoodFrame = QVideoFrame();
    m_displayRotationCcw = 0;
}

void VideoDecoder::seek(qint64 ms) { emit seekRequested(ms); }

auto VideoDecoder::sourceFps() const -> double { return m_sourceFps; }
auto VideoDecoder::totalFrameCount() const -> int { return static_cast<int>(m_index.size()); }

auto VideoDecoder::frameIndexFromSeconds(double seconds) const -> int {
    if (m_index.empty()) {
        return 0;
    }
    const double tb = av_q2d(m_timeBase);
    if (tb <= 0.0) {
        double fps = m_sourceFps > 0.0 ? m_sourceFps : 30.0;
        int f = static_cast<int>(std::llround(seconds * fps));
        f = std::max(f, 0);
        f = std::min(f, static_cast<int>(m_index.size()) - 1);
        return f;
    }
    const auto targetPts = static_cast<int64_t>(std::llround(seconds / tb));
    auto it = std::ranges::lower_bound(m_index, targetPts, std::ranges::less{}, &FrameIndexEntry::pts);
    int idx = static_cast<int>(std::distance(m_index.begin(), it));
    if (idx <= 0) {
        return 0;
    }
    if (std::cmp_greater_equal(idx, m_index.size())) {
        return static_cast<int>(m_index.size()) - 1;
    }
    const int64_t a = m_index[idx - 1].pts;
    const int64_t b = m_index[idx].pts;
    return std::llabs(targetPts - a) <= std::llabs(b - targetPts) ? idx - 1 : idx;
}

void VideoDecoder::seekToTime(double seconds) {
    if (!m_isReady) {
        return;
    }
    seconds = std::max(seconds, 0.0);
    const int frame = frameIndexFromSeconds(seconds);
    seekToFrame(frame, m_sourceFps);
}

void VideoDecoder::seekToFrame(int frame, double fps) { // NOLINT(bugprone-easily-swappable-parameters)
    if (!m_isReady) {
        return;
    }
    if (frame < 0) {
        return;
    }
    m_lastRequestedFrame.store(frame, std::memory_order_release);

    bool expected = false;
    if (!m_isDecoding.compare_exchange_strong(expected, true)) {
        return;
    }

    m_decodeFuture = QtConcurrent::run([this, fps]() -> void {
        // Always reset the decoding flag when the outer decode loop exits so the
        // next seekToFrame() can start a new worker. Individual tasks use their
        // own qScopeGuard for the same guarantee.
        const auto loopGuard = qScopeGuard([this]() {
            m_isDecoding.store(false, std::memory_order_release);
        });
        while (!m_closing.load(std::memory_order_acquire)) {
            int targetFrame = m_lastRequestedFrame.load(std::memory_order_acquire);
            decodeTask(targetFrame, fps);
            if (m_lastRequestedFrame.load(std::memory_order_acquire) == targetFrame) {
                m_isDecoding.store(false, std::memory_order_release);
                if (m_lastRequestedFrame.load(std::memory_order_acquire) != targetFrame) {
                    bool exp = false;
                    if (m_isDecoding.compare_exchange_strong(exp, true)) {
                        continue;
                    }
                }
                break;
            }
        }
    });
}

bool VideoDecoder::tryCacheHit(int targetFrame, const QString &clipKey) {
    QVideoFrame cachedFrame;
    if (getFrameFromGopCache(targetFrame, cachedFrame)) {
        m_gopCacheHits.fetch_add(1, std::memory_order_relaxed);
        m_store->setVideoFrameSafe(clipKey, cachedFrame);
        QMetaObject::invokeMethod(this, [this, targetFrame]() -> void { emit frameReady(targetFrame); }, Qt::QueuedConnection);
        return true;
    }

    if (QVideoFrame *cached = m_frameCache.object(targetFrame)) {
        m_frameCacheHits.fetch_add(1, std::memory_order_relaxed);
        m_store->setVideoFrameSafe(clipKey, *cached);
        QMetaObject::invokeMethod(this, [this, targetFrame]() { emit frameReady(targetFrame); }, Qt::QueuedConnection);
        return true;
    }

    m_cacheMisses.fetch_add(1, std::memory_order_relaxed);
    return false;
}

void VideoDecoder::decodeTask(int targetFrame, double fps) { // NOLINT(bugprone-easily-swappable-parameters)
    QMutexLocker locker(&m_mutex);
    if (m_closing.load(std::memory_order_acquire)) {
        return;
    }
    if ((m_decCtx == nullptr) || m_index.empty()) {
        return;
    }

    targetFrame = std::max(targetFrame, 0);
    targetFrame = std::min(targetFrame, static_cast<int>(m_index.size()) - 1);

    const QString &clipKey = clipIdString();

    if (tryCacheHit(targetFrame, clipKey)) {
        return;
    }

    const auto &targetEntry = m_index[targetFrame];
    bool needSeek = true;

    int keyIndex = m_prevKeyframe[targetFrame];
    int gopEndIndex = findGopEndIndex(keyIndex);

    if (m_lastDecodedFrame != -1 && targetFrame > m_lastDecodedFrame && targetFrame <= m_lastDecodedFrame + 120) {
        needSeek = false;
    }

    bool shouldFillGop = needSeek; // 逆再生やジャンプ時はGOPを充填する

    if (needSeek) {
        int64_t seekPts = m_index[keyIndex].pts;
        if (avformat_seek_file(m_fmtCtx, m_streamIndex, seekPts, seekPts, seekPts, AVSEEK_FLAG_BACKWARD) < 0) {
            av_seek_frame(m_fmtCtx, m_streamIndex, seekPts, AVSEEK_FLAG_BACKWARD);
        }
        avcodec_flush_buffers(m_decCtx);
        m_lastDecodedFrame = keyIndex - 1;
    }

    GopCacheBlock newGopBlock;
    newGopBlock.keyframeIndex = keyIndex;
    newGopBlock.startFrame = keyIndex;
    newGopBlock.endFrame = gopEndIndex;

    bool targetDispatched = false;
    // 再使用: デコードループで毎回alloc/unrefする代わりにメンバ変数をunrefして再利用
    av_packet_unref(m_pkt);
    int maxDecodeCount = std::max(500, (gopEndIndex - keyIndex) + 10);
    bool eof = false;

    while (maxDecodeCount-- > 0) {
        int ret = 0;
        if (!eof) {
            ret = av_read_frame(m_fmtCtx, m_pkt);
            if (ret < 0) {
                eof = true;
            }
        }
        if (eof) {
            ret = avcodec_send_packet(m_decCtx, nullptr);
        } else if (m_pkt->stream_index == m_streamIndex) {
            ret = avcodec_send_packet(m_decCtx, m_pkt);
        }
        if (!eof) {
            av_packet_unref(m_pkt);
        }

        while (ret >= 0 || ret == AVERROR(EAGAIN)) {
            int rxRet = avcodec_receive_frame(m_decCtx, m_frame);
            if (rxRet == AVERROR(EAGAIN)) {
                break;
            }
            if (rxRet == AVERROR_EOF) {
                eof = true;
                break;
            }
            if (rxRet < 0) {
                break;
            }

            int64_t currentPts = m_frame->best_effort_timestamp != AV_NOPTS_VALUE ? m_frame->best_effort_timestamp : m_frame->pts;

            auto it = std::ranges::lower_bound(m_index, currentPts, std::ranges::less{}, &FrameIndexEntry::pts);
            int decodedFrameIndex = -1;
            if (it != m_index.end() && it->pts == currentPts) {
                decodedFrameIndex = static_cast<int>(std::distance(m_index.begin(), it));
            }

            if (decodedFrameIndex == -1) {
                continue;
            }

            if (decodedFrameIndex == targetFrame && !targetDispatched) {
                QVideoFrame *cached = m_frameCache.object(decodedFrameIndex);
                if (cached && cached->isValid()) {
                    m_store->setVideoFrameSafe(clipKey, *cached);
                    m_lastGoodFrame = *cached;
                    if (auto *app = qApp) {
                        QMetaObject::invokeMethod(this, [this, targetFrame]() { emit frameReady(targetFrame); }, Qt::QueuedConnection);
                    }
                    targetDispatched = true;
                }
            }

            // キャッシュにない場合のみデコードと変換を行う
            if (!m_frameCache.contains(decodedFrameIndex)) {
                AVFrame *srcFrame = m_frame;
                AVFrame *swFrame = nullptr;
                AVFrame *convertedFrame = nullptr;

                if (m_frame->format == m_hwPixFmt) {
                    swFrame = av_frame_alloc();
                    if (av_hwframe_transfer_data(swFrame, m_frame, 0) == 0) {
                        srcFrame = swFrame;
                    } else {
                        av_frame_free(&swFrame);
                        srcFrame = nullptr;
                    }
                }

                if (srcFrame != nullptr) {
                    // Qtが直接扱えないフォーマット(10bit等)の場合、YUV420P(8bit)に変換する
                    AVPixelFormat pixFmt = static_cast<AVPixelFormat>(srcFrame->format);
                    if (pixFmt == AV_PIX_FMT_YUVJ420P)
                        pixFmt = AV_PIX_FMT_YUV420P;
                    if (pixFmt == AV_PIX_FMT_YUVJ422P)
                        pixFmt = AV_PIX_FMT_YUV422P;
                    if (pixFmt == AV_PIX_FMT_YUVJ444P)
                        pixFmt = AV_PIX_FMT_YUV444P;

                    bool useHBD = DocumentModel::instance().projectSettings().highBitDepth;

                    bool isSupported = (pixFmt == AV_PIX_FMT_YUV420P || pixFmt == AV_PIX_FMT_NV12 || pixFmt == AV_PIX_FMT_RGBA);
                    if (useHBD)
                        isSupported |= (pixFmt == AV_PIX_FMT_RGBA64LE || pixFmt == AV_PIX_FMT_P010LE);

                    const bool needsPixelRotation = m_displayRotationCcw != 0;
                    const bool needsRgbaForRotation = needsPixelRotation && pixFmt != AV_PIX_FMT_RGBA && pixFmt != AV_PIX_FMT_RGBA64LE;
                    if (!isSupported || needsRgbaForRotation) {
                        // プロジェクト設定に応じてターゲットフォーマットを選択
                        AVPixelFormat targetFmt = useHBD ? AV_PIX_FMT_RGBA64LE : AV_PIX_FMT_RGBA;
                        m_swsCtx = sws_getCachedContext(m_swsCtx, srcFrame->width, srcFrame->height, static_cast<AVPixelFormat>(srcFrame->format), srcFrame->width, srcFrame->height, targetFmt, SWS_BILINEAR, nullptr, nullptr, nullptr);
                        if (m_swsCtx != nullptr) {
                            convertedFrame = av_frame_alloc();
                            convertedFrame->format = targetFmt;
                            convertedFrame->width = srcFrame->width;
                            convertedFrame->height = srcFrame->height;
                            if (av_frame_get_buffer(convertedFrame, 32) == 0) {
                                sws_scale(m_swsCtx, srcFrame->data, srcFrame->linesize, 0, srcFrame->height, convertedFrame->data, convertedFrame->linesize);
                                convertedFrame->pts = srcFrame->pts;
                                convertedFrame->best_effort_timestamp = srcFrame->best_effort_timestamp;
                                srcFrame = convertedFrame;
                            } else {
                                av_frame_free(&convertedFrame);
                                convertedFrame = nullptr;
                            }
                        }
                    }

                    AVFrame *rotatedFrame = nullptr;
                    if (needsPixelRotation) {
                        rotatedFrame = rotatePackedRgbaFrame(srcFrame, m_displayRotationCcw);
                        if (rotatedFrame != nullptr) {
                            srcFrame = rotatedFrame;
                        } else {
                            qWarning() << "[VideoDecoder][rotation] failed to rotate frame, using unrotated pixels:" << m_source;
                        }
                    }

                    QVideoFrameFormat::PixelFormat qtFmt = QVideoFrameFormat::Format_Invalid;
                    switch (srcFrame->format) {
                    case AV_PIX_FMT_YUV420P:
                    case AV_PIX_FMT_YUVJ420P:
                        qtFmt = QVideoFrameFormat::Format_YUV420P;
                        break;
                    case AV_PIX_FMT_NV12:
                        qtFmt = QVideoFrameFormat::Format_NV12;
                        break;
                    case AV_PIX_FMT_P010LE:
                        qtFmt = QVideoFrameFormat::Format_P010;
                        break;
                    case AV_PIX_FMT_RGBA:
                        qtFmt = QVideoFrameFormat::Format_RGBA8888;
                        break;
                    case AV_PIX_FMT_RGBA64LE:
                        qtFmt = QVideoFrameFormat::Format_RGBA8888; // メタデータ上は8bitとして扱う
                        break;
                    default:
                        qtFmt = QVideoFrameFormat::Format_YUV420P;
                        break;
                    }

                    const QSize outputSize(srcFrame->width, srcFrame->height);
                    const AVPixelFormat outputPixFmt = static_cast<AVPixelFormat>(srcFrame->format);
                    AVFrame *ownedFrame = av_frame_alloc();
                    av_frame_ref(ownedFrame, srcFrame);
                    if (swFrame != nullptr) {
                        av_frame_free(&swFrame);
                    }
                    if (convertedFrame != nullptr) {
                        av_frame_free(&convertedFrame);
                    }
                    if (rotatedFrame != nullptr) {
                        av_frame_free(&rotatedFrame);
                    }

                    QVideoFrameFormat format(outputSize, qtFmt);
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
                    QVideoFrame videoFrame(new FFmpegVideoBuffer(ownedFrame, format), format);
#pragma clang diagnostic pop
                    av_frame_free(&ownedFrame);

                    videoFrame.setStartTime(-1);
                    videoFrame.setEndTime(-1);

                    if (videoFrame.isValid()) {
                        m_decodedFrames.fetch_add(1, std::memory_order_relaxed);
                        // メタデータが8bitでも、実際のメモリ消費(RGBA64)に合わせてコスト計算を行う
                        int bpp = (outputPixFmt == AV_PIX_FMT_RGBA64LE) ? 8 : (qtFmt == QVideoFrameFormat::Format_RGBA8888 ? 4 : 2);
                        int64_t cost = static_cast<int64_t>(videoFrame.width()) * videoFrame.height() * bpp;
                        auto *cachedFrame = new QVideoFrame(videoFrame);

                        m_frameCache.insert(decodedFrameIndex, cachedFrame, static_cast<int>(std::clamp<int64_t>(cost, 0, INT_MAX)));
                        newGopBlock.frames.insert(decodedFrameIndex, videoFrame);

                        // 最後に成功したフレームを更新 (Concealment 用)
                        m_lastGoodFrame = videoFrame;

                        if (decodedFrameIndex == targetFrame && !targetDispatched) {
                            m_store->setVideoFrameSafe(clipKey, m_lastGoodFrame);
                            if (auto *app = qApp) {
                                QMetaObject::invokeMethod(this, [this, targetFrame]() { emit frameReady(targetFrame); }, Qt::QueuedConnection);
                            }
                            targetDispatched = true;
                        }
                    }
                }
            }

            if (decodedFrameIndex != -1) {
                m_lastDecodedFrame = decodedFrameIndex;
            }

            // 途中で新しいフレーム要求が来た場合は即座にこのタスクを中断
            if (m_lastRequestedFrame.load(std::memory_order_acquire) != targetFrame) {
                av_packet_unref(m_pkt);
                return;
            }

            // 順再生時、またはGOP末尾に到達した場合は終了
            if ((!shouldFillGop && m_lastDecodedFrame >= targetFrame) || m_lastDecodedFrame >= gopEndIndex) {
                break;
            }
        }
        if ((!shouldFillGop && m_lastDecodedFrame >= targetFrame) || m_lastDecodedFrame >= gopEndIndex) {
            break;
        }
    }
    av_packet_unref(m_pkt);

    if (shouldFillGop) {
        storeGopCacheBlock(std::move(newGopBlock));
    }

    if (!targetDispatched && m_lastGoodFrame.isValid() && !m_closing.load(std::memory_order_acquire)) {
        m_store->setVideoFrameSafe(clipKey, m_lastGoodFrame);
        if (auto *app = qApp) {
            QMetaObject::invokeMethod(this, [this, targetFrame]() { emit frameReady(targetFrame); }, Qt::QueuedConnection);
        }
        targetDispatched = true;
    }

    if (!targetDispatched) {
        QPointer<VideoDecoder> self(this);
        if (auto *app = qApp) {
            QMetaObject::invokeMethod(
                this,
                [self, targetFrame]() -> void {
                    if (self && !self->m_closing.load(std::memory_order_acquire)) {
                        emit self->frameError(targetFrame);
                    }
                },
                Qt::QueuedConnection);
        }
    }
}

void VideoDecoder::updateCacheSize() {
    int sizeMB = SettingsManager::instance().settings().value(QStringLiteral("cacheSize"), 512).toInt();
    int minSizeMB = SettingsManager::instance().value(QStringLiteral("videoDecoderMinCacheMB"), 64).toInt();
    sizeMB = std::max(sizeMB, minSizeMB);
    m_frameCache.setMaxCost(static_cast<qsizetype>(sizeMB) * 1024 * 1024);
}

auto VideoDecoder::cacheStats() const -> CacheStats {
    QMutexLocker locker(&m_mutex);
    std::lock_guard<std::mutex> gopLocker(m_gopCacheMutex);
    return {
        .gopHits = m_gopCacheHits.load(std::memory_order_relaxed),
        .frameHits = m_frameCacheHits.load(std::memory_order_relaxed),
        .misses = m_cacheMisses.load(std::memory_order_relaxed),
        .decodedFrames = m_decodedFrames.load(std::memory_order_relaxed),
        .gopEvictions = m_gopCacheEvictions.load(std::memory_order_relaxed),
        .gopBlocks = m_gopCacheCount,
        .frameEntries = m_frameCache.size(),
        .frameCost = m_frameCache.totalCost(),
        .frameMaxCost = m_frameCache.maxCost(),
    };
}

void VideoDecoder::setPlaying(bool playing) {
    // 再生状態をスレッドセーフに更新
    m_isPlaying.store(playing, std::memory_order_release);
}

} // namespace AviQtl::Core
