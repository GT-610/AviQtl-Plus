#include "video_frame_store.hpp"
#include <QImage>
#include <QSignalSpy>
#include <QTest>
#include <QThread>
#include <QVideoFrameFormat>
#include <QVideoSink>
#include <thread>

using namespace AviQtl::Core;

class TestVideoFrameStore : public QObject {
    Q_OBJECT

  private slots:
    void setAndGetFrame();
    void safeImageUpdateRunsOnStoreThread();
    void invalidateClearsImageAndVideoSink();
    void evictsOldestImageFrames();
    void evictsOldestVideoFrames();
    void destroyedSinkIsForgotten();
};

namespace {
QVideoFrame makeVideoFrame() {
    return QVideoFrame(QVideoFrameFormat(QSize(2, 2), QVideoFrameFormat::Format_BGRA8888));
}
} // namespace

void TestVideoFrameStore::setAndGetFrame() {
    VideoFrameStore store;
    QImage img(100, 100, QImage::Format_RGB32);
    img.fill(Qt::red);

    store.setFrame("test_key", img);
    QVERIFY(!store.frame("test_key").isNull());

    QImage retrieved = store.frame("test_key");
    QCOMPARE(retrieved.size(), QSize(100, 100));
}

void TestVideoFrameStore::safeImageUpdateRunsOnStoreThread() {
    VideoFrameStore store;
    QSignalSpy spy(&store, &VideoFrameStore::frameUpdated);
    QVERIFY(spy.isValid());

    QThread *signalThread = nullptr;
    connect(&store, &VideoFrameStore::frameUpdated, &store, [&signalThread]() { signalThread = QThread::currentThread(); });

    QByteArray pixels(8 * 6 * 4, '\0');
    QImage image(reinterpret_cast<uchar *>(pixels.data()), 8, 6, 8 * 4, QImage::Format_RGBA8888);
    image.fill(Qt::red);
    std::thread producer([&store, &image]() { store.setFrameSafe(QStringLiteral("worker"), image); });
    producer.join();
    auto *pixelBytes = reinterpret_cast<uchar *>(pixels.data());
    pixelBytes[0] = 0;
    pixelBytes[1] = 0;
    pixelBytes[2] = 255;
    pixelBytes[3] = 255;

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 5'000);
    QCOMPARE(spy.first().at(0).toString(), QStringLiteral("worker"));
    QCOMPARE(signalThread, store.thread());
    QCOMPARE(store.frame(QStringLiteral("worker")).size(), image.size());
    QCOMPARE(store.frame(QStringLiteral("worker")).pixelColor(0, 0), QColor(Qt::red));
}

void TestVideoFrameStore::invalidateClearsImageAndVideoSink() {
    VideoFrameStore store;
    QImage img(10, 10, QImage::Format_RGB32);
    const QVideoFrame videoFrame = makeVideoFrame();
    QVERIFY(videoFrame.isValid());
    QVideoSink sink;
    QSignalSpy spy(&store, &VideoFrameStore::frameUpdated);

    store.setFrame(QStringLiteral("key1"), img);
    store.registerSink(QStringLiteral("key1"), &sink);
    store.setVideoFrameSafe(QStringLiteral("key1"), videoFrame);
    QVERIFY(!store.frame(QStringLiteral("key1")).isNull());
    QVERIFY(sink.videoFrame().isValid());
    spy.clear();

    std::thread invalidator([&store]() { store.invalidateFrame(QStringLiteral("key1")); });
    invalidator.join();

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 5'000);
    QVERIFY(store.frame(QStringLiteral("key1")).isNull());
    QVERIFY(!sink.videoFrame().isValid());
}

void TestVideoFrameStore::evictsOldestImageFrames() {
    VideoFrameStore store;
    const QImage image(1, 1, QImage::Format_ARGB32);

    for (int i = 0; i < 257; ++i) {
        store.setFrame(QString::number(i), image);
    }

    QVERIFY(store.frame(QStringLiteral("0")).isNull());
    QVERIFY(!store.frame(QStringLiteral("1")).isNull());
    QVERIFY(!store.frame(QStringLiteral("256")).isNull());

    store.setFrame(QStringLiteral("1"), image);
    store.setFrame(QStringLiteral("257"), image);
    QVERIFY(!store.frame(QStringLiteral("1")).isNull());
    QVERIFY(store.frame(QStringLiteral("2")).isNull());
}

void TestVideoFrameStore::evictsOldestVideoFrames() {
    VideoFrameStore store;
    const QVideoFrame videoFrame = makeVideoFrame();
    QVERIFY(videoFrame.isValid());

    for (int i = 0; i < 257; ++i) {
        store.setVideoFrameSafe(QString::number(i), videoFrame);
    }

    store.setVideoFrameSafe(QStringLiteral("1"), videoFrame);
    store.setVideoFrameSafe(QStringLiteral("257"), videoFrame);

    QVideoSink evictedSink;
    QVideoSink refreshedSink;
    QVideoSink retainedSink;
    store.registerSink(QStringLiteral("2"), &evictedSink);
    store.registerSink(QStringLiteral("1"), &refreshedSink);
    store.registerSink(QStringLiteral("257"), &retainedSink);
    QVERIFY(!evictedSink.videoFrame().isValid());
    QVERIFY(refreshedSink.videoFrame().isValid());
    QVERIFY(retainedSink.videoFrame().isValid());
}

void TestVideoFrameStore::destroyedSinkIsForgotten() {
    VideoFrameStore store;
    auto *sink = new QVideoSink;
    store.registerSink(QStringLiteral("key"), sink);
    delete sink;

    const QVideoFrame videoFrame = makeVideoFrame();
    QVERIFY(videoFrame.isValid());
    store.setVideoFrameSafe(QStringLiteral("key"), videoFrame);

    QVideoSink replacement;
    store.registerSink(QStringLiteral("key"), &replacement);
    QVERIFY(replacement.videoFrame().isValid());
}

#include "test_video_frame_store.moc"
QTEST_MAIN(TestVideoFrameStore)
