#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include <QElapsedTimer>
#include <QTest>
#include <QTextStream>

using namespace AviQtl::UI;

namespace {
constexpr int kClipCount = 5'000;
constexpr int kLayerCount = 50;
constexpr int kClipDuration = 10;
constexpr int kClipSpacing = 20;

void populateLargeTimeline(TimelineController &controller) {
    QList<ClipData> clips;
    clips.reserve(kClipCount);
    for (int i = 0; i < kClipCount; ++i) {
        ClipData clip;
        clip.id = i + 1;
        clip.sceneId = controller.currentSceneId();
        clip.type = QStringLiteral("performance_clip");
        clip.layer = i % kLayerCount;
        clip.startFrame = (i / kLayerCount) * kClipSpacing;
        clip.durationFrames = kClipDuration;
        clips.append(std::move(clip));
    }

    controller.timeline()->addClipsDirectInternal(clips);
    controller.timeline()->setNextClipId(kClipCount + 1);
    QCoreApplication::processEvents();
}

QVariantMap moveFor(const ClipData &clip, int frameDelta) { return {{QStringLiteral("id"), clip.id}, {QStringLiteral("layer"), clip.layer}, {QStringLiteral("startFrame"), clip.startFrame + frameDelta}, {QStringLiteral("duration"), clip.durationFrames}}; }
} // namespace

class TestLargeTimelinePerformance : public QObject {
    Q_OBJECT

  private slots:
    void materializesQmlClipSnapshot();
    void findsClipsAcrossLargeTimeline();
    void movesLargeSelectionWithUndoRedo();
};

void TestLargeTimelinePerformance::materializesQmlClipSnapshot() {
    TimelineController controller;
    populateLargeTimeline(controller);

    QElapsedTimer timer;
    timer.start();
    const QVariantList snapshot = controller.clips();
    const qint64 elapsedMs = timer.elapsed();

    QCOMPARE(snapshot.size(), kClipCount);
    QCOMPARE(snapshot.first().toMap().value(QStringLiteral("id")).toInt(), 1);
    QCOMPARE(snapshot.last().toMap().value(QStringLiteral("id")).toInt(), kClipCount);
    QTextStream(stdout) << "large_timeline qml_snapshot clips=" << kClipCount << " elapsed_ms=" << elapsedMs << Qt::endl;
}

void TestLargeTimelinePerformance::findsClipsAcrossLargeTimeline() {
    TimelineController controller;
    populateLargeTimeline(controller);

    QElapsedTimer timer;
    timer.start();
    int checksum = 0;
    constexpr int lookupCount = 10'000;
    for (int i = 0; i < lookupCount; ++i) {
        const int id = ((i * 7919) % kClipCount) + 1;
        const ClipData *clip = controller.timeline()->findClipById(id);
        QVERIFY(clip != nullptr);
        checksum += clip->id;
    }
    const qint64 elapsedMs = timer.elapsed();

    QVERIFY(checksum > 0);
    QTextStream(stdout) << "large_timeline clip_lookup clips=" << kClipCount << " lookups=" << lookupCount << " elapsed_ms=" << elapsedMs << Qt::endl;
}

void TestLargeTimelinePerformance::movesLargeSelectionWithUndoRedo() {
    TimelineController controller;
    populateLargeTimeline(controller);

    for (const int batchSize : {1, 10, 50, 100, 500, 1'000}) {
        QVariantList selectedIds;
        QVariantList moves;
        selectedIds.reserve(batchSize);
        moves.reserve(batchSize);
        for (int id = 1; id <= batchSize; ++id) {
            const ClipData *clip = controller.timeline()->findClipById(id);
            QVERIFY(clip != nullptr);
            selectedIds.append(id);
            moves.append(moveFor(*clip, 5));
        }
        controller.timeline()->applySelectionIds(selectedIds);
        controller.timeline()->undoStack()->clear();

        QElapsedTimer timer;
        timer.start();
        controller.applyClipBatchMove(moves);
        const qint64 moveMs = timer.elapsed();

        QCOMPARE(controller.timeline()->findClipById(1)->startFrame, 5);
        const int expectedLastStart = ((batchSize - 1) / kLayerCount) * kClipSpacing + 5;
        QCOMPARE(controller.timeline()->findClipById(batchSize)->startFrame, expectedLastStart);

        timer.restart();
        controller.timeline()->undo();
        const qint64 undoMs = timer.elapsed();
        QCOMPARE(controller.timeline()->findClipById(1)->startFrame, 0);

        timer.restart();
        controller.timeline()->redo();
        const qint64 redoMs = timer.elapsed();
        QCOMPARE(controller.timeline()->findClipById(1)->startFrame, 5);

        QTextStream(stdout) << "large_timeline batch_move clips=" << kClipCount << " selected=" << batchSize << " move_ms=" << moveMs << " undo_ms=" << undoMs << " redo_ms=" << redoMs << Qt::endl;

        controller.timeline()->undo();
        QCOMPARE(controller.timeline()->findClipById(1)->startFrame, 0);
        controller.timeline()->undoStack()->clear();
    }
}

QTEST_MAIN(TestLargeTimelinePerformance)
#include "test_large_timeline_performance.moc"
