#include "video_decoder.hpp"
#include "video_encoder.hpp"
#include "video_frame_store.hpp"
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <QVector3D>
#include <QVideoSink>
#include <array>

using namespace AviQtl::Core;

class TestVideoDecoder : public QObject {
    Q_OBJECT

  private slots:
    void initialStateIsEmpty();
    void decodesEncodedFramesThroughVideoSink();

  private:
    static bool createTestVideo(const QString &path);
    static QVector3D averageColor(const QImage &image);
};

bool TestVideoDecoder::createTestVideo(const QString &path) {
    VideoEncoder encoder;
    VideoEncoder::Config config;
    config.width = 96;
    config.height = 64;
    config.fps_num = 30;
    config.fps_den = 1;
    config.crf = 10;
    config.codecName = QStringLiteral("libx264");
    config.outputUrl = path;
    config.preset = QStringLiteral("ultrafast");
    if (!encoder.open(config))
        return false;

    const std::array<QColor, 4> colors = {QColor(Qt::red), QColor(Qt::yellow), QColor(Qt::cyan), QColor(Qt::blue)};
    for (int frame = 0; frame < static_cast<int>(colors.size()); ++frame) {
        QImage image(config.width, config.height, QImage::Format_RGBA8888);
        image.fill(colors[frame]);
        if (!encoder.pushFrame(image, frame)) {
            encoder.close();
            return false;
        }
    }
    encoder.close();
    return QFile::exists(path) && QFile(path).size() > 0;
}

QVector3D TestVideoDecoder::averageColor(const QImage &image) {
    QVector3D sum;
    const int pixelCount = image.width() * image.height();
    if (pixelCount <= 0)
        return QVector3D(-1.0F, -1.0F, -1.0F);

    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QColor pixel = image.pixelColor(x, y);
            sum += QVector3D(pixel.red(), pixel.green(), pixel.blue());
        }
    }
    return sum / static_cast<float>(pixelCount);
}

void TestVideoDecoder::initialStateIsEmpty() {
    VideoFrameStore store;
    VideoDecoder decoder(1, QUrl::fromLocalFile(QStringLiteral("missing-video.mp4")), &store);
    QVERIFY(!decoder.isReady());
    QCOMPARE(decoder.sourceFps(), 0.0);
    QCOMPARE(decoder.totalFrameCount(), 0);
}

void TestVideoDecoder::decodesEncodedFramesThroughVideoSink() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString videoPath = dir.filePath(QStringLiteral("decoder-roundtrip.mp4"));
    QVERIFY(createTestVideo(videoPath));

    constexpr int clipId = 7;
    VideoFrameStore store;
    QVideoSink sink;
    store.registerSink(QString::number(clipId), &sink);

    VideoDecoder decoder(clipId, QUrl::fromLocalFile(videoPath), &store);
    QSignalSpy readySpy(&decoder, &MediaDecoder::ready);
    QSignalSpy metadataSpy(&decoder, &VideoDecoder::videoMetaReady);
    decoder.scheduleStart();

    QTRY_COMPARE_WITH_TIMEOUT(readySpy.count(), 1, 10'000);
    QTRY_COMPARE_WITH_TIMEOUT(metadataSpy.count(), 1, 10'000);
    const QList<QVariant> metadata = metadataSpy.takeFirst();
    QCOMPARE(metadata.at(0).toInt(), 4);
    QVERIFY(qAbs(metadata.at(1).toDouble() - 30.0) < 0.01);
    QVERIFY(decoder.isReady());
    QCOMPARE(decoder.totalFrameCount(), 4);
    QVERIFY(qAbs(decoder.sourceFps() - 30.0) < 0.01);

    QSignalSpy firstFrameSpy(&decoder, &MediaDecoder::frameReady);
    QSignalSpy firstSinkSpy(&sink, &QVideoSink::videoFrameChanged);
    decoder.seekToFrame(0, decoder.sourceFps());
    QTRY_COMPARE_WITH_TIMEOUT(firstFrameSpy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(firstSinkSpy.count() >= 1, 10'000);
    QCOMPARE(firstFrameSpy.takeFirst().at(0).toInt(), 0);
    const QImage firstImage = sink.videoFrame().toImage();
    QVERIFY(!firstImage.isNull());
    const QVector3D firstColor = averageColor(firstImage);
    QVERIFY2(firstColor.x() > firstColor.z() + 100.0F,
             qPrintable(QStringLiteral("expected red first frame, got r=%1 g=%2 b=%3").arg(firstColor.x()).arg(firstColor.y()).arg(firstColor.z())));

    QSignalSpy lastFrameSpy(&decoder, &MediaDecoder::frameReady);
    QSignalSpy lastSinkSpy(&sink, &QVideoSink::videoFrameChanged);
    decoder.seekToFrame(3, decoder.sourceFps());
    QTRY_COMPARE_WITH_TIMEOUT(lastFrameSpy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(lastSinkSpy.count() >= 1, 10'000);
    QCOMPARE(lastFrameSpy.takeFirst().at(0).toInt(), 3);
    const QImage lastImage = sink.videoFrame().toImage();
    QVERIFY(!lastImage.isNull());
    const QVector3D lastColor = averageColor(lastImage);
    QVERIFY2(lastColor.z() > lastColor.x() + 100.0F,
             qPrintable(QStringLiteral("expected blue last frame, got r=%1 g=%2 b=%3").arg(lastColor.x()).arg(lastColor.y()).arg(lastColor.z())));

    // Seeking back exercises the populated GOP/frame cache rather than only
    // the initial forward decode path.
    QSignalSpy cachedFrameSpy(&decoder, &MediaDecoder::frameReady);
    QSignalSpy cachedSinkSpy(&sink, &QVideoSink::videoFrameChanged);
    decoder.seekToFrame(0, decoder.sourceFps());
    QTRY_COMPARE_WITH_TIMEOUT(cachedFrameSpy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(cachedSinkSpy.count() >= 1, 10'000);
    QCOMPARE(cachedFrameSpy.takeFirst().at(0).toInt(), 0);
    const QVector3D cachedColor = averageColor(sink.videoFrame().toImage());
    QVERIFY2(cachedColor.x() > cachedColor.z() + 100.0F,
             qPrintable(QStringLiteral("expected cached red frame, got r=%1 g=%2 b=%3").arg(cachedColor.x()).arg(cachedColor.y()).arg(cachedColor.z())));
}

QTEST_MAIN(TestVideoDecoder)
#include "test_video_decoder.moc"
