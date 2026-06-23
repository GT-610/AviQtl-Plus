#include "video_encoder.hpp"
#include "settings_manager.hpp"
#include <QDebug>
#include <QLoggingCategory>
#include <math.h>
#include <algorithm>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/audio_fifo.h>
#include <libavutil/channel_layout.h>
#include <libavutil/hwcontext.h>
#include <libavutil/imgutils.h>
#include <libavutil/opt.h>
#include <libavutil/pixdesc.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
}

Q_LOGGING_CATEGORY(lcVideoEncoder, "aviqtl.video_encoder")

namespace AviQtl::Core {

constexpr size_t MAX_QUEUE_SIZE = 16;

namespace {

// Minimal scoped helpers to remove repeated nullptr checks in cleanup.
template <typename T, void (*Deleter)(T **)>
class AvPointer {
public:
    AvPointer() = default;
    explicit AvPointer(T *ptr) : m_ptr(ptr) {}
    ~AvPointer() { reset(); }

    AvPointer(const AvPointer &) = delete;
    AvPointer &operator=(const AvPointer &) = delete;
    AvPointer(AvPointer &&other) noexcept : m_ptr(other.release()) {}
    AvPointer &operator=(AvPointer &&other) noexcept {
        if (this != &other) {
            reset();
            m_ptr = other.release();
        }
        return *this;
    }

