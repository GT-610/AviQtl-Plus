#include "project_serializer.hpp"
#include "effect_registry.hpp"
#include "project_service.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTemporaryDir>
#include <QTest>
#include <algorithm>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestProjectSerializer : public QObject {
    Q_OBJECT

  private slots:
    void atomicSaveReplacesAnExistingProject();
    void saveFailureLeavesAnInvalidTargetUntouched();
    void sceneStateAndMediaPathsRoundTrip();
    void invalidJsonDoesNotReplaceProjectState();
};

void TestProjectSerializer::atomicSaveReplacesAnExistingProject() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString path = dir.filePath(QStringLiteral("project.aviqtl"));
    QFile original(path);
    QVERIFY(original.open(QIODevice::WriteOnly));
    QCOMPARE(original.write("not a project"), qint64(13));
    original.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY2(ProjectSerializer::save(path, &timeline, &project, &error), qPrintable(error));

    QFile saved(path);
    QVERIFY(saved.open(QIODevice::ReadOnly));
    const QJsonDocument document = QJsonDocument::fromJson(saved.readAll());
    QVERIFY(document.isObject());
    QCOMPARE(document.object().value(QStringLiteral("version")).toInt(), 3);
}

void TestProjectSerializer::sceneStateAndMediaPathsRoundTrip() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    EffectMetadata imageMeta;
    imageMeta.id = QStringLiteral("image");
    imageMeta.name = QStringLiteral("Image");
    imageMeta.kind = QStringLiteral("object");
    imageMeta.qmlSource = QStringLiteral("ImageObject.qml");
    imageMeta.defaultParams = {{QStringLiteral("path"), QString()}};
    EffectRegistry::instance().registerEffect(imageMeta);

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    timeline.updateSceneSettings(0, QStringLiteral("Portable Scene"), 1280, 720, 24.0, 480, QStringLiteral("BPM"), 135.0, 0.25, 12, 3, false, 18);
    timeline.setLayerState(4, true, 0);
    timeline.setLayerState(7, true, 1);

    QList<SceneData> scenes = timeline.getAllScenes();
    QCOMPARE(scenes.size(), 1);
    scenes[0].durationFrames = 360;
    timeline.setScenes(scenes);

    const QString mediaDir = dir.filePath(QStringLiteral("media"));
    QVERIFY(QDir().mkpath(mediaDir));
    const QString mediaPath = QDir(mediaDir).filePath(QStringLiteral("still.png"));
    QFile media(mediaPath);
    QVERIFY(media.open(QIODevice::WriteOnly));
    QCOMPARE(media.write("image"), qint64(5));
    media.close();

    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("image"), 0, 0);
    ClipData *clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    auto imageEffect = std::find_if(clip->effects.begin(), clip->effects.end(), [](const EffectModel *effect) { return effect != nullptr && effect->id() == QLatin1String("image"); });
    QVERIFY(imageEffect != clip->effects.end());
    (*imageEffect)->setParam(QStringLiteral("path"), mediaPath);

    const QString projectPath = dir.filePath(QStringLiteral("portable.aviqtl"));
    QString error;
    QVERIFY2(ProjectSerializer::save(projectPath, &timeline, &project, &error), qPrintable(error));

    QFile saved(projectPath);
    QVERIFY(saved.open(QIODevice::ReadOnly));
    const QJsonObject root = QJsonDocument::fromJson(saved.readAll()).object();
    const QJsonObject savedScene = root.value(QStringLiteral("scenes")).toArray().first().toObject();
    QCOMPARE(savedScene.value(QStringLiteral("nestedDuration")).toInt(), 360);
    QCOMPARE(savedScene.value(QStringLiteral("lockedLayers")).toArray().first().toInt(), 4);
    QCOMPARE(savedScene.value(QStringLiteral("hiddenLayers")).toArray().first().toInt(), 7);
    const QJsonArray effects = root.value(QStringLiteral("clips")).toArray().first().toObject().value(QStringLiteral("effects")).toArray();
    const auto savedImageEffect = std::find_if(effects.begin(), effects.end(), [](const QJsonValue &value) { return value.toObject().value(QStringLiteral("id")).toString() == QLatin1String("image"); });
    QVERIFY(savedImageEffect != effects.end());
    QCOMPARE(savedImageEffect->toObject().value(QStringLiteral("params")).toObject().value(QStringLiteral("path")).toString(), QStringLiteral("media/still.png"));

    SelectionService loadedSelection;
    TimelineService loadedTimeline(&loadedSelection);
    ProjectService loadedProject;
    QVERIFY2(ProjectSerializer::load(projectPath, &loadedTimeline, &loadedProject, &error), qPrintable(error));
    const SceneData &loadedScene = loadedTimeline.getAllScenes().first();
    QCOMPARE(loadedScene.durationFrames, 360);
    QVERIFY(loadedScene.lockedLayers.contains(4));
    QVERIFY(loadedScene.hiddenLayers.contains(7));
    QCOMPARE(loadedScene.gridMode, QStringLiteral("BPM"));
    QCOMPARE(loadedScene.gridBpm, 135.0);
    QCOMPARE(loadedScene.gridOffset, 0.25);
    QCOMPARE(loadedScene.gridInterval, 12);
    QCOMPARE(loadedScene.gridSubdivision, 3);
    QCOMPARE(loadedScene.enableSnap, false);
    QCOMPARE(loadedScene.magneticSnapRange, 18);
    const ClipData *loadedClip = loadedTimeline.findClipById(clipId);
    QVERIFY(loadedClip != nullptr);
    const auto loadedImageEffect = std::find_if(loadedClip->effects.begin(), loadedClip->effects.end(), [](const EffectModel *effect) { return effect != nullptr && effect->id() == QLatin1String("image"); });
    QVERIFY(loadedImageEffect != loadedClip->effects.end());
    QCOMPARE((*loadedImageEffect)->params().value(QStringLiteral("path")).toString(), QDir::cleanPath(mediaPath));
}

void TestProjectSerializer::invalidJsonDoesNotReplaceProjectState() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = dir.filePath(QStringLiteral("broken.aviqtl"));
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    QCOMPARE(file.write("{broken"), qint64(7));
    file.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    project.setWidth(1234);
    QString error;
    QVERIFY(!ProjectSerializer::load(path, &timeline, &project, &error));
    QVERIFY(!error.isEmpty());
    QCOMPARE(project.width(), 1234);
    QCOMPARE(timeline.getAllScenes().size(), 1);
}

void TestProjectSerializer::saveFailureLeavesAnInvalidTargetUntouched() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString directoryTarget = dir.filePath(QStringLiteral("not-a-project-file"));
    QVERIFY(QDir().mkpath(directoryTarget));

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY(!ProjectSerializer::save(directoryTarget, &timeline, &project, &error));
    QVERIFY(!error.isEmpty());
    QVERIFY(QFileInfo(directoryTarget).isDir());
}

QTEST_MAIN(TestProjectSerializer)
#include "test_project_serializer.moc"
