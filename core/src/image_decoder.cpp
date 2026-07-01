#include "image_decoder.hpp"
#include "ffmpeg_video_buffer.hpp"
#include "video_frame_store.hpp"
#include <QDebug>
#include <QVideoFrame>
#include <QVideoFrameFormat>
#include <QtConcurrent>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/imgutils.h>
#include <libswscale/swscale.h>
}

namespace AviQtl::Core {

ImageDecoder::ImageDecoder(int clipId, const QUrl &source, VideoFrameStore *store, QObject *parent) : MediaDecoder(clipId, source, parent), m_store(store) {}

ImageDecoder::~ImageDecoder() {
    if (m_future.isRunning()) {
        m_future.waitForFinished();
    }
}

void ImageDecoder::seek(qint64 ms) {
    Q_UNUSED(ms);
    if (m_isReady && m_cachedVideoFrame.isValid()) {
        // すでにデコード済みの場合は、ストアに再通知してSink（画面）を更新する
        m_store->setVideoFrameSafe(clipIdString(), m_cachedVideoFrame);
    } else if (!m_future.isRunning()) {
        load();
    }
}
void ImageDecoder::setPlaying(bool playing) { Q_UNUSED(playing); }
void ImageDecoder::startDecoding() { load(); }

void ImageDecoder::load() {
    QString path = m_source.toLocalFile();
    if (path.isEmpty()) {
        path = m_source.toString();
    }
    if (path.isEmpty()) {
        return;
    }
    if (m_future.isRunning()) {
        m_future.waitForFinished();
    }
    m_future = QtConcurrent::run([this, path]() -> void { decodeImage(path); });
}

void ImageDecoder::decodeImage(const QString &path) {
    AVFormatContext *fmtCtx = nullptr;
    if (avformat_open_input(&fmtCtx, path.toUtf8().constData(), nullptr, nullptr) != 0) {
        qWarning() << "[ImageDecoder] avformat_open_input failed:" << path;
        return;
    }
    auto fmtGuard = qScopeGuard([&fmtCtx]() { avformat_close_input(&fmtCtx); });

    if (avformat_find_stream_info(fmtCtx, nullptr) < 0) {
        return;
    }

    int streamIdx = av_find_best_stream(fmtCtx, AVMEDIA_TYPE_VIDEO, -1, -1, nullptr, 0);
    if (streamIdx < 0) {
        return;
    }

    AVStream *stream = fmtCtx->streams[streamIdx];
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == nullptr) {
        return;
    }

    AVCodecContext *decCtx = avcodec_alloc_context3(codec);
    if (decCtx == nullptr) {
        return;
    }
    auto decCtxGuard = qScopeGuard([&decCtx]() { avcodec_free_context(&decCtx); });

    if (avcodec_parameters_to_context(decCtx, stream->codecpar) < 0) {
        return;
    }
    if (avcodec_open2(decCtx, codec, nullptr) < 0) {
        return;
    }

    AVPacket *pkt = av_packet_alloc();
    if (pkt == nullptr) {
        qWarning() << "[ImageDecoder] av_packet_alloc failed";
        return;
    }
    auto pktGuard = qScopeGuard([&pkt]() { av_packet_free(&pkt); });

    AVFrame *srcFrame = av_frame_alloc();
    if (srcFrame == nullptr) {
        qWarning() << "[ImageDecoder] av_frame_alloc failed";
        return;
    }
    auto srcFrameGuard = qScopeGuard([&srcFrame]() { av_frame_free(&srcFrame); });

    bool decoded = false;
    int ret = 0;
    while (!decoded && ret >= 0) {
        ret = av_read_frame(fmtCtx, pkt);
        if (ret < 0) {
            // Real EOF or error: flush decoder
            avcodec_send_packet(decCtx, nullptr);
            while (avcodec_receive_frame(decCtx, srcFrame) == 0) {
                decoded = true;
                break;
            }
            break;
        }
        if (pkt->stream_index != streamIdx) {
            av_packet_unref(pkt);
            continue;
        }

        // Send packet; on EAGAIN, drain frames then retry
        while (true) {
            int sendRet = avcodec_send_packet(decCtx, pkt);
            if (sendRet == 0) {
                break;
            }
            if (sendRet == AVERROR(EAGAIN)) {
                while (avcodec_receive_frame(decCtx, srcFrame) == 0) {
                    decoded = true;
                    break;
                }
                if (decoded) {
                    break;
                }
                continue; // retry send after draining
            }
            break; // fatal error, discard packet
        }

        // Drain available frames after send
        while (avcodec_receive_frame(decCtx, srcFrame) == 0) {
            decoded = true;
            break;
        }
        av_packet_unref(pkt);
    }

    if (decoded) {
        AVPixelFormat targetFmt = AV_PIX_FMT_RGBA;
        AVFrame *rgbaFrame = av_frame_alloc();
        if (rgbaFrame == nullptr) {
            qWarning() << "[ImageDecoder] av_frame_alloc (rgba) failed";
            return;
        }
        rgbaFrame->format = targetFmt;
        rgbaFrame->width = srcFrame->width;
        rgbaFrame->height = srcFrame->height;
        if (av_frame_get_buffer(rgbaFrame, 0) < 0) {
            av_frame_free(&rgbaFrame);
            return;
        }
        auto rgbaGuard = qScopeGuard([&rgbaFrame]() { av_frame_free(&rgbaFrame); });

        SwsContext *swsCtx = sws_getContext(srcFrame->width, srcFrame->height, static_cast<AVPixelFormat>(srcFrame->format), rgbaFrame->width, rgbaFrame->height, targetFmt, SWS_BILINEAR, nullptr, nullptr, nullptr);
        if (swsCtx == nullptr) {
            return;
        }
        auto swsGuard = qScopeGuard([&swsCtx]() { sws_freeContext(swsCtx); });

        sws_scale(swsCtx, srcFrame->data, srcFrame->linesize, 0, srcFrame->height, rgbaFrame->data, rgbaFrame->linesize);

        QImage img(rgbaFrame->data[0], rgbaFrame->width, rgbaFrame->height, rgbaFrame->linesize[0], QImage::Format_RGBA8888);
        m_cachedImage = img.copy();
        const QString &clipIdStr = clipIdString();
        m_store->setFrameSafe(clipIdStr, m_cachedImage);

        QVideoFrameFormat fmt(QSize(rgbaFrame->width, rgbaFrame->height), QVideoFrameFormat::Format_RGBA8888);
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        auto *buf = new FFmpegVideoBuffer(rgbaFrame, fmt);
        QVideoFrame vf(buf, fmt);
#pragma clang diagnostic pop
        m_cachedVideoFrame = vf;

        // FFmpegVideoBuffer has av_frame_ref'd, safe to free here
        av_frame_free(&rgbaFrame);
        rgbaGuard.dismiss();

        m_store->setVideoFrameSafe(clipIdStr, m_cachedVideoFrame);
        QMetaObject::invokeMethod(this, [this]() -> void { emit ready(); }, Qt::QueuedConnection);
    }
}

} // namespace AviQtl::Core
