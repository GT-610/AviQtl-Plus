#include <QTest>
#include <QVariant>
#include "media_utils.hpp"

using namespace AviQtl::Core::MediaUtils;

class TestMediaUtils : public QObject {
    Q_OBJECT

private slots:
    // --- isDirectAudioMode ---
    void isDirectAudioMode_true() {
        QVERIFY(isDirectAudioMode(QStringLiteral("直接")));
        QVERIFY(isDirectAudioMode(QStringLiteral("鐩存帴")));
        QVERIFY(isDirectAudioMode(QStringLiteral("モード: 直接")));
    }

    void isDirectAudioMode_false() {
        QVERIFY(!isDirectAudioMode(QStringLiteral("")));
        QVERIFY(!isDirectAudioMode(QStringLiteral("再生")));
        QVERIFY(!isDirectAudioMode(QStringLiteral("normal")));
    }

    // --- isVideoFile ---
    void isVideoFile_supported() {
        QVERIFY(isVideoFile(QStringLiteral("video.mp4")));
        QVERIFY(isVideoFile(QStringLiteral("clip.MOV")));
        QVERIFY(isVideoFile(QStringLiteral("test.avi")));
        QVERIFY(isVideoFile(QStringLiteral("file.mkv")));
        QVERIFY(isVideoFile(QStringLiteral("out.webm")));
        QVERIFY(isVideoFile(QStringLiteral("old.wmv")));
        QVERIFY(isVideoFile(QStringLiteral("/path/to/video.mp4")));
    }

    void isVideoFile_unsupported() {
        QVERIFY(!isVideoFile(QStringLiteral("")));
        QVERIFY(!isVideoFile(QStringLiteral("audio.mp3")));
        QVERIFY(!isVideoFile(QStringLiteral("image.png")));
        QVERIFY(!isVideoFile(QStringLiteral("video.mp4.bak")));
        QVERIFY(!isVideoFile(QStringLiteral("readme.txt")));
    }

    // --- resolveAudioTime ---
    void resolveAudioTime_directMode() {
        QCOMPARE(resolveAudioTime(0.0, true, 5.0, 0.0, 100.0), 5.0);
        QCOMPARE(resolveAudioTime(10.0, true, 3.5, 1.0, 200.0), 3.5);
    }

    void resolveAudioTime_normalMode() {
        // (relTime * (speed / 100.0)) + startTime
        QCOMPARE(resolveAudioTime(0.0, false, 0.0, 1.0, 100.0), 1.0);
        QCOMPARE(resolveAudioTime(2.0, false, 0.0, 0.0, 100.0), 2.0);
        QCOMPARE(resolveAudioTime(2.0, false, 0.0, 0.0, 50.0), 1.0);
        QCOMPARE(resolveAudioTime(2.0, false, 0.0, 1.0, 200.0), 5.0);
    }

    // --- resolveVideoTime ---
    void resolveVideoTime_invalidFps() {
        QCOMPARE(resolveVideoTime(10, 0.0, false, 0.0, 0.0, 100.0), 0.0);
        QCOMPARE(resolveVideoTime(10, -1.0, false, 0.0, 0.0, 100.0), 0.0);
    }

    void resolveVideoTime_directMode() {
        // directFrame / sourceFps
        QCOMPARE(resolveVideoTime(0, 30.0, true, 60.0, 0.0, 100.0), 2.0);
        QCOMPARE(resolveVideoTime(0, 24.0, true, 48.0, 0.0, 100.0), 2.0);
    }

    void resolveVideoTime_normalMode() {
        // startSec + (relTime * (speed / 100.0))
        // relFrame=30, sourceFps=30, startFrame=0, speed=100 -> 0 + (30/30 * 1.0) = 1.0
        QCOMPARE(resolveVideoTime(30, 30.0, false, 0.0, 0.0, 100.0), 1.0);
        // relFrame=30, sourceFps=30, startFrame=60, speed=100 -> 2.0 + 1.0 = 3.0
        QCOMPARE(resolveVideoTime(30, 30.0, false, 0.0, 60.0, 100.0), 3.0);
        // relFrame=30, sourceFps=30, startFrame=0, speed=200 -> 0 + (1.0 * 2.0) = 2.0
        QCOMPARE(resolveVideoTime(30, 30.0, false, 0.0, 0.0, 200.0), 2.0);
        // relFrame=15, sourceFps=30, startFrame=30, speed=50 -> 1.0 + (0.5 * 0.5) = 1.25
        QCOMPARE(resolveVideoTime(15, 30.0, false, 0.0, 30.0, 50.0), 1.25);
    }

    // --- maxVideoDurationFrames ---
    void maxVideoDurationFrames_invalidParams() {
        QCOMPARE(maxVideoDurationFrames(100, 30.0, 0.0, 0.0, 30), 0);
        QCOMPARE(maxVideoDurationFrames(100, 30.0, -1.0, 0.0, 30), 0);
        QCOMPARE(maxVideoDurationFrames(100, 0.0, 100.0, 0.0, 30), 0);
        QCOMPARE(maxVideoDurationFrames(100, 30.0, 100.0, 0.0, 0), 0);
    }

    void maxVideoDurationFrames_basic() {
        // 100 frames at 30fps = 3.333s, speed=100% -> 3.333s * 30fps = 100 frames
        QCOMPARE(maxVideoDurationFrames(100, 30.0, 100.0, 0.0, 30), 100);
    }

    void maxVideoDurationFrames_withStartFrame() {
        // 100 frames at 30fps = 3.333s, startFrame=30 -> startSec=1.0, remaining=2.333s
        // speed=100% -> 2.333s * 30fps = 70 frames
        QCOMPARE(maxVideoDurationFrames(100, 30.0, 100.0, 30.0, 30), 70);
    }

    void maxVideoDurationFrames_withSpeed() {
        // 100 frames at 30fps = 3.333s, speed=200% -> remaining/(200/100) * 30fps = 50 frames
        QCOMPARE(maxVideoDurationFrames(100, 30.0, 200.0, 0.0, 30), 50);
    }

    void maxVideoDurationFrames_startBeyondEnd() {
        // startFrame=200 at 30fps = 6.667s, but total is only 3.333s -> negative remaining
        QCOMPARE(maxVideoDurationFrames(100, 30.0, 100.0, 200.0, 30), 0);
    }

    void maxVideoDurationFrames_differentProjectFps() {
        // 60 frames at 30fps = 2.0s, projectFps=60 -> 2.0s * 60fps = 120 frames
        QCOMPARE(maxVideoDurationFrames(60, 30.0, 100.0, 0.0, 60), 120);
    }
};

QTEST_MAIN(TestMediaUtils)
#include "test_media_utils.moc"
