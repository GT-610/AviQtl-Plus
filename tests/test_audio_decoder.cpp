#include "audio_decoder.hpp"
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QUrl>
#include <QTest>
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

    for (int i = 0; i < frameCount; ++i) {
        const auto left = static_cast<qint16>(std::sin(static_cast<double>(i) * 0.01) * 12000.0);
        const auto right = static_cast<qint16>(std::cos(static_cast<double>(i) * 0.01) * 12000.0);
        writeLe16(file, static_cast<quint16>(left));
        writeLe16(file, static_cast<quint16>(right));
    }

    return QUrl::fromLocalFile(path);
}

} // namespace

class TestAudioDecoder : public QObject {
    Q_OBJECT

  private slots:
    void constructor();
    void setSampleRate();
    void totalDurationSec();
    void lastError();
    void readyAndGetSamplesIntoReadsAcrossChunks();
    void getSamplesPadsPastEnd();
    void getPeaksBuildsWaveform();
    void getPeaksHighZoomReadsSamples();
};

void TestAudioDecoder::constructor() {
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
    QCOMPARE(decoder.totalDurationSec(), 0.0);
    QVERIFY(decoder.lastError().isEmpty());
}

void TestAudioDecoder::setSampleRate() {
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
    decoder.setSampleRate(44100);
    // No direct way to verify sample rate was set, but it shouldn't crash
    QVERIFY(true);
}

void TestAudioDecoder::totalDurationSec() {
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
    // Duration is unknown until decoding starts.
    QCOMPARE(decoder.totalDurationSec(), 0.0);
}

void TestAudioDecoder::lastError() {
    AudioDecoder decoder(1, QUrl::fromLocalFile("/nonexistent/file.wav"));
    // Error should be empty initially
    QVERIFY(decoder.lastError().isEmpty());
}

void TestAudioDecoder::readyAndGetSamplesIntoReadsAcrossChunks() {
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
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
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
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
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
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
    QTemporaryDir dir;
    AudioDecoder decoder(1, createTestWav(dir));
    QSignalSpy readySpy(&decoder, &AudioDecoder::ready);

    decoder.scheduleStart();
    QTRY_COMPARE(readySpy.count(), 1);

    const std::vector<float> peaks = decoder.getPeaks(0.1, 0.001, 20);
    QCOMPARE(peaks.size(), static_cast<std::size_t>(40));
    QVERIFY(std::any_of(peaks.begin(), peaks.end(), [](float value) { return std::abs(value) > 0.001F; }));
}

#include "test_audio_decoder.moc"
QTEST_MAIN(TestAudioDecoder)
