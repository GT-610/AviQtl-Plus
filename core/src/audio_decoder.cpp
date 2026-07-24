#include "audio_decoder.hpp"
#include <QDebug>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QMutexLocker>
#include <QtConcurrent>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>

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
        if (!m_closing.load(std::memory_order_acquire)) {
            m_peakFuture = QtConcurrent::run([this] { buildPeakCache(); });
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
        m_chunkHits.store(0, std::memory_order_release);
        m_chunkMisses.store(0, std::memory_order_release);
        m_decodedChunks.store(0, std::memory_order_release);
        m_chunkEvictions.store(0, std::memory_order_release);
        m_peakCacheComplete.store(false, std::memory_order_release);
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
    m_peakGeneration.fetch_add(1, std::memory_order_acq_rel);
    m_peakCacheComplete.store(false, std::memory_order_release);
    m_totalDurationSec = 0.0;
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
    m_peakGeneration.fetch_add(1, std::memory_order_acq_rel);
    m_peakCacheComplete.store(false, std::memory_order_release);
}

void AudioDecoder::seek(qint64 ms) {
    m_seekTargetMs.store(ms, std::memory_order_release);
    const auto targetChunk = static_cast<int64_t>((static_cast<double>(std::max<qint64>(0, ms)) / 1000.0) / kChunkDurationSec);
    {
        QMutexLocker locker(&m_mutex);
        m_chunkCache.clear();
        m_chunkOrder.clear();
        if (!m_prefetchFuture.isRunning()) {
            m_prefetchFuture = QtConcurrent::run([this, targetChunk] {
                if (!m_closing.load(std::memory_order_acquire)) {
                    decodeChunk(targetChunk);
                }
            });
        }
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
            m_chunkHits.fetch_add(1, std::memory_order_relaxed);
            m_chunkOrder.removeAll(chunkIdx);
            m_chunkOrder.append(chunkIdx);
            return true;
        }
    }

    m_chunkMisses.fetch_add(1, std::memory_order_relaxed);
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

            int outSamples = swr_get_out_samples(m_swrCtx, m_frame->nb_samples);
            if (outSamples < 0) {
                outSamples = static_cast<int>(av_rescale_rnd(m_frame->nb_samples, m_sampleRate, m_decCtx->sample_rate, AV_ROUND_UP));
            }
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
    m_decodedChunks.fetch_add(1, std::memory_order_relaxed);

    return true;
}