    T *get() const { return m_ptr; }
    T **ptr() { return &m_ptr; }
    T *operator->() const { return m_ptr; }
    T &operator*() const { return *m_ptr; }
    void reset(T *ptr = nullptr) {
        if (m_ptr) {
            Deleter(&m_ptr);
        }
        m_ptr = ptr;
    }
    T *release() {
        T *tmp = m_ptr;
        m_ptr = nullptr;
        return tmp;
    }
    explicit operator bool() const { return m_ptr != nullptr; }

private:
    T *m_ptr = nullptr;
};

using AvPacketPtr = AvPointer<AVPacket, av_packet_free>;
using AvFramePtr = AvPointer<AVFrame, av_frame_free>;

} // namespace

VideoEncoder::VideoEncoder(QObject *parent) : QObject(parent) {}

VideoEncoder::~VideoEncoder() { close(); }

QStringList VideoEncoder::availableVideoEncoders() {
    QStringList result;
    const QStringList allEncoders = {
        QStringLiteral("libx264"),
        QStringLiteral("h264_nvenc"),
        QStringLiteral("h264_amf"),
        QStringLiteral("h264_qsv"),
        QStringLiteral("h264_vaapi"),
        QStringLiteral("libx265"),
        QStringLiteral("hevc_nvenc"),
        QStringLiteral("hevc_amf"),
        QStringLiteral("hevc_qsv"),
        QStringLiteral("hevc_vaapi"),
        QStringLiteral("libaom-av1"),
        QStringLiteral("av1_nvenc"),
        QStringLiteral("av1_amf"),
        QStringLiteral("av1_vaapi"),
    };

    for (const auto &name : allEncoders) {
        const AVCodec *codec = avcodec_find_encoder_by_name(name.toStdString().c_str());
        if (codec == nullptr) {
            continue;
        }

        // For hardware encoders, test if the device can be created
        if (name.contains(QLatin1String("nvenc")) || name.contains(QLatin1String("vaapi")) ||
            name.contains(QLatin1String("qsv")) || name.contains(QLatin1String("videotoolbox"))) {
            AVHWDeviceType type = AV_HWDEVICE_TYPE_NONE;
            if (name.contains(QLatin1String("nvenc"))) {
                type = AV_HWDEVICE_TYPE_CUDA;
            } else if (name.contains(QLatin1String("vaapi"))) {
                type = AV_HWDEVICE_TYPE_VAAPI;
            } else if (name.contains(QLatin1String("qsv"))) {
                type = AV_HWDEVICE_TYPE_QSV;
            } else if (name.contains(QLatin1String("videotoolbox"))) {
                type = AV_HWDEVICE_TYPE_VIDEOTOOLBOX;
            }

            AVBufferRef *hwDeviceCtx = nullptr;
            int err = av_hwdevice_ctx_create(&hwDeviceCtx, type, nullptr, nullptr, 0);
            if (err < 0) {
                continue; // Hardware not available
            }
            av_buffer_unref(&hwDeviceCtx);
        }

        result.append(name);
    }

    // Always ensure software fallbacks are present
    if (!result.contains(QStringLiteral("libx264"))) {
        result.prepend(QStringLiteral("libx264"));
    }

    return result;
}

QStringList VideoEncoder::availableAudioEncoders() {
    QStringList result;
    const QStringList allEncoders = {
        QStringLiteral("aac"),
        QStringLiteral("libopus"),
        QStringLiteral("libmp3lame"),
        QStringLiteral("flac"),
        QStringLiteral("pcm_s16le"),
    };

    for (const auto &name : allEncoders) {
        const AVCodec *codec = avcodec_find_encoder_by_name(name.toStdString().c_str());
        if (codec != nullptr) {
            result.append(name);
        }
    }

    return result;
}

QString VideoEncoder::fallbackEncoder(const QString &hwEncoder) {
    static const QMap<QString, QString> fallbackMap = {
        {QStringLiteral("h264_nvenc"), QStringLiteral("libx264")},
        {QStringLiteral("h264_amf"), QStringLiteral("libx264")},
        {QStringLiteral("h264_qsv"), QStringLiteral("libx264")},
        {QStringLiteral("h264_vaapi"), QStringLiteral("libx264")},
        {QStringLiteral("hevc_nvenc"), QStringLiteral("libx265")},
        {QStringLiteral("hevc_amf"), QStringLiteral("libx265")},
        {QStringLiteral("hevc_qsv"), QStringLiteral("libx265")},
        {QStringLiteral("hevc_vaapi"), QStringLiteral("libx265")},
        {QStringLiteral("av1_nvenc"), QStringLiteral("libaom-av1")},
        {QStringLiteral("av1_amf"), QStringLiteral("libaom-av1")},
        {QStringLiteral("av1_vaapi"), QStringLiteral("libaom-av1")},
    };

    return fallbackMap.value(hwEncoder, hwEncoder);
}

void VideoEncoder::cleanup() {
    if (m_swsCtx != nullptr) {
        sws_freeContext(m_swsCtx);
        m_swsCtx = nullptr;
    }
    m_swsSrcFmt = -1;
    if (m_swrCtx != nullptr) {
        swr_free(&m_swrCtx);
        m_swrCtx = nullptr;
    }
    av_frame_free(&m_hwFrame);
    av_frame_free(&m_swFrame);
    av_frame_free(&m_audioFrame);
    if (m_audioFifo != nullptr) {
        av_audio_fifo_free(m_audioFifo);
        m_audioFifo = nullptr;
    }
    avcodec_free_context(&m_encCtx);
    if (m_fmtCtx != nullptr) {
        if ((m_fmtCtx->oformat->flags & AVFMT_NOFILE) == 0) {
            avio_closep(&m_fmtCtx->pb);
        }
        avformat_free_context(m_fmtCtx);
        m_fmtCtx = nullptr;
    }
    av_buffer_unref(&m_hwDeviceCtx);
    avcodec_free_context(&m_audioEncCtx);
}

auto VideoEncoder::initHardware(const QString &codecName) -> bool {
    int err = 0;
    AVHWDeviceType type = AV_HWDEVICE_TYPE_NONE;

    // コーデック名から適切なHWデバイスタイプを推論
    if (codecName.contains(QLatin1String("nvenc"))) {
        type = AV_HWDEVICE_TYPE_CUDA;
    } else if (codecName.contains(QLatin1String("vaapi"))) {
        type = AV_HWDEVICE_TYPE_VAAPI;
    } else if (codecName.contains(QLatin1String("qsv"))) {
        type = AV_HWDEVICE_TYPE_QSV;
    } else if (codecName.contains(QLatin1String("d3d11"))) {
        type = AV_HWDEVICE_TYPE_D3D11VA;
    } else if (codecName.contains(QLatin1String("dxva2"))) {
        type = AV_HWDEVICE_TYPE_DXVA2;
    } else if (codecName.contains(QLatin1String("videotoolbox"))) {
        type = AV_HWDEVICE_TYPE_VIDEOTOOLBOX;
    } else if (codecName.contains(QLatin1String("amf"))) {
        type = AV_HWDEVICE_TYPE_NONE;
    }

    if (type == AV_HWDEVICE_TYPE_NONE) {
        return true; // SWエンコードまたはデバイス不要
    }

    err = av_hwdevice_ctx_create(&m_hwDeviceCtx, type, nullptr, nullptr, 0);
    if (err < 0) {
        qWarning() << "Failed to create HW device context for" << codecName << "Error:" << err;
        return false;
    }
    qCInfo(lcVideoEncoder) << "Hardware device initialized:" << av_hwdevice_get_type_name(type);
    return true;
}

auto VideoEncoder::open(const Config &config) -> bool {
    std::scoped_lock lock(m_mutex);
    cleanup();
    m_config = config;
    m_headerWritten = false;
    m_encodedFrameCount = 0;

    avformat_alloc_output_context2(&m_fmtCtx, nullptr, nullptr, config.outputUrl.toStdString().c_str());
    if (m_fmtCtx == nullptr) {
        qWarning() << "Could not deduce output format from file extension.";
        return false;
    }

    const AVCodec *codec = avcodec_find_encoder_by_name(config.codecName.toStdString().c_str());
    if (codec == nullptr) {
        qWarning() << "Codec not found:" << config.codecName;
        cleanup();
        return false;
    }

    m_stream = avformat_new_stream(m_fmtCtx, codec);
    if (m_stream == nullptr) {
        cleanup();
        return false;
    }

    m_encCtx = avcodec_alloc_context3(codec);
    if (m_encCtx == nullptr) {
        cleanup();
        return false;
    }

    if (!initHardware(config.codecName)) {
        // Hardware initialization failed, try fallback to software encoder
        QString fallback = fallbackEncoder(config.codecName);
        if (fallback != config.codecName) {
            qCInfo(lcVideoEncoder) << "Hardware encoder" << config.codecName << "not available, falling back to" << fallback;
            m_config.codecName = fallback;
            const AVCodec *fallbackCodec = avcodec_find_encoder_by_name(fallback.toStdString().c_str());
            if (fallbackCodec == nullptr) {
                qWarning() << "Fallback codec not found:" << fallback;
                cleanup();
                return false;
            }
            // Re-create codec context with fallback codec
            avcodec_free_context(&m_encCtx);
            m_encCtx = avcodec_alloc_context3(fallbackCodec);
            if (m_encCtx == nullptr) {
                cleanup();
                return false;
            }
            // No hardware context needed for software encoder
        } else {
            cleanup();
            return false;
        }
    }

    if (m_hwDeviceCtx != nullptr) {
        AVBufferRef *hw_frames_ref = av_hwframe_ctx_alloc(m_hwDeviceCtx);
        if (hw_frames_ref == nullptr) {
            qWarning() << "Failed to allocate hardware frame context";
            cleanup();
            return false;
        }
        auto *frames_ctx = reinterpret_cast<AVHWFramesContext *>(hw_frames_ref->data);

        // Pixel format setup per codec.
        if (config.codecName.contains(QLatin1String("vaapi"))) {
            frames_ctx->format = AV_PIX_FMT_VAAPI;
            frames_ctx->sw_format = AV_PIX_FMT_NV12;
        } else if (config.codecName.contains(QLatin1String("nvenc"))) {
            frames_ctx->format = AV_PIX_FMT_CUDA;
            frames_ctx->sw_format = AV_PIX_FMT_NV12;
        } else if (config.codecName.contains(QLatin1String("qsv"))) {
            frames_ctx->format = AV_PIX_FMT_QSV;
            frames_ctx->sw_format = AV_PIX_FMT_NV12;
        }

        frames_ctx->width = config.width;
        frames_ctx->height = config.height;
        frames_ctx->initial_pool_size = SettingsManager::instance().value(QStringLiteral("hwFramePoolSize"), 32).toInt();

        if (av_hwframe_ctx_init(hw_frames_ref) >= 0) {
            m_encCtx->hw_frames_ctx = hw_frames_ref;
        } else {
            qWarning() << "Failed to initialize hardware frame context";
            av_buffer_unref(&hw_frames_ref);
        }
    }

    m_encCtx->width = config.width;
    m_encCtx->height = config.height;
    m_encCtx->time_base = {.num = config.fps_den, .den = config.fps_num};
    m_stream->time_base = m_encCtx->time_base;
    m_stream->avg_frame_rate = {.num = config.fps_num, .den = config.fps_den};
    m_stream->r_frame_rate = m_stream->avg_frame_rate;

    // ピクセルフォーマットの自動選択
    if (m_encCtx->hw_frames_ctx != nullptr) {
        m_encCtx->pix_fmt = reinterpret_cast<AVHWFramesContext *>(m_encCtx->hw_frames_ctx->data)->format;
    } else {
        // SWエンコードのデフォルト: 可能な限り10bit以上の高精度フォーマットを優先選択
        m_encCtx->pix_fmt = AV_PIX_FMT_YUV420P;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        if (codec->pix_fmts != nullptr) {
            m_encCtx->pix_fmt = codec->pix_fmts[0]; // 互換性のためデフォルトを最初の候補に
            for (const enum AVPixelFormat *p = codec->pix_fmts; *p != AV_PIX_FMT_NONE; p++) {
                const AVPixFmtDescriptor *desc = av_pix_fmt_desc_get(*p);
                if (desc && !(desc->flags & AV_PIX_FMT_FLAG_RGB) && desc->comp[0].depth >= 10) {
                    m_encCtx->pix_fmt = *p;
                    break;
                }
            }
        }
#pragma clang diagnostic pop
    }

    if (config.crf >= 0) {
        // CRF モード: libx264/libx265/libaom 系ソフトウェアエンコーダ向け
        av_opt_set_int(m_encCtx->priv_data, "crf", config.crf, 0);
        // HWエンコーダではフォールバックとして qp を設定
        av_opt_set_int(m_encCtx->priv_data, "qp", config.crf, 0);
    } else {
        m_encCtx->bit_rate = config.bitrate;
        m_encCtx->rc_max_rate = config.bitrate;
        m_encCtx->rc_buffer_size = static_cast<int>(config.bitrate / 2); // 0.5秒バッファ
    }

    // Set encoding preset if specified
    if (!config.preset.isEmpty()) {
        av_opt_set(m_encCtx->priv_data, "preset", config.preset.toStdString().c_str(), 0);
    }

    // Set H.264/H.265 profile if specified
    if (!config.profile.isEmpty()) {
        av_opt_set(m_encCtx->priv_data, "profile", config.profile.toStdString().c_str(), 0);
    }

    // グローバルヘッダーが必要なコンテナ(mp4等)の場合
    if ((m_fmtCtx->oformat->flags & AVFMT_GLOBALHEADER) != 0) {
        m_encCtx->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    }

    if (avcodec_open2(m_encCtx, codec, nullptr) < 0) {
        qWarning() << "Could not open codec.";
        cleanup();
        return false;
    }

    if (avcodec_parameters_from_context(m_stream->codecpar, m_encCtx) < 0) {
        qWarning() << "Failed to copy video codec parameters to stream";
        cleanup();
        return false;
    }

    if ((m_fmtCtx->oformat->flags & AVFMT_NOFILE) == 0) {
        if (avio_open(&m_fmtCtx->pb, config.outputUrl.toStdString().c_str(), AVIO_FLAG_WRITE) < 0) {
            qWarning() << "Could not open output file:" << config.outputUrl;
            cleanup();
            return false;
        }
    }

    m_swFrame = av_frame_alloc();
    if (m_encCtx->hw_frames_ctx != nullptr) {
        m_swFrame->format = reinterpret_cast<AVHWFramesContext *>(m_encCtx->hw_frames_ctx->data)->sw_format;
    } else {
        m_swFrame->format = m_encCtx->pix_fmt;
    }
    m_swFrame->width = config.width;
    m_swFrame->height = config.height;
    if (av_frame_get_buffer(m_swFrame, 32) < 0) {
        qWarning() << "Failed to allocate SW frame buffer.";
        cleanup();
        return false;
    }

    m_hwFrame = av_frame_alloc(); // For HW upload

    qCInfo(lcVideoEncoder) << "VideoEncoder opened using codec:" << config.codecName;

    // エンコードスレッド開始
    m_stopEncoding = false;
    m_errorOccurred = false;
    {
        std::scoped_lock qlock(m_queueMutex);
        std::queue<EncodeTask> empty;
        std::swap(m_taskQueue, empty);
    }
    m_workerThread = std::thread(&VideoEncoder::encodingLoop, this);
    return true;
}

auto VideoEncoder::addAudioStream(int sampleRate, int channels) -> bool {
    std::scoped_lock lock(m_mutex);
    if (m_fmtCtx == nullptr) {
        return false;
    }

    const AVCodec *codec = avcodec_find_encoder_by_name(m_config.audioCodecName.toStdString().c_str());
    if (codec == nullptr) {
        codec = avcodec_find_encoder(AV_CODEC_ID_AAC); // フォールバック
    }

    if (codec == nullptr) {
        qWarning() << "AAC codec not found.";
        return false;
    }

    m_audioStream = avformat_new_stream(m_fmtCtx, codec);
    if (m_audioStream == nullptr) {
        return false;
    }

    m_audioEncCtx = avcodec_alloc_context3(codec);
    if (m_audioEncCtx == nullptr) {
        return false;
    }

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    m_audioEncCtx->sample_fmt = (codec->sample_fmts != nullptr) ? codec->sample_fmts[0] : AV_SAMPLE_FMT_FLTP;
#pragma clang diagnostic pop
    m_audioEncCtx->bit_rate = m_config.audioBitrate;
    m_audioEncCtx->sample_rate = sampleRate;
    av_channel_layout_default(&m_audioEncCtx->ch_layout, channels);
    m_audioStream->time_base = {.num = 1, .den = sampleRate};

    if ((m_fmtCtx->oformat->flags & AVFMT_GLOBALHEADER) != 0) {
        m_audioEncCtx->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    }

    if (avcodec_open2(m_audioEncCtx, codec, nullptr) < 0) {
        qWarning() << "Could not open audio codec.";
        return false;
    }

    if (avcodec_parameters_from_context(m_audioStream->codecpar, m_audioEncCtx) < 0) {
        qWarning() << "Failed to copy audio codec parameters to stream";
        return false;
    }

    // FIFO and resampler initialization
    m_audioFifo = av_audio_fifo_alloc(m_audioEncCtx->sample_fmt, channels, 1024);
    if (m_audioFifo == nullptr) {
        qWarning() << "Failed to allocate audio FIFO";
        return false;
    }
    m_audioFrame = av_frame_alloc();
    m_audioFrame->nb_samples = m_audioEncCtx->frame_size;
    m_audioFrame->format = m_audioEncCtx->sample_fmt;
    m_audioFrame->ch_layout = m_audioEncCtx->ch_layout;
    m_audioFrame->sample_rate = m_audioEncCtx->sample_rate;
    if (av_frame_get_buffer(m_audioFrame, 0) < 0) {
        qWarning() << "Failed to allocate audio frame buffer.";
        return false;
    }

    // Input (Float Interleaved) -> Output (Encoder Format, likely FLTP)
    if (swr_alloc_set_opts2(&m_swrCtx, &m_audioEncCtx->ch_layout, m_audioEncCtx->sample_fmt, m_audioEncCtx->sample_rate, &m_audioEncCtx->ch_layout, AV_SAMPLE_FMT_FLT, sampleRate, 0, nullptr) < 0) {
        qWarning() << "Failed to allocate audio resampler.";
        return false;
    }
    if (swr_init(m_swrCtx) < 0) {
        qWarning() << "Failed to initialize audio resampler.";
        return false;
    }

    qCInfo(lcVideoEncoder) << "Audio stream added: AAC" << sampleRate << "Hz";
    return true;
}

auto VideoEncoder::writeHeaderIfNeeded() -> bool {
    if (m_headerWritten) {
        return true;
    }
    if (avformat_write_header(m_fmtCtx, nullptr) < 0) {
        qWarning() << "Error occurred when opening output file.";
        return false;
    }
    m_headerWritten = true;
    return true;
}

auto VideoEncoder::pushFrame(const QImage &img, int64_t pts) -> bool {
    if (m_errorOccurred) {
        return false;
    }
    if (!m_fmtCtx || !m_encCtx) {
        qWarning() << "VideoEncoder::pushFrame called before open().";
        return false;
    }

    std::unique_lock<std::mutex> lock(m_queueMutex);
    // バックプレッシャー: キューがいっぱいなら消費されるのを待つ
    m_queuePushCv.wait(lock, [this] -> bool { return m_taskQueue.size() < MAX_QUEUE_SIZE || m_stopEncoding; });

    if (m_stopEncoding) {
        return false;
    }

    EncodeTask task;
    task.type = EncodeTask::Video;
    task.videoImg = img;
    task.videoPts = pts;
    m_taskQueue.push(task);
    lock.unlock();
    m_queueCv.notify_one();
    return true;
}

auto VideoEncoder::processVideo(const QImage &img, int64_t pts) -> bool {
    QImage sourceImg = img;
    AVPixelFormat srcPixFmt = AV_PIX_FMT_NONE;
    switch (img.format()) {
    case QImage::Format_RGBA8888:
    case QImage::Format_RGBX8888:
        srcPixFmt = AV_PIX_FMT_RGBA;
        break;
    case QImage::Format_ARGB32:
#if Q_BYTE_ORDER == Q_LITTLE_ENDIAN
        // QtのFormat_ARGB32はリトルエンド環境ではB-G-R-Aの順序(BGRA)
        srcPixFmt = AV_PIX_FMT_BGRA;
#else
        // ビッグエンド環境では正確なマッピングが複雑なためフォールバック
        sourceImg = img.convertToFormat(QImage::Format_RGBA8888);
        srcPixFmt = AV_PIX_FMT_RGBA;
#endif
        break;
    case QImage::Format_RGB888:
        srcPixFmt = AV_PIX_FMT_RGB24;
        break;
    case QImage::Format_RGBA16FPx4:
#if Q_BYTE_ORDER == Q_LITTLE_ENDIAN
        srcPixFmt = AV_PIX_FMT_RGBAF16LE;
#else
        srcPixFmt = AV_PIX_FMT_RGBAF16BE;
#endif
        break;
    case QImage::Format_RGBA64:
#if Q_BYTE_ORDER == Q_LITTLE_ENDIAN
        srcPixFmt = AV_PIX_FMT_RGBA64LE;
#else
        srcPixFmt = AV_PIX_FMT_RGBA64BE;
#endif
        break;
    default:
        // 未対応/プレマルチプライドフォーマットはRGBA8888に変換してフォールバック
        sourceImg = img.convertToFormat(QImage::Format_RGBA8888);
        srcPixFmt = AV_PIX_FMT_RGBA;
        break;
    }

    // 内部スレッドで実行される実際の映像エンコード処理
    std::scoped_lock lock(m_mutex);
    if (m_encCtx == nullptr) {
        return false;
    }

    if (!writeHeaderIfNeeded()) {
        return false;
    }

    if (m_swsCtx == nullptr || m_swsSrcFmt != static_cast<int>(srcPixFmt)) {
        if (m_swsCtx != nullptr) {
            sws_freeContext(m_swsCtx);
        }
        m_swsCtx = sws_getContext(sourceImg.width(), sourceImg.height(), srcPixFmt, m_config.width, m_config.height, static_cast<AVPixelFormat>(m_swFrame->format), SWS_BILINEAR, nullptr, nullptr, nullptr);
        m_swsSrcFmt = static_cast<int>(srcPixFmt);
    }

    // SWフレームを書き込み可能にする
    if (av_frame_make_writable(m_swFrame) < 0) {
        return false;
    }

    // QImageのメモリレイアウトに合わせる
    const uint8_t *srcData[1] = {sourceImg.bits()};
    int srcLinesize[1] = {static_cast<int>(sourceImg.bytesPerLine())};

    // 変換実行
    sws_scale(m_swsCtx, srcData, srcLinesize, 0, sourceImg.height(), m_swFrame->data, m_swFrame->linesize);
    m_swFrame->pts = m_encodedFrameCount++;

    AVFrame *encodeFrame = m_swFrame;

    if (m_encCtx->hw_frames_ctx != nullptr) {
        if (av_hwframe_get_buffer(m_encCtx->hw_frames_ctx, m_hwFrame, 0) < 0) {
            qWarning() << "Failed to allocate HW frame.";
            return false;
        }
        if (av_hwframe_transfer_data(m_hwFrame, m_swFrame, 0) < 0) {
            qWarning() << "Failed to transfer data to GPU.";
            av_frame_unref(m_hwFrame);
            return false;
        }
        m_hwFrame->pts = m_swFrame->pts;
        encodeFrame = m_hwFrame;
    }

    int ret = avcodec_send_frame(m_encCtx, encodeFrame);

    // Drain output when the encoder signals EAGAIN.
    while (ret == AVERROR(EAGAIN)) {
        bool packetRead = false;
        while (true) {
            AvPacketPtr pkt(av_packet_alloc());
            if (!pkt) {
                return false;
            }
            int rxRet = avcodec_receive_packet(m_encCtx, pkt.get());
            if (rxRet == AVERROR(EAGAIN) || rxRet == AVERROR_EOF) {
                break;
            }
            if (rxRet < 0) {
                return false;
            }

            if (pkt->duration == 0) {
                pkt->duration = 1;
            }
            av_packet_rescale_ts(pkt.get(), m_encCtx->time_base, m_stream->time_base);
            pkt->stream_index = m_stream->index;
            int writeRet = av_interleaved_write_frame(m_fmtCtx, pkt.get());
            if (writeRet < 0) {
                char errbuf[AV_ERROR_MAX_STRING_SIZE] = {0};
                av_strerror(writeRet, errbuf, sizeof(errbuf));
                qWarning() << "av_interleaved_write_frame failed:" << errbuf;
                m_errorOccurred = true;
                return false;
            }
            packetRead = true;
        }
        if (!packetRead) {
            break; // No progress; avoid busy-looping.
        }
        ret = avcodec_send_frame(m_encCtx, encodeFrame);
    }

    if (encodeFrame == m_hwFrame) {
        av_frame_unref(m_hwFrame);
    }

    if (ret < 0) {
        qWarning() << "Error sending frame to codec:" << ret;
        m_errorOccurred = true;
        return false;
    }

    while (true) {
        AvPacketPtr pkt(av_packet_alloc());
        if (!pkt) {
            return false;
        }
        ret = avcodec_receive_packet(m_encCtx, pkt.get());
        if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
            break;
        }
        if (ret < 0) {
            qWarning() << "Error during encoding.";
            m_errorOccurred = true;
            return false;
        }

        // Fallback for missing packet duration (constant frame rate).
        if (pkt->duration == 0) {
            pkt->duration = 1;
        }

        av_packet_rescale_ts(pkt.get(), m_encCtx->time_base, m_stream->time_base);
        pkt->stream_index = m_stream->index;

        int writeRet = av_interleaved_write_frame(m_fmtCtx, pkt.get());
        if (writeRet < 0) {
            char errbuf[AV_ERROR_MAX_STRING_SIZE] = {0};
            av_strerror(writeRet, errbuf, sizeof(errbuf));
            qWarning() << "av_interleaved_write_frame failed:" << errbuf;
            m_errorOccurred = true;
            return false;
        }
    }

