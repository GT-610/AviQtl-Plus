#include "audio_decoder.hpp"
#include <QDebug>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QMutexLocker>
#include <QtConcurrent>
#include <algorithm>
#include <cmath>
#include <cstring>

Q_LOGGING_CATEGORY(lcAudioDecoder, "aviqtl.audio_decoder")

namespace AviQtl::Core {

namespace {

auto sourcePathFromUrl(const QUrl &url) -> QString {
    QString path = url.toLocalFile();
    if (path.isEmpty()) {
        path = url.toString();
    }
    return path;
}

auto makeStereoLayout() -> AVChannelLayout {
    AVChannelLayout layout = AV_CHANNEL_LAYOUT_STEREO;
    return layout;
}

} // namespace

AudioDecoder::AudioDecoder(int clipId, const QUrl &source, QObject *parent) : MediaDecoder(clipId, source, parent) {
    // Creation and decoding are separated; callers start via scheduleStart().
}

AudioDecoder::~AudioDecoder() {
    m_closing.store(true, std::memory_order_release);
    if (m_decodeFuture.isRunning()) {
        m_decodeFuture.waitForFinished();
    }
    if (m_peakFuture.isRunning()) {
        m_peakFuture.waitForFinished();
    }
    if (m_prefetchFuture.isRunning()) {
        m_prefetchFuture.waitForFinished();
    }
    closeFFmpeg();
}

void AudioDecoder::startDecoding() {
    m_decodeFuture = QtConcurrent::run([this] {
        {
            QMutexLocker ffmpegLocker(&m_ffmpegMutex);
            closeFFmpeg();
            m_isReady.store(false, std::memory_order_release);
            m_lastError.clear();

            if (!openFile()) {
                const QString reason = m_lastError;
                QMetaObject::invokeMethod(
                    this,
                    [this, reason]() { emit decodingFailed(reason); },
                    Qt::QueuedConnection);
                return;
            }

            m_isReady.store(true, std::memory_order_release);
        }

        QMetaObject::invokeMethod(this, [this]() { emit ready(); }, Qt::QueuedConnection);

        if (!m_closing.load(std::memory_order_acquire)) {
            decodeChunk(0);
        }
    });
}

auto AudioDecoder::openFile() -> bool {
    const QString path = sourcePathFromUrl(m_source);

    if (avformat_open_input(&m_fmtCtx, path.toUtf8().constData(), nullptr, nullptr) < 0) {
        m_lastError = QStringLiteral("avformat_open_input failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        return false;
    }

    if (avformat_find_stream_info(m_fmtCtx, nullptr) < 0) {
        m_lastError = QStringLiteral("avformat_find_stream_info failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }

    m_streamIdx = av_find_best_stream(m_fmtCtx, AVMEDIA_TYPE_AUDIO, -1, -1, nullptr, 0);
    if (m_streamIdx < 0) {
        m_lastError = QStringLiteral("Audio stream not found: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }
    m_stream = m_fmtCtx->streams[m_streamIdx];

    const AVCodec *codec = avcodec_find_decoder(m_stream->codecpar->codec_id);
    if (codec == nullptr) {
        m_lastError = QStringLiteral("No supported codec found: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }

    m_decCtx = avcodec_alloc_context3(codec);
    if (m_decCtx == nullptr) {
        m_lastError = QStringLiteral("avcodec_alloc_context3 failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }
    if (avcodec_parameters_to_context(m_decCtx, m_stream->codecpar) < 0) {
        m_lastError = QStringLiteral("avcodec_parameters_to_context failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }
    if (avcodec_open2(m_decCtx, codec, nullptr) < 0) {
        m_lastError = QStringLiteral("avcodec_open2 failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }

    rebuildSwrContext();
    if (m_swrCtx == nullptr) {
        m_lastError = QStringLiteral("swr_init failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }

    m_frame = av_frame_alloc();
    m_pkt = av_packet_alloc();
    if (m_frame == nullptr || m_pkt == nullptr) {
        m_lastError = QStringLiteral("FFmpeg allocation failed: %1").arg(path);
        qWarning() << "[AudioDecoder]" << m_lastError;
        closeFFmpeg();
        return false;
    }

    double duration = 0.0;
    if (m_stream->duration != AV_NOPTS_VALUE) {
        duration = static_cast<double>(m_stream->duration) * av_q2d(m_stream->time_base);
    } else if (m_fmtCtx->duration != AV_NOPTS_VALUE) {
        duration = static_cast<double>(m_fmtCtx->duration) / static_cast<double>(AV_TIME_BASE);
    }

    {
        QMutexLocker locker(&m_mutex);
        m_totalDurationSec = std::max(0.0, duration);
        m_chunkCache.clear();
        m_chunkOrder.clear();
        m_peakPyramid.clear();
        m_peakBuildData.reset();
    }

    return true;
}

void AudioDecoder::rebuildSwrContext() {
    if (m_swrCtx != nullptr) {
        swr_free(&m_swrCtx);
    }
    if (m_decCtx == nullptr) {
        return;
    }

    m_swrCtx = swr_alloc();
    if (m_swrCtx == nullptr) {
        return;
    }

    AVChannelLayout stereo = makeStereoLayout();
    av_opt_set_chlayout(m_swrCtx, "in_chlayout", &m_decCtx->ch_layout, 0);
    av_opt_set_int(m_swrCtx, "in_sample_rate", m_decCtx->sample_rate, 0);
    av_opt_set_sample_fmt(m_swrCtx, "in_sample_fmt", m_decCtx->sample_fmt, 0);
    av_opt_set_chlayout(m_swrCtx, "out_chlayout", &stereo, 0);
    av_opt_set_int(m_swrCtx, "out_sample_rate", m_sampleRate, 0);
    av_opt_set_sample_fmt(m_swrCtx, "out_sample_fmt", AV_SAMPLE_FMT_FLT, 0);

    if (swr_init(m_swrCtx) < 0) {
        swr_free(&m_swrCtx);
    }
}

void AudioDecoder::closeFFmpeg() {
    if (m_frame != nullptr) {
        av_frame_free(&m_frame);
        m_frame = nullptr;
    }
    if (m_pkt != nullptr) {
        av_packet_free(&m_pkt);
        m_pkt = nullptr;
    }
    if (m_swrCtx != nullptr) {
        swr_free(&m_swrCtx);
        m_swrCtx = nullptr;
    }
    if (m_decCtx != nullptr) {
        avcodec_free_context(&m_decCtx);
        m_decCtx = nullptr;
    }
    if (m_fmtCtx != nullptr) {
        avformat_close_input(&m_fmtCtx);
        m_fmtCtx = nullptr;
    }
    m_stream = nullptr;
    m_streamIdx = -1;

    QMutexLocker locker(&m_mutex);
    m_chunkCache.clear();
    m_chunkOrder.clear();
    m_peakPyramid.clear();
    m_peakBuildData.reset();
    m_totalDurationSec = 0.0;
}

auto AudioDecoder::lastError() const -> QString {
    return m_lastError;
}

void AudioDecoder::setSampleRate(int sampleRate) {
    if (sampleRate <= 0 || m_sampleRate == sampleRate) {
        return;
    }

    QMutexLocker ffmpegLocker(&m_ffmpegMutex);
    m_sampleRate = sampleRate;
    rebuildSwrContext();
    if (m_decCtx != nullptr && m_swrCtx == nullptr) {
        m_lastError = QStringLiteral("swr_init failed after sample-rate change");
        qWarning() << "[AudioDecoder]" << m_lastError;
    }

    QMutexLocker locker(&m_mutex);
    m_chunkCache.clear();
    m_chunkOrder.clear();
    m_peakPyramid.clear();
    m_peakBuildData.reset();
}

void AudioDecoder::seek(qint64 ms) {
    m_seekTargetMs.store(ms, std::memory_order_release);
    {
        QMutexLocker locker(&m_mutex);
        m_chunkCache.clear();
        m_chunkOrder.clear();
    }
    emit seekRequested(ms);
}

auto AudioDecoder::chunkStartSample(int64_t chunkIdx) const -> int64_t {
    return static_cast<int64_t>(std::llround(static_cast<double>(chunkIdx) * kChunkDurationSec * static_cast<double>(m_sampleRate))) * 2;
}

auto AudioDecoder::chunkSampleCount() const -> int {
    return static_cast<int>(std::llround(kChunkDurationSec * static_cast<double>(m_sampleRate))) * 2;
}

auto AudioDecoder::ensureChunk(int64_t chunkIdx) -> bool {
    if (chunkIdx < 0) {
        return false;
    }

    {
        QMutexLocker locker(&m_mutex);
        auto it = m_chunkCache.find(chunkIdx);
        if (it != m_chunkCache.end() && it.value().fullyDecoded) {
            m_chunkOrder.removeAll(chunkIdx);
            m_chunkOrder.append(chunkIdx);
            return true;
        }
    }

    return decodeChunk(chunkIdx);
}

auto AudioDecoder::decodeChunk(int64_t chunkIdx) -> bool {
    if (chunkIdx < 0 || m_closing.load(std::memory_order_acquire)) {
        return false;
    }

    QMutexLocker ffmpegLocker(&m_ffmpegMutex);
    if (m_fmtCtx == nullptr || m_decCtx == nullptr || m_stream == nullptr || m_swrCtx == nullptr || m_frame == nullptr || m_pkt == nullptr) {
        return false;
    }

    const double startTimeSec = static_cast<double>(chunkIdx) * kChunkDurationSec;
    const double durationLimit = totalDurationSec();
    if (durationLimit > 0.0 && startTimeSec >= durationLimit) {
        return false;
    }

    const int samplesNeeded = chunkSampleCount();
    std::vector<float> chunkData;
    chunkData.reserve(static_cast<std::size_t>(samplesNeeded));

    const int64_t targetTimestamp = static_cast<int64_t>(startTimeSec / av_q2d(m_stream->time_base));
    if (avformat_seek_file(m_fmtCtx, m_streamIdx, targetTimestamp, targetTimestamp, targetTimestamp + 1, AVSEEK_FLAG_ANY) < 0) {
        qWarning() << "[AudioDecoder] avformat_seek_file failed for chunk" << chunkIdx;
        return false;
    }
    avcodec_flush_buffers(m_decCtx);
    swr_close(m_swrCtx);
    if (swr_init(m_swrCtx) < 0) {
        m_lastError = QStringLiteral("swr_init failed while decoding chunk");
        return false;
    }

    std::vector<float> convertBuf;
    bool eof = false;

    auto receiveFrames = [&]() -> bool {
        while (chunkData.size() < static_cast<std::size_t>(samplesNeeded)) {
            const int receiveRet = avcodec_receive_frame(m_decCtx, m_frame);
            if (receiveRet == AVERROR(EAGAIN) || receiveRet == AVERROR_EOF) {
                return true;
            }
            if (receiveRet < 0) {
                qWarning() << "[AudioDecoder] avcodec_receive_frame failed";
                return false;
            }

            const int outSamples = static_cast<int>(av_rescale_rnd(m_frame->nb_samples, m_sampleRate, m_decCtx->sample_rate, AV_ROUND_UP));
            convertBuf.resize(static_cast<std::size_t>(std::max(outSamples, 0)) * 2);
            auto *outPtr = reinterpret_cast<uint8_t *>(convertBuf.data());
            const int converted = swr_convert(m_swrCtx, &outPtr, outSamples, const_cast<const uint8_t **>(m_frame->data), m_frame->nb_samples);
            if (converted > 0) {
                const auto convertedSamples = static_cast<std::size_t>(converted) * 2;
                const auto remaining = static_cast<std::size_t>(samplesNeeded) - chunkData.size();
                const auto toCopy = std::min(convertedSamples, remaining);
                chunkData.insert(chunkData.end(), convertBuf.begin(), convertBuf.begin() + static_cast<ptrdiff_t>(toCopy));
            }
            av_frame_unref(m_frame);
        }
        return true;
    };

    while (chunkData.size() < static_cast<std::size_t>(samplesNeeded) && !m_closing.load(std::memory_order_acquire)) {
        const int readRet = av_read_frame(m_fmtCtx, m_pkt);
        if (readRet < 0) {
            eof = true;
            break;
        }

        if (m_pkt->stream_index != m_streamIdx) {
            av_packet_unref(m_pkt);
            continue;
        }

        const int sendRet = avcodec_send_packet(m_decCtx, m_pkt);
        av_packet_unref(m_pkt);
        if (sendRet < 0) {
            qWarning() << "[AudioDecoder] avcodec_send_packet failed";
            return false;
        }
        if (!receiveFrames()) {
            return false;
        }
    }

    if (eof && chunkData.size() < static_cast<std::size_t>(samplesNeeded)) {
        avcodec_send_packet(m_decCtx, nullptr);
        if (!receiveFrames()) {
            return false;
        }
    }

    if (chunkData.size() < static_cast<std::size_t>(samplesNeeded)) {
        const int delaySamples = static_cast<int>(swr_get_delay(m_swrCtx, m_sampleRate));
        if (delaySamples > 0) {
            convertBuf.resize(static_cast<std::size_t>(delaySamples) * 2);
            auto *outPtr = reinterpret_cast<uint8_t *>(convertBuf.data());
            const int flushed = swr_convert(m_swrCtx, &outPtr, delaySamples, nullptr, 0);
            if (flushed > 0) {
                const auto convertedSamples = static_cast<std::size_t>(flushed) * 2;
                const auto remaining = static_cast<std::size_t>(samplesNeeded) - chunkData.size();
                const auto toCopy = std::min(convertedSamples, remaining);
                chunkData.insert(chunkData.end(), convertBuf.begin(), convertBuf.begin() + static_cast<ptrdiff_t>(toCopy));
            }
        }
    }

    {
        QMutexLocker locker(&m_mutex);
        m_chunkCache.insert(chunkIdx, AudioChunk{.index = chunkIdx, .data = std::move(chunkData), .fullyDecoded = true});
        m_chunkOrder.removeAll(chunkIdx);
        m_chunkOrder.append(chunkIdx);
        evictChunks();
    }

    return true;
}

void AudioDecoder::evictChunks() {
    while (m_chunkOrder.size() > kMaxCachedChunks) {
        const int64_t evictIdx = m_chunkOrder.takeFirst();
        m_chunkCache.remove(evictIdx);
    }
}

auto AudioDecoder::getSamplesInto(double startTime, int count, float *out) -> int { // NOLINT(bugprone-easily-swappable-parameters)
    if (out == nullptr || count <= 0) {
        return 0;
    }

    std::fill(out, out + count, 0.0F);

    if (!m_isReady.load(std::memory_order_acquire) || m_sampleRate <= 0) {
        return 0;
    }

    startTime = std::max(startTime, 0.0);
    auto startSample = static_cast<int64_t>(std::floor(startTime * static_cast<double>(m_sampleRate))) * 2;
    if (startSample % 2 != 0) {
        --startSample;
    }

    const int samplesPerChunk = chunkSampleCount();
    if (samplesPerChunk <= 0) {
        return 0;
    }

    const int64_t endSample = startSample + count;
    const int64_t startChunk = startSample / samplesPerChunk;
    const int64_t endChunk = (endSample - 1) / samplesPerChunk;

    for (int64_t chunkIdx = startChunk; chunkIdx <= endChunk; ++chunkIdx) {
        ensureChunk(chunkIdx);
    }

    int written = 0;
    int outOffset = 0;
    int64_t readSample = startSample;

    while (outOffset < count) {
        const int64_t chunkIdx = readSample / samplesPerChunk;
        const int offsetInChunk = static_cast<int>(readSample - (chunkIdx * samplesPerChunk));

        AudioChunk chunkCopy;
        {
            QMutexLocker locker(&m_mutex);
            auto it = m_chunkCache.find(chunkIdx);
            if (it == m_chunkCache.end()) {
                break;
            }
            chunkCopy = it.value();
            m_chunkOrder.removeAll(chunkIdx);
            m_chunkOrder.append(chunkIdx);
        }

        if (offsetInChunk >= static_cast<int>(chunkCopy.data.size())) {
            break;
        }

        const int available = static_cast<int>(chunkCopy.data.size()) - offsetInChunk;
        const int toCopy = std::min(available, count - outOffset);
        std::memcpy(out + outOffset, chunkCopy.data.data() + offsetInChunk, static_cast<std::size_t>(toCopy) * sizeof(float));

        written += toCopy;
        outOffset += toCopy;
        readSample += toCopy;
    }

    const int64_t nextChunk = endChunk + 1;
    {
        QMutexLocker locker(&m_mutex);
        const bool withinDuration = m_totalDurationSec <= 0.0 || (static_cast<double>(nextChunk) * kChunkDurationSec) < m_totalDurationSec;
        if (withinDuration && !m_chunkCache.contains(nextChunk) && !m_prefetchFuture.isRunning()) {
            m_prefetchFuture = QtConcurrent::run([this, nextChunk] {
                if (!m_closing.load(std::memory_order_acquire)) {
                    decodeChunk(nextChunk);
                }
            });
        }
    }

    return written;
}

auto AudioDecoder::getSamples(double startTime, int count) -> std::vector<float> { // NOLINT(bugprone-easily-swappable-parameters)
    std::vector<float> result(static_cast<std::size_t>(std::max(count, 0)), 0.0F);
    getSamplesInto(startTime, count, result.data());
    return result;
}

void AudioDecoder::buildPeakCache() {
    QMutexLocker locker(&m_mutex);
    m_peakPyramid.clear();
}

auto AudioDecoder::getPeaks(double startSec, double durationSec, int pixelWidth) -> std::vector<float> {
    Q_UNUSED(startSec);
    Q_UNUSED(durationSec);
    if (pixelWidth <= 0) {
        return {};
    }

    QMutexLocker locker(&m_mutex);
    if (m_peakPyramid.empty()) {
        return std::vector<float>(static_cast<std::size_t>(pixelWidth) * 2, 0.0F);
    }

    return std::vector<float>(static_cast<std::size_t>(pixelWidth) * 2, 0.0F);
}

auto AudioDecoder::totalDurationSec() const -> double {
    QMutexLocker locker(&m_mutex);
    return m_totalDurationSec;
}

void AudioDecoder::setPlaying(bool playing) {
    Q_UNUSED(playing);
}

} // namespace AviQtl::Core
