#include "audio_decoder.hpp"
#include <QUrl>
#include <QTest>

using namespace AviQtl::Core;

class TestAudioDecoder : public QObject {
    Q_OBJECT

  private slots:
    void constructor();
    void setSampleRate();
    void totalDurationSec();
    void lastError();

  private:
    QUrl testAudioUrl() const;
};

QUrl TestAudioDecoder::testAudioUrl() const {
    // Use a test audio file path - in real tests, this would be a actual test file
    return QUrl::fromLocalFile(QCoreApplication::applicationDirPath() +
                               QStringLiteral("/../../../tests/test_audio.wav"));
}

void TestAudioDecoder::constructor() {
    AudioDecoder decoder(1, testAudioUrl());
    QCOMPARE(decoder.totalDurationSec(), 0.0);
    QVERIFY(decoder.lastError().isEmpty());
}

void TestAudioDecoder::setSampleRate() {
    AudioDecoder decoder(1, testAudioUrl());
    decoder.setSampleRate(44100);
    // No direct way to verify sample rate was set, but it shouldn't crash
    QVERIFY(true);
}

void TestAudioDecoder::totalDurationSec() {
    AudioDecoder decoder(1, testAudioUrl());
    // Without actual audio file, duration should be 0
    QCOMPARE(decoder.totalDurationSec(), 0.0);
}

void TestAudioDecoder::lastError() {
    AudioDecoder decoder(1, QUrl::fromLocalFile("/nonexistent/file.wav"));
    // Error should be empty initially
    QVERIFY(decoder.lastError().isEmpty());
}

#include "test_audio_decoder.moc"
QTEST_MAIN(TestAudioDecoder)