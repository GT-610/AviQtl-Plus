#include "project_serializer.hpp"
#include "project_service.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::UI;

class TestClipByUpperObject : public QObject {
    Q_OBJECT

  private slots:
    void defaultValueIsFalse() {
        SelectionService selection;
        TimelineService timeline(&selection);

        ClipData clip;
        clip.id = 1;
        clip.sceneId = timeline.currentSceneId();
        clip.type = QStringLiteral("image");
        clip.startFrame = 0;
        clip.durationFrames = 10;
        clip.layer = 1;
        timeline.addClipDirectInternal(clip);

        QCOMPARE(timeline.clipByUpperObject(1), false);
    }

    void toggleSupportsUndoRedo() {
        SelectionService selection;
        TimelineService timeline(&selection);

        ClipData clip;
        clip.id = 1;
        clip.sceneId = timeline.currentSceneId();
        clip.type = QStringLiteral("image");
        clip.startFrame = 0;
        clip.durationFrames = 10;
        clip.layer = 1;
        timeline.addClipDirectInternal(clip);

        QSignalSpy changedSpy(&timeline, &TimelineService::clipsChanged);
        timeline.setClipByUpperObject(1, true);
        QCOMPARE(timeline.clipByUpperObject(1), true);
        QVERIFY(changedSpy.count() >= 1);

        timeline.undo();
        QCOMPARE(timeline.clipByUpperObject(1), false);

        timeline.redo();
        QCOMPARE(timeline.clipByUpperObject(1), true);
    }

    void serializerRoundTripsFlag() {
        SelectionService selection;
        TimelineService timeline(&selection);
        ProjectService project;

        ClipData clip;
        clip.id = 1;
        clip.sceneId = timeline.currentSceneId();
        clip.type = QStringLiteral("image");
        clip.startFrame = 0;
        clip.durationFrames = 10;
        clip.layer = 1;
        clip.clipByUpperObject = true;
        timeline.addClipDirectInternal(clip);

        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        const QString path = dir.filePath(QStringLiteral("project.aviqtl"));
        QString error;
        QVERIFY2(AviQtl::Core::ProjectSerializer::save(path, &timeline, &project, &error), qPrintable(error));

        SelectionService loadedSelection;
        TimelineService loadedTimeline(&loadedSelection);
        ProjectService loadedProject;
        QVERIFY2(AviQtl::Core::ProjectSerializer::load(path, &loadedTimeline, &loadedProject, &error), qPrintable(error));

        QCOMPARE(loadedTimeline.clipByUpperObject(1), true);
    }
};

QTEST_MAIN(TestClipByUpperObject)
#include "test_clip_by_upper_object.moc"
