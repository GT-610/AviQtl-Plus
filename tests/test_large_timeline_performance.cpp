#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "window_manager.hpp"
#include "workspace.hpp"
#include <QElapsedTimer>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSet>
#include <QTest>
#include <QTextStream>
#include <memory>

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
    void filtersViewportAndRetainsActiveClips();
    void virtualizesTimelineViewDelegates();
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

void TestLargeTimelinePerformance::filtersViewportAndRetainsActiveClips() {
    TimelineController controller;
    populateLargeTimeline(controller);

    const QVariantList viewport = controller.clipsForViewport(0, 200, 0, 9, {kClipCount});
    QCOMPARE(viewport.size(), 101);

    QSet<int> ids;
    for (const QVariant &value : viewport) {
        ids.insert(value.toMap().value(QStringLiteral("id")).toInt());
    }
    QVERIFY(ids.contains(1));
    QVERIFY(ids.contains(kClipCount));

    const QVariantList retainedVisible = controller.clipsForViewport(0, 200, 0, 9, {1});
    QCOMPARE(retainedVisible.size(), 100);
}

void TestLargeTimelinePerformance::virtualizesTimelineViewDelegates() {
    Workspace workspace;
    workspace.newProject();
    TimelineController *controller = workspace.currentTimeline();
    QVERIFY(controller != nullptr);
    populateLargeTimeline(*controller);
    controller->setTimelineScale(1.0);

    QQmlEngine engine;
    QQmlContext *context = engine.rootContext();
    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &AviQtl::Core::SettingsManager::instance());
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(&WindowManager::instance()));

    QElapsedTimer timer;
    timer.start();
    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/timeline/TimelineView.qml")));
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    std::unique_ptr<QObject> object(component.create(context));
    QVERIFY2(object != nullptr, qPrintable(component.errorString()));
    auto *timelineView = qobject_cast<QQuickItem *>(object.get());
    QVERIFY(timelineView != nullptr);
    timelineView->setProperty("layerCount", kLayerCount);
    timelineView->setSize(QSizeF(1'000, 300));

    QQuickWindow window;
    window.setGeometry(0, 0, 1'000, 300);
    timelineView->setParentItem(window.contentItem());
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 5'000);
    QTRY_VERIFY_WITH_TIMEOUT(timelineView->property("renderedClipCount").toInt() > 0, 5'000);

    const int initialDelegateCount = timelineView->property("renderedClipCount").toInt();
    QVERIFY(initialDelegateCount < 800);
    QVERIFY(!timelineView->property("renderedClipIds").toList().contains(kClipCount));
    const qint64 creationMs = timer.elapsed();
    QTextStream(stdout) << "large_timeline qml_delegates total_clips=" << kClipCount << " rendered=" << initialDelegateCount << " elapsed_ms=" << creationMs << Qt::endl;

    const int initialLoadedLastFrame = timelineView->property("loadedLastFrame").toInt();
    const int initialModelRevision = timelineView->property("viewportModelRevision").toInt();
    timelineView->setProperty("contentX", 80.0);
    QTest::qWait(20);
    QCOMPARE(timelineView->property("loadedLastFrame").toInt(), initialLoadedLastFrame);
    QCOMPARE(timelineView->property("viewportModelRevision").toInt(), initialModelRevision);
    QCOMPARE(timelineView->property("renderedClipCount").toInt(), initialDelegateCount);

    controller->timeline()->applySelectionIds({kClipCount});
    QTRY_VERIFY_WITH_TIMEOUT(timelineView->property("renderedClipIds").toList().contains(kClipCount), 5'000);
    const int retainedDelegateCount = timelineView->property("renderedClipCount").toInt();
    const int retainedModelRevision = timelineView->property("viewportModelRevision").toInt();
    timelineView->setProperty("isDraggingMulti", true);
    QTRY_VERIFY_WITH_TIMEOUT(timelineView->property("renderedClipIds").toList().contains(kClipCount), 5'000);
    QCOMPARE(timelineView->property("renderedClipCount").toInt(), retainedDelegateCount);
    QCOMPARE(timelineView->property("viewportModelRevision").toInt(), retainedModelRevision);

    timelineView->setProperty("isDraggingMulti", false);
    timelineView->setProperty("contentX", 1'500.0);
    timelineView->setProperty("contentY", 1'200.0);
    QTRY_VERIFY_WITH_TIMEOUT(timelineView->property("loadedFirstLayer").toInt() > 0, 5'000);
    QVERIFY(timelineView->property("viewportModelRevision").toInt() > retainedModelRevision);
    QTRY_VERIFY_WITH_TIMEOUT(timelineView->property("renderedClipIds").toList().contains(kClipCount), 5'000);
    QVERIFY(!timelineView->property("renderedClipIds").toList().contains(1));
    QVERIFY(timelineView->property("renderedClipCount").toInt() < 800);
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