    return true;
}

auto VideoEncoder::pushAudio(const float *samples, int sampleCount) -> bool {
    if (m_errorOccurred) {
        return false;
    }

    std::unique_lock<std::mutex> lock(m_queueMutex);
    m_queuePushCv.wait(lock, [this] -> bool { return m_taskQueue.size() < MAX_QUEUE_SIZE || m_stopEncoding; });

    if (m_stopEncoding) {
        return false;
    }

    EncodeTask task;
    task.type = EncodeTask::Audio;
    task.audioSamples.assign(samples, samples + sampleCount);
    m_taskQueue.push(task);
    lock.unlock();
    m_queueCv.notify_one();
    return true;
}

auto VideoEncoder::processAudio(const std::vector<float> &samples) -> bool {
    std::scoped_lock lock(m_mutex);
    if ((m_audioEncCtx == nullptr) || (m_audioFifo == nullptr)) {
        return false;
    }

    // Sanitize floats only when needed to avoid an extra copy in the common case.
    const std::vector<float> *inputSamples = &samples;
    std::vector<float> sanitized;
    if (!std::all_of(samples.begin(), samples.end(), [](float v) { return std::isfinite(v); })) {
        sanitized.reserve(samples.size());
        for (float sample : samples) {
            sanitized.push_back(std::isfinite(sample) ? sample : 0.0F);
        }
        inputSamples = &sanitized;
    }

    // Convert interleaved float input to the encoder sample format.
    uint8_t **convertedData = nullptr;
    int linesize = 0;
    const auto convertedDataGuard = qScopeGuard([&convertedData, &linesize]() {
        if (convertedData != nullptr) {
            av_freep(&convertedData[0]);
            av_freep(&convertedData);
        }
    });

    const int sampleCount = static_cast<int>(inputSamples->size() / m_audioEncCtx->ch_layout.nb_channels);
    if (sampleCount <= 0) {
        return true;
    }
    if (av_samples_alloc_array_and_samples(&convertedData, &linesize, m_audioEncCtx->ch_layout.nb_channels, sampleCount, m_audioEncCtx->sample_fmt, 0) < 0) {
        return false;
    }

    const uint8_t *inputData[1] = {reinterpret_cast<const uint8_t *>(inputSamples->data())};
    int converted = swr_convert(m_swrCtx, convertedData, sampleCount, inputData, sampleCount);
    if (converted < 0) {
        qWarning() << "[VideoEncoder] swr_convert failed";
        return false;
    }

    if (av_audio_fifo_write(m_audioFifo, const_cast<void **>(reinterpret_cast<void **>(convertedData)), converted) < 0) {
        qWarning() << "[VideoEncoder] av_audio_fifo_write failed";
        return false;
    }

    while (av_audio_fifo_size(m_audioFifo) >= m_audioEncCtx->frame_size) {
        if (av_frame_make_writable(m_audioFrame) < 0) {
            break;
        }

        if (av_audio_fifo_read(m_audioFifo, reinterpret_cast<void **>(m_audioFrame->data), m_audioEncCtx->frame_size) < 0) {
            qWarning() << "[VideoEncoder] av_audio_fifo_read failed";
            break;
        }

        m_audioFrame->pts = m_audioPts;
        m_audioPts += m_audioFrame->nb_samples;

        int ret = avcodec_send_frame(m_audioEncCtx, m_audioFrame);

        // Drain output when the encoder signals EAGAIN.
        while (ret == AVERROR(EAGAIN)) {
            bool packetRead = false;
            while (true) {
                AvPacketPtr pkt(av_packet_alloc());
                if (!pkt) {
                    return false;
                }
                int rxRet = avcodec_receive_packet(m_audioEncCtx, pkt.get());
                if (rxRet == AVERROR(EAGAIN) || rxRet == AVERROR_EOF) {
                    break;
                }
                if (rxRet < 0) {
                    return false;
                }

                av_packet_rescale_ts(pkt.get(), m_audioEncCtx->time_base, m_audioStream->time_base);
                pkt->stream_index = m_audioStream->index;
                int writeRet = av_interleaved_write_frame(m_fmtCtx, pkt.get());
                if (writeRet < 0) {
                    char errbuf[AV_ERROR_MAX_STRING_SIZE] = {0};
                    av_strerror(writeRet, errbuf, sizeof(errbuf));
                    qWarning() << "[VideoEncoder] av_interleaved_write_frame failed:" << errbuf;
                    m_errorOccurred = true;
                    return false;
                }
                packetRead = true;
            }
            if (!packetRead) {
                break;
            }
            ret = avcodec_send_frame(m_audioEncCtx, m_audioFrame);
        }

        if (ret < 0) {
            qWarning() << "Error sending audio frame to codec:" << ret;
            m_errorOccurred = true;
            return false;
        }

        while (true) {
            AvPacketPtr pkt(av_packet_alloc());
            if (!pkt) {
                return false;
            }
            int rxRet = avcodec_receive_packet(m_audioEncCtx, pkt.get());
            if (rxRet == AVERROR(EAGAIN) || rxRet == AVERROR_EOF) {
                break;
            }
            if (rxRet < 0) {
                m_errorOccurred = true;
                return false;
            }

            av_packet_rescale_ts(pkt.get(), m_audioEncCtx->time_base, m_audioStream->time_base);
            pkt->stream_index = m_audioStream->index;
            int writeRet = av_interleaved_write_frame(m_fmtCtx, pkt.get());
            if (writeRet < 0) {
                char errbuf[AV_ERROR_MAX_STRING_SIZE] = {0};
                av_strerror(writeRet, errbuf, sizeof(errbuf));
                qWarning() << "[VideoEncoder] av_interleaved_write_frame failed:" << errbuf;
                m_errorOccurred = true;
                return false;
            }
        }
    }
    return true;
}

