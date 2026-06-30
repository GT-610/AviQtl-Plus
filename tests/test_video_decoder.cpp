#include "video_decoder.hpp"
#include "video_frame_store.hpp"
#include <QUrl>
#include <QTest>

using namespace AviQtl::Core;

class TestVideoDecoder : public QObject {
    Q_OBJECT

  private slots:
    void constructor();
    void sourceFps();
    void totalFrameCount();

  private:
    QUrl testVideoUrl() const;
};

QUrl TestVideoDecoder::testVideoUrl() const {
    return QUrl::fromLocalFile(QCoreApplication::applicationDirPath() +
                               QStringLiteral("/../../../tests/test_video.mp4"));
}

void TestVideoDecoder::constructor() {
    VideoFrameStore store;
    VideoDecoder decoder(1, testVideoUrl(), &store);
    QCOMPARE(decoder.sourceFps(), 0.0);
    QCOMPARE(decoder.totalFrameCount(), 0);
}

void TestVideoDecoder::sourceFps() {
    VideoFrameStore store;
    VideoDecoder decoder(1, testVideoUrl(), &store);
    QCOMPARE(decoder.sourceFps(), 0.0);
}

void TestVideoDecoder::totalFrameCount() {
    VideoFrameStore store;
    VideoDecoder decoder(1, testVideoUrl(), &store);
    QCOMPARE(decoder.totalFrameCount(), 0);
}

#include "test_video_decoder.moc"
QTEST_MAIN(TestVideoDecoder)