void AudioDecoder::evictChunks() {
    while (m_chunkOrder.size() > kMaxCachedChunks) {
        const int64_t evictIdx = m_chunkOrder.takeFirst();
        m_chunkCache.remove(evictIdx);
        m_chunkEvictions.fetch_add(1, std::memory_order_relaxed);
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

auto AudioDecoder::buildBasePeaks(const std::vector<float> &samples) const -> std::vector<PeakEntry> {
    std::vector<PeakEntry> peaks;
    if (samples.empty()) {
        return peaks;
    }

    const int frameCount = static_cast<int>(samples.size() / 2);
    peaks.reserve(static_cast<std::size_t>(frameCount / 32) + 1);
    for (int frame = 0; frame < frameCount; frame += 32) {
        float pMin = 0.0F;
        float pMax = 0.0F;
        const int frameEnd = std::min(frame + 32, frameCount);
        for (int i = frame; i < frameEnd; ++i) {
            const auto leftIdx = static_cast<std::size_t>(i) * 2;
            const float left = samples[leftIdx];
            const float right = samples[leftIdx + 1];
            pMin = std::min({pMin, left, right});
            pMax = std::max({pMax, left, right});
        }
        peaks.push_back({.min = pMin, .max = pMax});
    }

    return peaks;
}

auto AudioDecoder::silentBasePeaksForChunk(int64_t chunkIdx, double totalDurationSec) const -> std::vector<PeakEntry> {
    const double chunkStartSec = static_cast<double>(chunkIdx) * kChunkDurationSec;
    const double chunkEndSec = std::min(chunkStartSec + kChunkDurationSec, totalDurationSec);
    if (chunkEndSec <= chunkStartSec || m_sampleRate <= 0) {
        return {};
    }

    const int frameCount = static_cast<int>(std::ceil((chunkEndSec - chunkStartSec) * static_cast<double>(m_sampleRate)));
    const int peakCount = std::max(0, (frameCount + 31) / 32);
    return std::vector<PeakEntry>(static_cast<std::size_t>(peakCount), PeakEntry{.min = 0.0F, .max = 0.0F});
}

void AudioDecoder::rebuildPeakPyramidFromBase() {
    if (m_peakPyramid.empty()) {
        return;
    }

    while (m_peakPyramid.size() > 1) {
        m_peakPyramid.pop_back();
    }

    for (int level = 0; level < 5; ++level) {
        const auto &prev = m_peakPyramid.back();
        if (prev.peaks.size() < 8) {
            break;
        }

        PeakLevel next;
        next.samplesPerEntry = prev.samplesPerEntry * 8;
        next.peaks.reserve((prev.peaks.size() / 8) + 1);

        for (std::size_t i = 0; i < prev.peaks.size(); i += 8) {
            float pMin = 0.0F;
            float pMax = 0.0F;
            const std::size_t end = std::min(i + 8, prev.peaks.size());
            for (std::size_t j = i; j < end; ++j) {
                pMin = std::min(pMin, prev.peaks[j].min);
                pMax = std::max(pMax, prev.peaks[j].max);
            }
            next.peaks.push_back({.min = pMin, .max = pMax});
        }
        m_peakPyramid.push_back(std::move(next));
    }
}

void AudioDecoder::buildPeakCache() {
    m_peakCacheComplete.store(false, std::memory_order_release);
    const int generation = m_peakGeneration.load(std::memory_order_acquire);
    const double duration = totalDurationSec();
    if (duration <= 0.0 || m_sampleRate <= 0) {
        QMutexLocker locker(&m_mutex);
        if (generation == m_peakGeneration.load(std::memory_order_acquire)) {
            m_peakPyramid.clear();
        }
        return;
    }

    const auto chunkCount = static_cast<int64_t>(std::ceil(duration / kChunkDurationSec));
    {
        QMutexLocker locker(&m_mutex);
        if (generation != m_peakGeneration.load(std::memory_order_acquire)) {
            return;
        }
        m_peakPyramid.clear();
        m_peakPyramid.push_back(PeakLevel{.samplesPerEntry = 32, .peaks = {}});
        m_peakPyramid.front().peaks.reserve(static_cast<std::size_t>((duration * static_cast<double>(m_sampleRate)) / 32.0) + 1);
    }

    for (int64_t chunkIdx = 0; chunkIdx < chunkCount && !m_closing.load(std::memory_order_acquire); ++chunkIdx) {
        if (generation != m_peakGeneration.load(std::memory_order_acquire)) {
            return;
        }
        std::vector<PeakEntry> peaks;
        if (ensureChunk(chunkIdx)) {
            AudioChunk chunkCopy;
            {
                QMutexLocker locker(&m_mutex);
                auto it = m_chunkCache.find(chunkIdx);
                if (it != m_chunkCache.end()) {
                    chunkCopy = it.value();
                }
            }
            peaks = buildBasePeaks(chunkCopy.data);
        }
        if (peaks.empty()) {
            peaks = silentBasePeaksForChunk(chunkIdx, duration);
        }

        {
            QMutexLocker locker(&m_mutex);
            if (generation != m_peakGeneration.load(std::memory_order_acquire) || m_peakPyramid.empty()) {
                return;
            }
            auto &base = m_peakPyramid.front().peaks;
            base.insert(base.end(), peaks.begin(), peaks.end());
        }
    }

    QMutexLocker locker(&m_mutex);
    if (generation == m_peakGeneration.load(std::memory_order_acquire)) {
        rebuildPeakPyramidFromBase();
        m_peakCacheComplete.store(true, std::memory_order_release);
    }
}

auto AudioDecoder::getPeaks(double startSec, double durationSec, int pixelWidth) -> std::vector<float> {
    if (pixelWidth <= 0) {
        return {};
    }

    if (durationSec <= 0.0 || m_sampleRate <= 0) {
        return std::vector<float>(static_cast<std::size_t>(pixelWidth) * 2, 0.0F);
    }

    {
        QMutexLocker locker(&m_mutex);
        if (m_peakPyramid.empty() && !m_peakFuture.isRunning() && m_isReady.load(std::memory_order_acquire)) {
            m_peakFuture = QtConcurrent::run([this] { buildPeakCache(); });
        }
    }

    startSec = std::max(startSec, 0.0);
    const double samplesPerPixel = (durationSec * static_cast<double>(m_sampleRate)) / static_cast<double>(pixelWidth);
    std::vector<float> result;
    result.reserve(static_cast<std::size_t>(pixelWidth) * 2);

    if (samplesPerPixel < 32.0) {
        for (int pixel = 0; pixel < pixelWidth; ++pixel) {
            const double pixelStart = startSec + (durationSec * static_cast<double>(pixel) / static_cast<double>(pixelWidth));
            const double pixelEnd = startSec + (durationSec * static_cast<double>(pixel + 1) / static_cast<double>(pixelWidth));
            const auto startFrame = static_cast<int64_t>(std::floor(pixelStart * static_cast<double>(m_sampleRate)));
            const auto endFrame = std::max<int64_t>(startFrame + 1, static_cast<int64_t>(std::ceil(pixelEnd * static_cast<double>(m_sampleRate))));
            const int sampleCount = static_cast<int>(std::min<int64_t>((endFrame - startFrame) * 2, std::numeric_limits<int>::max()));

            std::vector<float> samples(static_cast<std::size_t>(sampleCount), 0.0F);
            const int written = getSamplesInto(pixelStart, sampleCount, samples.data());

            float pMin = 0.0F;
            float pMax = 0.0F;
            for (int i = 0; i + 1 < written; i += 2) {
                pMin = std::min({pMin, samples[static_cast<std::size_t>(i)], samples[static_cast<std::size_t>(i + 1)]});
                pMax = std::max({pMax, samples[static_cast<std::size_t>(i)], samples[static_cast<std::size_t>(i + 1)]});
            }
            result.push_back(pMin);
            result.push_back(pMax);
        }
        return result;
    }

    PeakLevel levelCopy;
    {
        QMutexLocker locker(&m_mutex);
        if (m_peakPyramid.empty()) {
            return std::vector<float>(static_cast<std::size_t>(pixelWidth) * 2, 0.0F);
        }

        std::size_t levelIdx = 0;
        for (std::size_t i = 0; i < m_peakPyramid.size(); ++i) {
            if (m_peakPyramid[i].samplesPerEntry <= samplesPerPixel) {
                levelIdx = i;
            } else {
                break;
            }
        }
        levelCopy = m_peakPyramid[levelIdx];
    }

    if (levelCopy.peaks.empty()) {
        return std::vector<float>(static_cast<std::size_t>(pixelWidth) * 2, 0.0F);
    }

    for (int pixel = 0; pixel < pixelWidth; ++pixel) {
        const double pixelStart = startSec + (durationSec * static_cast<double>(pixel) / static_cast<double>(pixelWidth));
        const double pixelEnd = startSec + (durationSec * static_cast<double>(pixel + 1) / static_cast<double>(pixelWidth));
        const auto entryStart = static_cast<std::size_t>(std::max(0.0, std::floor((pixelStart * static_cast<double>(m_sampleRate)) / static_cast<double>(levelCopy.samplesPerEntry))));
        const auto entryEnd = static_cast<std::size_t>(std::max(0.0, std::ceil((pixelEnd * static_cast<double>(m_sampleRate)) / static_cast<double>(levelCopy.samplesPerEntry))));

        float pMin = 0.0F;
        float pMax = 0.0F;
        const std::size_t end = std::min(std::max(entryStart + 1, entryEnd), levelCopy.peaks.size());
        if (entryStart < levelCopy.peaks.size()) {
            for (std::size_t i = entryStart; i < end; ++i) {
                pMin = std::min(pMin, levelCopy.peaks[i].min);
                pMax = std::max(pMax, levelCopy.peaks[i].max);
            }
        }
        result.push_back(pMin);
        result.push_back(pMax);
    }

    return result;
}

auto AudioDecoder::totalDurationSec() const -> double {
    QMutexLocker locker(&m_mutex);
    return m_totalDurationSec;
}

auto AudioDecoder::cacheStats() const -> CacheStats {
    CacheStats stats;
    stats.chunkHits = m_chunkHits.load(std::memory_order_relaxed);
    stats.chunkMisses = m_chunkMisses.load(std::memory_order_relaxed);
    stats.decodedChunks = m_decodedChunks.load(std::memory_order_relaxed);
    stats.chunkEvictions = m_chunkEvictions.load(std::memory_order_relaxed);
    stats.maxChunkEntries = kMaxCachedChunks;
    stats.peakCacheComplete = m_peakCacheComplete.load(std::memory_order_acquire);

    QMutexLocker locker(&m_mutex);
    stats.chunkEntries = m_chunkCache.size();
    for (auto it = m_chunkCache.cbegin(); it != m_chunkCache.cend(); ++it) {
        stats.cachedSamples += static_cast<qsizetype>(it.value().data.size());
    }
    stats.peakLevels = static_cast<qsizetype>(m_peakPyramid.size());
    if (!m_peakPyramid.empty()) {
        stats.peakEntries = static_cast<qsizetype>(m_peakPyramid.front().peaks.size());
    }
    return stats;
}

void AudioDecoder::setPlaying(bool playing) {
    Q_UNUSED(playing);
}

} // namespace AviQtl::Core
