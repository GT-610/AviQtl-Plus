#include "selection_service.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include <QTest>

using namespace AviQtl::UI;

class TestTimelineBatchMove : public QObject {
    Q_OBJECT

  private slots:
    void preservesLayoutAndUndoRedo();
    void avoidsUnselectedCollisionsAsOneGroup();
    void clampsDragDeltaAtTimelineBounds();
    void rejectsBatchMoveWhenSourceOrTargetLayerIsLocked();
    void snapsFramesUsingCurrentSceneSettings();

  private:
    static int addClip(TimelineService &timeline, const QString &type, int startFrame, int layer, int duration);
    static const ClipData &clip(const TimelineService &timeline, int id);
    static QVariantMap move(int id, int layer, int startFrame, int duration);
};

int TestTimelineBatchMove::addClip(TimelineService &timeline, const QString &type, int startFrame, int layer, int duration) {
    const int id = timeline.nextClipId();
    timeline.createClip(type, startFrame, layer);
    timeline.updateClip(id, layer, startFrame, duration);
    return id;
}

const ClipData &TestTimelineBatchMove::clip(const TimelineService &timeline, int id) {
    const auto *result = timeline.findClipById(id);
    Q_ASSERT(result != nullptr);
    return *result;
}

QVariantMap TestTimelineBatchMove::move(int id, int layer, int startFrame, int duration) {
    return {{QStringLiteral("id"), id}, {QStringLiteral("layer"), layer}, {QStringLiteral("startFrame"), startFrame}, {QStringLiteral("duration"), duration}};
}

void TestTimelineBatchMove::preservesLayoutAndUndoRedo() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int first = addClip(timeline, QStringLiteral("first"), 0, 0, 10);
    const int second = addClip(timeline, QStringLiteral("second"), 20, 2, 10);
    timeline.undoStack()->clear();

    timeline.applyClipBatchMove({move(first, 1, 10, 10), move(second, 3, 30, 10)});
    QCOMPARE(clip(timeline, first).startFrame, 10);
    QCOMPARE(clip(timeline, first).layer, 1);
    QCOMPARE(clip(timeline, second).startFrame, 30);
    QCOMPARE(clip(timeline, second).layer, 3);
    QCOMPARE(clip(timeline, second).startFrame - clip(timeline, first).startFrame, 20);
    QCOMPARE(clip(timeline, second).layer - clip(timeline, first).layer, 2);

    timeline.undo();
    QCOMPARE(clip(timeline, first).startFrame, 0);
    QCOMPARE(clip(timeline, first).layer, 0);
    QCOMPARE(clip(timeline, second).startFrame, 20);
    QCOMPARE(clip(timeline, second).layer, 2);

    timeline.redo();
    QCOMPARE(clip(timeline, first).startFrame, 10);
    QCOMPARE(clip(timeline, second).startFrame, 30);
}

void TestTimelineBatchMove::avoidsUnselectedCollisionsAsOneGroup() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int first = addClip(timeline, QStringLiteral("first"), 0, 0, 10);
    const int second = addClip(timeline, QStringLiteral("second"), 20, 1, 10);
    addClip(timeline, QStringLiteral("obstacle"), 15, 0, 10);
    timeline.undoStack()->clear();

    timeline.applyClipBatchMove({move(first, 0, 10, 10), move(second, 1, 30, 10)});

    QCOMPARE(clip(timeline, first).startFrame, 25);
    QCOMPARE(clip(timeline, second).startFrame, 45);
    QCOMPARE(clip(timeline, second).startFrame - clip(timeline, first).startFrame, 20);
}

void TestTimelineBatchMove::clampsDragDeltaAtTimelineBounds() {
    TimelineController controller;
    TimelineService *timeline = controller.timeline();
    const int first = addClip(*timeline, QStringLiteral("first"), 5, 1, 10);
    const int second = addClip(*timeline, QStringLiteral("second"), 20, 3, 10);
    timeline->applySelectionIds({first, second});

    const QPoint result = controller.resolveDragDelta(first, -20, -4, {first, second}, 5, 1, 3, 128);
    QCOMPARE(result.x(), -5);
    QCOMPARE(result.y(), -1);
}

void TestTimelineBatchMove::rejectsBatchMoveWhenSourceOrTargetLayerIsLocked() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int first = addClip(timeline, QStringLiteral("first"), 0, 0, 10);
    const int second = addClip(timeline, QStringLiteral("second"), 20, 1, 10);
    timeline.undoStack()->clear();

    timeline.setLayerState(2, true, 0);
    timeline.undoStack()->clear();
    timeline.applyClipBatchMove({move(first, 1, 10, 10), move(second, 2, 30, 10)});
    QCOMPARE(clip(timeline, first).startFrame, 0);
    QCOMPARE(clip(timeline, second).startFrame, 20);
    QVERIFY(timeline.undoStack()->isClean());

    timeline.setLayerState(2, false, 0);
    timeline.setLayerState(0, true, 0);
    timeline.undoStack()->clear();
    timeline.applyClipBatchMove({move(first, 1, 10, 10), move(second, 2, 30, 10)});
    QCOMPARE(clip(timeline, first).layer, 0);
    QCOMPARE(clip(timeline, second).layer, 1);
}

void TestTimelineBatchMove::snapsFramesUsingCurrentSceneSettings() {
    TimelineController controller;
    const int sceneId = controller.currentSceneId();
    controller.updateSceneSettings(sceneId, QStringLiteral("Scene"), 1280, 720, 30.0, 300, QStringLiteral("Frame"), 120.0, 0.0, 10, 4, true, 10);
    QCOMPARE(controller.snapFrame(16.0), 20);
    QCOMPARE(controller.snapFrame(16.0, true), 16);

    controller.updateSceneSettings(sceneId, QStringLiteral("Scene"), 1280, 720, 30.0, 300, QStringLiteral("BPM"), 120.0, 0.5, 10, 4, true, 10);
    QCOMPARE(controller.snapFrame(22.0), 15);

    controller.setTimelineScale(0.4);
    controller.updateSceneSettings(sceneId, QStringLiteral("Scene"), 1280, 720, 30.0, 300, QStringLiteral("Auto"), 120.0, 0.0, 10, 4, true, 10);
    QCOMPARE(controller.snapFrame(44.0), 30);

    controller.updateSceneSettings(sceneId, QStringLiteral("Scene"), 1280, 720, 30.0, 300, QStringLiteral("Frame"), 120.0, 0.0, 10, 4, false, 10);
    QCOMPARE(controller.snapFrame(16.0), 16);
}

QTEST_MAIN(TestTimelineBatchMove)
#include "test_timeline_batch_move.moc"