void VideoEncoder::encodingLoop() {
    while (true) {
        EncodeTask task;
        {
            std::unique_lock<std::mutex> lock(m_queueMutex);
            m_queueCv.wait(lock, [this] -> bool { return !m_taskQueue.empty() || m_stopEncoding; });

            if (m_taskQueue.empty() && m_stopEncoding) {
                break; // 終了
            }

            task = m_taskQueue.front();
            m_taskQueue.pop();
            // キューに空きができたことを通知
            m_queuePushCv.notify_one();
        }

        bool success = true;
        if (task.type == EncodeTask::Video) {
            success = processVideo(task.videoImg, task.videoPts);
        } else if (task.type == EncodeTask::Audio) {
            success = processAudio(task.audioSamples);
        }

        if (!success) {
            m_errorOccurred = true;
            qWarning() << "Encoding task failed in worker thread.";
        }
    }
}

void VideoEncoder::close() {
    {
        std::scoped_lock lock(m_queueMutex);
        m_stopEncoding = true;
        m_queueCv.notify_all();
        m_queuePushCv.notify_all(); // push待ちも解除
    }

    if (m_workerThread.joinable()) {
        m_workerThread.join();
    }

    std::scoped_lock lock(m_mutex);
    if (m_encCtx == nullptr) {
        return;
    }

    writeHeaderIfNeeded(); // 何も書き込まれずにcloseされた場合の安全策

    // Flush video encoder.
    int ret = avcodec_send_frame(m_encCtx, nullptr);
    while (ret >= 0 || ret == AVERROR(EAGAIN)) {
        AvPacketPtr pkt(av_packet_alloc());
        if (!pkt) {
            break;
        }
        ret = avcodec_receive_packet(m_encCtx, pkt.get());
        if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
            break;
        }
        if (ret < 0) {
            qWarning() << "Error flushing video encoder:" << ret;
            break;
        }
        av_packet_rescale_ts(pkt.get(), m_encCtx->time_base, m_stream->time_base);
        pkt->stream_index = m_stream->index;
        int writeRet = av_interleaved_write_frame(m_fmtCtx, pkt.get());
        if (writeRet < 0) {
            char errbuf[AV_ERROR_MAX_STRING_SIZE] = {0};
            av_strerror(writeRet, errbuf, sizeof(errbuf));
            qWarning() << "av_interleaved_write_frame failed during video flush:" << errbuf;
            m_errorOccurred = true;
            break;
        }
    }

    // Flush audio encoder.
    if (m_audioEncCtx != nullptr) {
        // Drain any remaining samples that did not fill a full encoder frame.
        if (m_audioFifo != nullptr && av_audio_fifo_size(m_audioFifo) > 0) {
            const int remaining = av_audio_fifo_size(m_audioFifo);
            if (av_frame_make_writable(m_audioFrame) >= 0) {
                if (av_audio_fifo_read(m_audioFifo, reinterpret_cast<void **>(m_audioFrame->data), remaining) >= 0) {
                    // Pad with silence up to the encoder frame size if the encoder requires it.
                    if (remaining < m_audioEncCtx->frame_size) {
                        av_samples_set_silence(m_audioFrame->data, remaining,
                                              m_audioEncCtx->frame_size - remaining,
                                              m_audioEncCtx->ch_layout.nb_channels,
                                              m_audioEncCtx->sample_fmt);
                        m_audioFrame->nb_samples = m_audioEncCtx->frame_size;
                    }
                    m_audioFrame->pts = m_audioPts;
                    m_audioPts += m_audioFrame->nb_samples;
                    int sendRet = avcodec_send_frame(m_audioEncCtx, m_audioFrame);
                    if (sendRet < 0) {
                        qWarning() << "Error sending final audio frame:" << sendRet;
                    }
                }
            }
        }

        ret = avcodec_send_frame(m_audioEncCtx, nullptr);
        while (ret >= 0 || ret == AVERROR(EAGAIN)) {
            AvPacketPtr pkt(av_packet_alloc());
            if (!pkt) {
                break;
            }
            ret = avcodec_receive_packet(m_audioEncCtx, pkt.get());
            if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
                break;
            }
            if (ret < 0) {
                qWarning() << "Error flushing audio encoder:" << ret;
                break;
            }
            av_packet_rescale_ts(pkt.get(), m_audioEncCtx->time_base, m_audioStream->time_base);
            pkt->stream_index = m_audioStream->index;
            int writeRet = av_interleaved_write_frame(m_fmtCtx, pkt.get());
            if (writeRet < 0) {
                char errbuf[AV_ERROR_MAX_STRING_SIZE] = {0};
                av_strerror(writeRet, errbuf, sizeof(errbuf));
                qWarning() << "av_interleaved_write_frame failed during audio flush:" << errbuf;
                m_errorOccurred = true;
                break;
            }
        }
    }

    av_write_trailer(m_fmtCtx);
    cleanup();
    qCDebug(lcVideoEncoder) << "VideoEncoder closed.";
}

} // namespace AviQtl::Core