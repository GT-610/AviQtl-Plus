#include "audio_decoder.hpp"
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTextStream>
#include <QUrl>
#include <algorithm>
#include <array>
#include <cmath>

using namespace AviQtl::Core;

namespace {

constexpr int kTestSampleRate = 48000;
constexpr int kTestChannels = 2;
constexpr int kTestBitsPerSample = 16;

void writeFourCc(QFile &file, const char *fourCc) {
    QCOMPARE(file.write(fourCc, 4), 4);
}

void writeLe16(QFile &file, quint16 value) {
    char bytes[2] = {static_cast<char>(value & 0xFFU), static_cast<char>((value >> 8U) & 0xFFU)};
    QCOMPARE(file.write(bytes, 2), 2);
}

void writeLe32(QFile &file, quint32 value) {
    char bytes[4] = {
        static_cast<char>(value & 0xFFU),
        static_cast<char>((value >> 8U) & 0xFFU),
        static_cast<char>((value >> 16U) & 0xFFU),
        static_cast<char>((value >> 24U) & 0xFFU),
    };
    QCOMPARE(file.write(bytes, 4), 4);
}

QUrl createTestWav(QTemporaryDir &dir, double durationSec = 6.0) {
    const QString path = dir.filePath(QStringLiteral("test_audio.wav"));
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        return {};
    }

    const int frameCount = static_cast<int>(durationSec * kTestSampleRate);
    const quint32 dataBytes = static_cast<quint32>(frameCount * kTestChannels * (kTestBitsPerSample / 8));
    const quint32 byteRate = kTestSampleRate * kTestChannels * (kTestBitsPerSample / 8);
    const quint16 blockAlign = kTestChannels * (kTestBitsPerSample / 8);

    writeFourCc(file, "RIFF");
    writeLe32(file, 36U + dataBytes);
    writeFourCc(file, "WAVE");
    writeFourCc(file, "fmt ");
    writeLe32(file, 16);
    writeLe16(file, 1); // PCM
    writeLe16(file, kTestChannels);
    writeLe32(file, kTestSampleRate);
    writeLe32(file, byteRate);
    writeLe16(file, blockAlign);
    writeLe16(file, kTestBitsPerSample);
    writeFourCc(file, "data");
    writeLe32(file, dataBytes);

    constexpr int framesPerBlock = 4096;
    QByteArray block;
    for (int firstFrame = 0; firstFrame < frameCount; firstFrame += framesPerBlock) {
        const int blockFrames = std::min(framesPerBlock, frameCount - firstFrame);
        block.resize(blockFrames * kTestChannels * (kTestBitsPerSample / 8));
        for (int frame = 0; frame < blockFrames; ++frame) {
            const int sampleIndex = firstFrame + frame;
            const auto left = static_cast<quint16>(static_cast<qint16>(std::sin(static_cast<double>(sampleIndex) * 0.01) * 12000.0));
            const auto right = static_cast<quint16>(static_cast<qint16>(std::cos(static_cast<double>(sampleIndex) * 0.01) * 12000.0));
            char *out = block.data() + (frame * 4);
            out[0] = static_cast<char>(left & 0xFFU);
            out[1] = static_cast<char>((left >> 8U) & 0xFFU);
            out[2] = static_cast<char>(right & 0xFFU);
            out[3] = static_cast<char>((right >> 8U) & 0xFFU);
        }
        if (file.write(block) != block.size()) {
            return {};
        }
    }

    return QUrl::fromLocalFile(path);
}

} // namespace

class TestAudioDecoder : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void durationUnknownBeforeStart();
    void sampleRateChangeKeepsDecoderReadable();
    void readyAndGetSamplesIntoReadsAcrossChunks();
    void getSamplesPadsPastEnd();
    void getPeaksBuildsWaveform();
    void getPeaksHighZoomReadsSamples();
    void representativeLongAudioWorkload();

  private:
    QTemporaryDir m_dir;
    QUrl m_source;
};

void TestAudioDecoder::initTestCase() {
    QVERIFY(m_dir.isValid());
    m_source = createTestWav(m_dir);
    QVERIFY(m_source.isValid());
}

void TestAudioDecoder::durationUnknownBeforeStart() {
    AudioDecoder decoder(1, m_source);
    QCOMPARE(decoder.totalDurationSec(), 0.0);
}

void TestAudioDecoder::sampleRateChangeKeepsDecoderReadable() {
    AudioDecoder decoder(1, m_source);
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);
    decoder.scheduleStart();
    QTRY_COMPARE(readySpy.count(), 1);

    decoder.setSampleRate(44100);
    constexpr int sampleCount = 4410 * 2;
    std::vector<float> samples(sampleCount, 0.0F);
    QCOMPARE(decoder.getSamplesInto(0.1, sampleCount, samples.data()), sampleCount);
    QVERIFY(std::any_of(samples.begin(), samples.end(), [](float value) { return std::abs(value) > 0.001F; }));
}

void TestAudioDecoder::readyAndGetSamplesIntoReadsAcrossChunks() {
    AudioDecoder decoder(1, m_source);
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);

    decoder.scheduleStart();
    QTRY_COMPARE(readySpy.count(), 1);
    QVERIFY(std::abs(decoder.totalDurationSec() - 6.0) < 0.05);

    constexpr int frameCount = 2000;
    std::vector<float> samples(static_cast<std::size_t>(frameCount) * 2, 0.0F);
    const int written = decoder.getSamplesInto(3.99, static_cast<int>(samples.size()), samples.data());

    QCOMPARE(written, static_cast<int>(samples.size()));
    QVERIFY(std::any_of(samples.begin(), samples.end(), [](float value) { return std::abs(value) > 0.001F; }));
}

