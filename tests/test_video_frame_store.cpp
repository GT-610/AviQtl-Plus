#include "video_frame_store.hpp"
#include <QImage>
#include <QTest>

using namespace AviQtl::Core;

class TestVideoFrameStore : public QObject {
    Q_OBJECT

  private slots:
    void setAndGetFrame();
    void invalidateFrame();
};

void TestVideoFrameStore::setAndGetFrame() {
    VideoFrameStore store;
    QImage img(100, 100, QImage::Format_RGB32);
    img.fill(Qt::red);

    store.setFrame("test_key", img);
    QVERIFY(!store.frame("test_key").isNull());

    QImage retrieved = store.frame("test_key");
    QCOMPARE(retrieved.size(), QSize(100, 100));
}

void TestVideoFrameStore::invalidateFrame() {
    VideoFrameStore store;
    QImage img(10, 10, QImage::Format_RGB32);
    store.setFrame("key1", img);
    QVERIFY(!store.frame("key1").isNull());

    store.invalidateFrame("key1");
    QVERIFY(store.frame("key1").isNull());
}

#include "test_video_frame_store.moc"
QTEST_MAIN(TestVideoFrameStore)
