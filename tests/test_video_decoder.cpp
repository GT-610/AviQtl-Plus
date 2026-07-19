#include "settings_manager.hpp"
#include "video_decoder.hpp"
#include "video_encoder.hpp"
#include "video_frame_store.hpp"
#include <QElapsedTimer>
#include <QFile>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QTextStream>
#include <QUrl>
#include <QVector3D>
#include <QVideoSink>
#include <algorithm>
#include <array>
#include <utility>

using namespace AviQtl::Core;

namespace {
constexpr int kTestFrameCount = 60;
constexpr int kTestGopSize = 12;
constexpr int kColdFrame = 48;
constexpr int kEvictionFrameCount = 240;
constexpr qsizetype kEvictionCacheBytes = 2 * 1024 * 1024;
} // namespace

class TestVideoDecoder : public QObject {
    Q_OBJECT

  private slots:
    void initialStateIsEmpty();
    void decodesEncodedFramesThroughVideoSink();
    void frameCacheEvictionStaysWithinBudget();

  private:
    static bool createTestVideo(const QString &path, int frameCount = kTestFrameCount);
    static QVector3D averageColor(const QImage &image);
};

bool TestVideoDecoder::createTestVideo(const QString &path, int frameCount) {
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
    config.gopSize = kTestGopSize;
    if (!encoder.open(config))
        return false;

    const std::array<QColor, 4> colors = {QColor(Qt::red), QColor(Qt::yellow), QColor(Qt::cyan), QColor(Qt::blue)};
    for (int frame = 0; frame < frameCount; ++frame) {
        QImage image(config.width, config.height, QImage::Format_RGBA8888);
        image.fill(colors[static_cast<std::size_t>(frame) % colors.size()]);
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
    QCOMPARE(metadata.at(0).toInt(), kTestFrameCount);
    QVERIFY(qAbs(metadata.at(1).toDouble() - 30.0) < 0.01);
    QVERIFY(decoder.isReady());
    QCOMPARE(decoder.totalFrameCount(), kTestFrameCount);
    QVERIFY(qAbs(decoder.sourceFps() - 30.0) < 0.01);

    QSignalSpy coldFrameSpy(&decoder, &MediaDecoder::frameReady);
    QSignalSpy coldSinkSpy(&sink, &QVideoSink::videoFrameChanged);
    decoder.seekToFrame(kColdFrame, decoder.sourceFps());
    QTRY_COMPARE_WITH_TIMEOUT(coldFrameSpy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(coldSinkSpy.count() >= 1, 10'000);
    QCOMPARE(coldFrameSpy.takeFirst().at(0).toInt(), kColdFrame);
    const QImage coldImage = sink.videoFrame().toImage();
    QVERIFY(!coldImage.isNull());
    const QVector3D coldColor = averageColor(coldImage);
    QVERIFY2(coldColor.x() > coldColor.z() + 100.0F, qPrintable(QStringLiteral("expected red cold frame, got r=%1 g=%2 b=%3").arg(coldColor.x()).arg(coldColor.y()).arg(coldColor.z())));

    const VideoDecoder::CacheStats coldStats = decoder.cacheStats();
    QTextStream(stdout) << "video_decoder cold_seek misses=" << coldStats.misses << " gop_hits=" << coldStats.gopHits << " frame_hits=" << coldStats.frameHits << " decoded_frames=" << coldStats.decodedFrames << " gop_blocks=" << coldStats.gopBlocks
                        << Qt::endl;
    QCOMPARE(coldStats.misses, quint64{1});
    QCOMPARE(coldStats.decodedFrames, quint64{kTestGopSize});
    QCOMPARE(coldStats.gopBlocks, 1);
    QVERIFY(coldStats.frameEntries > 0);
    QVERIFY(coldStats.frameCost <= coldStats.frameMaxCost);

    QElapsedTimer seekTimer;
    QSignalSpy nearbyFrameSpy(&decoder, &MediaDecoder::frameReady);
    QSignalSpy nearbySinkSpy(&sink, &QVideoSink::videoFrameChanged);
    seekTimer.start();
    decoder.seekToFrame(kTestFrameCount - 1, decoder.sourceFps());
    if (nearbyFrameSpy.isEmpty()) {
        QVERIFY(nearbyFrameSpy.wait(10'000));
    }
    if (nearbySinkSpy.isEmpty()) {
        QVERIFY(nearbySinkSpy.wait(10'000));
    }
    const qint64 nearbySeekMs = seekTimer.elapsed();
    QCOMPARE(nearbyFrameSpy.count(), 1);
    QCOMPARE(nearbyFrameSpy.takeFirst().at(0).toInt(), kTestFrameCount - 1);
    const QVector3D nearbyColor = averageColor(sink.videoFrame().toImage());
    QVERIFY2(nearbyColor.z() > nearbyColor.x() + 100.0F, qPrintable(QStringLiteral("expected blue nearby frame, got r=%1 g=%2 b=%3").arg(nearbyColor.x()).arg(nearbyColor.y()).arg(nearbyColor.z())));
    const VideoDecoder::CacheStats nearbyStats = decoder.cacheStats();
    QTextStream(stdout) << "video_decoder nearby_reseek elapsed_ms=" << nearbySeekMs << " misses=" << nearbyStats.misses << " gop_hits=" << nearbyStats.gopHits << " frame_hits=" << nearbyStats.frameHits << " gop_blocks=" << nearbyStats.gopBlocks
                        << Qt::endl;
    QCOMPARE(nearbyStats.misses, quint64{1});
    QCOMPARE(nearbyStats.gopHits, quint64{1});
    QCOMPARE(nearbyStats.frameHits, quint64{0});

    auto seekAndMeasure = [&decoder, &sink](int frame, QImage &image) -> qint64 {
        QSignalSpy frameSpy(&decoder, &MediaDecoder::frameReady);
        QElapsedTimer timer;
        timer.start();
        decoder.seekToFrame(frame, decoder.sourceFps());
        if (frameSpy.isEmpty() && !frameSpy.wait(10'000)) {
            return -1;
        }
        if (frameSpy.count() != 1 || frameSpy.takeFirst().at(0).toInt() != frame) {
            return -1;
        }
        image = sink.videoFrame().toImage();
        return timer.elapsed();
    };
    const std::array<std::pair<int, QColor>, 4> crossGopTargets = {
        std::pair{37, QColor(Qt::yellow)},
        std::pair{26, QColor(Qt::cyan)},
        std::pair{15, QColor(Qt::blue)},
        std::pair{0, QColor(Qt::red)},
    };
    for (const auto &[frame, expectedColor] : crossGopTargets) {
        QImage image;
        const qint64 elapsedMs = seekAndMeasure(frame, image);
        QVERIFY2(elapsedMs >= 0 && !image.isNull(), qPrintable(QStringLiteral("seek to frame %1 failed").arg(frame)));
        const QVector3D actualColor = averageColor(image);
        const QVector3D expectedColorVector(expectedColor.red(), expectedColor.green(), expectedColor.blue());
        QVERIFY2((actualColor - expectedColorVector).length() < 80.0F, qPrintable(QStringLiteral("unexpected frame %1 color: r=%2 g=%3 b=%4").arg(frame).arg(actualColor.x()).arg(actualColor.y()).arg(actualColor.z())));
        const VideoDecoder::CacheStats stats = decoder.cacheStats();
        QTextStream(stdout) << "video_decoder cross_gop frame=" << frame << " elapsed_ms=" << elapsedMs << " misses=" << stats.misses << " gop_hits=" << stats.gopHits << " frame_hits=" << stats.frameHits << " decoded_frames=" << stats.decodedFrames
                            << " gop_blocks=" << stats.gopBlocks << " gop_evictions=" << stats.gopEvictions << Qt::endl;
    }

    QSignalSpy hotReseekFrameSpy(&decoder, &MediaDecoder::frameReady);
    QSignalSpy hotReseekSinkSpy(&sink, &QVideoSink::videoFrameChanged);
    decoder.seekToFrame(kColdFrame, decoder.sourceFps());
    QTRY_COMPARE_WITH_TIMEOUT(hotReseekFrameSpy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(hotReseekSinkSpy.count() >= 1, 10'000);
    QCOMPARE(hotReseekFrameSpy.takeFirst().at(0).toInt(), kColdFrame);
    const QVector3D hotReseekColor = averageColor(sink.videoFrame().toImage());
    QVERIFY2(hotReseekColor.x() > hotReseekColor.z() + 100.0F, qPrintable(QStringLiteral("expected hot re-seek red frame, got r=%1 g=%2 b=%3").arg(hotReseekColor.x()).arg(hotReseekColor.y()).arg(hotReseekColor.z())));

    const VideoDecoder::CacheStats finalStats = decoder.cacheStats();
    QTextStream(stdout) << "video_decoder final_cache misses=" << finalStats.misses << " gop_hits=" << finalStats.gopHits << " frame_hits=" << finalStats.frameHits << " decoded_frames=" << finalStats.decodedFrames << " gop_blocks=" << finalStats.gopBlocks
                        << " gop_evictions=" << finalStats.gopEvictions << " frame_entries=" << finalStats.frameEntries << " frame_cost=" << finalStats.frameCost << Qt::endl;
    QCOMPARE(finalStats.misses, quint64{5});
    QCOMPARE(finalStats.gopHits, quint64{1});
    QCOMPARE(finalStats.frameHits, quint64{1});
    QCOMPARE(finalStats.decodedFrames, quint64{kTestFrameCount});
    QCOMPARE(finalStats.gopBlocks, 3);
    QCOMPARE(finalStats.gopEvictions, quint64{2});
    QCOMPARE(finalStats.frameEntries, kTestFrameCount);

    QSignalSpy burstFrameSpy(&decoder, &MediaDecoder::frameReady);
    decoder.seekToFrame(12, decoder.sourceFps());
    decoder.seekToFrame(24, decoder.sourceFps());
    decoder.seekToFrame(36, decoder.sourceFps());
    QTRY_VERIFY_WITH_TIMEOUT(std::any_of(burstFrameSpy.cbegin(), burstFrameSpy.cend(), [](const QList<QVariant> &arguments) { return arguments.first().toInt() == 36; }), 10'000);
    QTest::qWait(20);
    QCOMPARE(burstFrameSpy.last().first().toInt(), 36);
}

void TestVideoDecoder::frameCacheEvictionStaysWithinBudget() {
    SettingsManager &settings = SettingsManager::instance();
    const QVariant previousOverride = settings.value(QStringLiteral("_videoDecoderFrameCacheMaxBytes"));
    settings.setValue(QStringLiteral("_videoDecoderFrameCacheMaxBytes"), kEvictionCacheBytes);
    const auto restoreCacheOverride = qScopeGuard([&settings, previousOverride]() -> void {
        if (previousOverride.isValid()) {
            settings.setValue(QStringLiteral("_videoDecoderFrameCacheMaxBytes"), previousOverride);
        } else {
            settings.removeValue(QStringLiteral("_videoDecoderFrameCacheMaxBytes"));
        }
    });

    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString videoPath = dir.filePath(QStringLiteral("decoder-eviction.mp4"));
    QVERIFY(createTestVideo(videoPath, kEvictionFrameCount));

    constexpr int clipId = 8;
    VideoFrameStore store;
    QVideoSink sink;
    store.registerSink(QString::number(clipId), &sink);

    VideoDecoder decoder(clipId, QUrl::fromLocalFile(videoPath), &store);
    QSignalSpy readySpy(&decoder, &MediaDecoder::ready);
    decoder.scheduleStart();
    QTRY_COMPARE_WITH_TIMEOUT(readySpy.count(), 1, 10'000);
    QVERIFY(decoder.isReady());
    QCOMPARE(decoder.totalFrameCount(), kEvictionFrameCount);

    const std::array<QColor, 4> expectedColors = {QColor(Qt::red), QColor(Qt::yellow), QColor(Qt::cyan), QColor(Qt::blue)};
    auto seekAndVerify = [&decoder, &sink, &expectedColors](int frame) -> qint64 {
        QSignalSpy frameSpy(&decoder, &MediaDecoder::frameReady);
        QElapsedTimer timer;
        timer.start();
        decoder.seekToFrame(frame, decoder.sourceFps());
        if (frameSpy.isEmpty() && !frameSpy.wait(10'000)) {
            return -1;
        }
        if (frameSpy.count() != 1 || frameSpy.takeFirst().at(0).toInt() != frame) {
            return -1;
        }
        const QImage image = sink.videoFrame().toImage();
        if (image.isNull()) {
            return -1;
        }
        const QVector3D actualColor = averageColor(image);
        const QColor expectedColor = expectedColors[static_cast<std::size_t>(frame) % expectedColors.size()];
        const QVector3D expectedColorVector(static_cast<float>(expectedColor.red()), static_cast<float>(expectedColor.green()), static_cast<float>(expectedColor.blue()));
        if ((actualColor - expectedColorVector).length() >= 80.0F) {
            return -1;
        }
        return timer.elapsed();
    };

    QVector<qint64> coldSeekTimes;
    constexpr int gopCount = kEvictionFrameCount / kTestGopSize;
    coldSeekTimes.reserve(gopCount);
    for (int gop = gopCount - 1; gop >= 0; --gop) {
        const int frame = (gop * kTestGopSize) + (((gop * 5) + 1) % kTestGopSize);
        const qint64 elapsedMs = seekAndVerify(frame);
        QVERIFY2(elapsedMs >= 0, qPrintable(QStringLiteral("cold seek to frame %1 failed").arg(frame)));
        const int completedSeeks = gopCount - gop;
        QTRY_COMPARE_WITH_TIMEOUT(decoder.cacheStats().decodedFrames, static_cast<quint64>(completedSeeks * kTestGopSize), 10'000);
        QTRY_COMPARE_WITH_TIMEOUT(decoder.cacheStats().gopEvictions, static_cast<quint64>(std::max(0, completedSeeks - 3)), 10'000);
        coldSeekTimes.append(elapsedMs);
    }

    const VideoDecoder::CacheStats sweepStats = decoder.cacheStats();
    std::ranges::sort(coldSeekTimes);
    const qint64 medianColdSeekMs = coldSeekTimes.at(coldSeekTimes.size() / 2);
    QTextStream(stdout) << "video_decoder eviction_sweep seeks=" << gopCount << " median_ms=" << medianColdSeekMs << " misses=" << sweepStats.misses << " decoded_frames=" << sweepStats.decodedFrames << " gop_blocks=" << sweepStats.gopBlocks
                        << " gop_evictions=" << sweepStats.gopEvictions << " frame_entries=" << sweepStats.frameEntries << " frame_cost=" << sweepStats.frameCost << " frame_max_cost=" << sweepStats.frameMaxCost << Qt::endl;
    QCOMPARE(sweepStats.misses, quint64{gopCount});
    QCOMPARE(sweepStats.decodedFrames, quint64{kEvictionFrameCount});
    QCOMPARE(sweepStats.gopBlocks, 3);
    QCOMPARE(sweepStats.gopEvictions, quint64{gopCount - 3});
    QCOMPARE(sweepStats.frameMaxCost, kEvictionCacheBytes);
    QVERIFY(sweepStats.frameCost <= sweepStats.frameMaxCost);
    QVERIFY(sweepStats.frameEntries < kEvictionFrameCount);

    constexpr int frameCacheHitTarget = 47;
    const qint64 frameCacheHitMs = seekAndVerify(frameCacheHitTarget);
    QVERIFY(frameCacheHitMs >= 0);
    const VideoDecoder::CacheStats hitStats = decoder.cacheStats();
    QTextStream(stdout) << "video_decoder frame_cache_reseek frame=" << frameCacheHitTarget << " elapsed_ms=" << frameCacheHitMs << " misses=" << hitStats.misses << " gop_hits=" << hitStats.gopHits << " frame_hits=" << hitStats.frameHits << " decoded_frames=" << hitStats.decodedFrames << Qt::endl;
    QCOMPARE(hitStats.misses, sweepStats.misses);
    QCOMPARE(hitStats.gopHits, sweepStats.gopHits);
    QCOMPARE(hitStats.frameHits, sweepStats.frameHits + 1);
    QCOMPARE(hitStats.decodedFrames, sweepStats.decodedFrames);

    constexpr int evictedFrameTarget = kEvictionFrameCount - 1;
    const qint64 evictedReseekMs = seekAndVerify(evictedFrameTarget);
    QVERIFY(evictedReseekMs >= 0);
    QTRY_COMPARE_WITH_TIMEOUT(decoder.cacheStats().decodedFrames, hitStats.decodedFrames + kTestGopSize, 10'000);
    const VideoDecoder::CacheStats evictionStats = decoder.cacheStats();
    QTextStream(stdout) << "video_decoder evicted_reseek frame=" << evictedFrameTarget << " elapsed_ms=" << evictedReseekMs << " misses=" << evictionStats.misses << " frame_hits=" << evictionStats.frameHits << " decoded_frames=" << evictionStats.decodedFrames << " frame_entries=" << evictionStats.frameEntries
                        << " frame_cost=" << evictionStats.frameCost << Qt::endl;
    QCOMPARE(evictionStats.misses, hitStats.misses + 1);
    QCOMPARE(evictionStats.frameHits, hitStats.frameHits);
    QCOMPARE(evictionStats.decodedFrames, hitStats.decodedFrames + kTestGopSize);
    QVERIFY(evictionStats.frameCost <= evictionStats.frameMaxCost);
}

QTEST_MAIN(TestVideoDecoder)
#include "test_video_decoder.moc"