void TestAudioDecoder::getSamplesPadsPastEnd() {
    AudioDecoder decoder(1, m_source);
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);

    decoder.scheduleStart();
    QTRY_COMPARE(readySpy.count(), 1);

    const int count = kTestSampleRate;
    std::vector<float> samples(static_cast<std::size_t>(count), 1.0F);
    const int written = decoder.getSamplesInto(5.95, count, samples.data());

    QVERIFY(written > 0);
    QVERIFY(written < count);
    QVERIFY(std::all_of(samples.begin() + written, samples.end(), [](float value) { return value == 0.0F; }));
}

void TestAudioDecoder::getPeaksBuildsWaveform() {
    AudioDecoder decoder(1, m_source);
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);

    decoder.scheduleStart();
    QTRY_COMPARE(readySpy.count(), 1);

    QTRY_VERIFY_WITH_TIMEOUT(
        [&decoder]() {
            const std::vector<float> peaks = decoder.getPeaks(0.0, 6.0, 120);
            return peaks.size() == 240 && std::any_of(peaks.begin(), peaks.end(), [](float value) { return std::abs(value) > 0.001F; });
        }(),
        5000);
}

void TestAudioDecoder::getPeaksHighZoomReadsSamples() {
    AudioDecoder decoder(1, m_source);
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);

    decoder.scheduleStart();
    QTRY_COMPARE(readySpy.count(), 1);

    const std::vector<float> peaks = decoder.getPeaks(0.1, 0.001, 20);
    QCOMPARE(peaks.size(), static_cast<std::size_t>(40));
    QVERIFY(std::any_of(peaks.begin(), peaks.end(), [](float value) { return std::abs(value) > 0.001F; }));
}

void TestAudioDecoder::representativeLongAudioWorkload() {
    QString audioPath = qEnvironmentVariable("AVIQTL_PERF_AUDIO");
    QTemporaryDir syntheticDir;
    if (audioPath == QStringLiteral("synthetic")) {
        QVERIFY(syntheticDir.isValid());
        QElapsedTimer createTimer;
        createTimer.start();
        const QUrl source = createTestWav(syntheticDir, 120.0);
        QVERIFY(source.isValid());
        audioPath = source.toLocalFile();
        QTextStream(stdout) << "audio_representative create_ms=" << createTimer.elapsed() << " duration_sec=120 sample_rate=" << kTestSampleRate << Qt::endl;
    } else if (audioPath.isEmpty()) {
        QSKIP("Set AVIQTL_PERF_AUDIO to a representative media path, or to 'synthetic'.");
    }
    QVERIFY2(QFileInfo::exists(audioPath), qPrintable(QStringLiteral("representative audio does not exist: %1").arg(audioPath)));

    QElapsedTimer startupTimer;
    startupTimer.start();
    AudioDecoder decoder(2, QUrl::fromLocalFile(audioPath));
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);
    decoder.scheduleStart();
    QTRY_COMPARE_WITH_TIMEOUT(readySpy.count(), 1, 60'000);
    const qint64 startupMs = startupTimer.elapsed();
    const double duration = decoder.totalDurationSec();
    QVERIFY(duration > 0.0);

    constexpr int readFrames = 4800;
    std::vector<float> samples(readFrames * 2, 0.0F);
    const std::array<double, 5> readPositions = {0.0, 0.25, 0.5, 0.75, 0.95};
    QVector<qint64> readTimes;
    for (const double fraction : readPositions) {
        QElapsedTimer readTimer;
        readTimer.start();
        const int written = decoder.getSamplesInto(std::max(0.0, (duration * fraction) - 0.05), static_cast<int>(samples.size()), samples.data());
        readTimes.append(readTimer.elapsed());
        QVERIFY(written > 0);
    }

    QElapsedTimer waveformTimer;
    waveformTimer.start();
    std::vector<float> peaks;
    do {
        peaks = decoder.getPeaks(0.0, duration, 1920);
        if (decoder.cacheStats().peakCacheComplete) {
            break;
        }
        QTest::qWait(20);
    } while (waveformTimer.elapsed() < 60'000);
    QVERIFY(decoder.cacheStats().peakCacheComplete);
    peaks = decoder.getPeaks(0.0, duration, 1920);
    QVERIFY(std::any_of(peaks.cbegin(), peaks.cend(), [](float value) { return std::abs(value) > 0.001F; }));

    std::ranges::sort(readTimes);
    const AudioDecoder::CacheStats stats = decoder.cacheStats();
    QVERIFY(stats.chunkEntries <= stats.maxChunkEntries);
    QVERIFY(stats.cachedSamples <= stats.maxChunkEntries * kTestSampleRate * 2 * 4);
    QVERIFY(stats.peakEntries > 0);
    QTextStream(stdout) << "audio_representative path=" << QFileInfo(audioPath).fileName() << " bytes=" << QFileInfo(audioPath).size() << " duration_sec=" << duration << " startup_ms=" << startupMs << " reads=" << readTimes.size()
                        << " median_read_ms=" << readTimes.at(readTimes.size() / 2) << " max_read_ms=" << readTimes.last() << " waveform_ms=" << waveformTimer.elapsed() << " chunk_hits=" << stats.chunkHits << " chunk_misses=" << stats.chunkMisses
                        << " decoded_chunks=" << stats.decodedChunks << " evictions=" << stats.chunkEvictions << " chunk_entries=" << stats.chunkEntries << " cached_samples=" << stats.cachedSamples << " peak_levels=" << stats.peakLevels
                        << " peak_entries=" << stats.peakEntries << " peak_complete=" << stats.peakCacheComplete << Qt::endl;
}

#include "test_audio_decoder.moc"
QTEST_MAIN(TestAudioDecoder)
