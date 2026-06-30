#include "video_frame_store.hpp"
#include <QImage>
#include <QTest>

using namespace AviQtl::Core;

class TestVideoFrameStore : public QObject {
    Q_OBJECT

  private slots:
    void setAndGetFrame();
    void hasFrame();
    void invalidateFrame();
    void clear();
};

void TestVideoFrameStore::setAndGetFrame() {
    VideoFrameStore store;
    QImage img(100, 100, QImage::Format_RGB32);
    img.fill(Qt::red);

    store.setFrame("test_key", img);
    QVERIFY(store.hasFrame("test_key"));

    QImage retrieved = store.frame("test_key");
    QCOMPARE(retrieved.size(), QSize(100, 100));
}

void TestVideoFrameStore::hasFrame() {
    VideoFrameStore store;
    QVERIFY(!store.hasFrame("nonexistent"));

    QImage img(10, 10, QImage::Format_RGB32);
    store.setFrame("key1", img);
    QVERIFY(store.hasFrame("key1"));
    QVERIFY(!store.hasFrame("key2"));
}

void TestVideoFrameStore::invalidateFrame() {
    VideoFrameStore store;
    QImage img(10, 10, QImage::Format_RGB32);
    store.setFrame("key1", img);
    QVERIFY(store.hasFrame("key1"));

    store.invalidateFrame("key1");
    QVERIFY(!store.hasFrame("key1"));
}

void TestVideoFrameStore::clear() {
    VideoFrameStore store;
    QImage img(10, 10, QImage::Format_RGB32);
    store.setFrame("key1", img);
    store.setFrame("key2", img);

    store.clear();
    QVERIFY(!store.hasFrame("key1"));
    QVERIFY(!store.hasFrame("key2"));
}

#include "test_video_frame_store.moc"
QTEST_MAIN(TestVideoFrameStore)