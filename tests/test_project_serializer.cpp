#include "effect_registry.hpp"
#include "project_serializer.hpp"
#include "project_service.hpp"
#include "rust_project_document.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryDir>
#include <QTest>
#include <algorithm>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestProjectSerializer : public QObject {
    Q_OBJECT

  private slots:
    void atomicSaveReplacesAnExistingProject();
    void saveSnapshotIsCanonicalizedByRust();
    void saveFailureLeavesAnInvalidTargetUntouched();
    void sceneStateAndMediaPathsRoundTrip();
    void invalidGridSettingsUseDefaults();
    void invalidJsonDoesNotReplaceProjectState();
    void rejectedTimelineStateDoesNotReplaceProjectSettings();
    void missingEffectsDoNotShiftRuntimeMetadata();
    void setScenesRestoresRuntimeStateWhenProjectionIsRejected();
    void legacyProjectValuesAreNormalizedByRust();
    void unsupportedVersionDoesNotReplaceProjectState();
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

void TestProjectSerializer::saveSnapshotIsCanonicalizedByRust() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QVariantMap snapshot{
        {QStringLiteral("version"), AviQtl::RustCore::currentProjectVersion()},
        {QStringLiteral("settings"), QVariantMap{{QStringLiteral("width"), 0}, {QStringLiteral("height"), 50000}, {QStringLiteral("fps"), 0}, {QStringLiteral("sampleRate"), 500000}}},
        {QStringLiteral("scenes"), QVariantList{QVariantMap{{QStringLiteral("id"), 4}, {QStringLiteral("duration"), -1}}}},
        {QStringLiteral("clips"), QVariantList{QVariantMap{{QStringLiteral("id"), 8}, {QStringLiteral("sceneId"), 4}, {QStringLiteral("type"), QStringLiteral("camera")}, {QStringLiteral("layer"), 500}}}},
    };

    const QString path = dir.filePath(QStringLiteral("canonical.aviqtl"));
    QString error;
    QVERIFY2(ProjectSerializer::saveSnapshot(path, snapshot, &error), qPrintable(error));

    QFile saved(path);
    QVERIFY(saved.open(QIODevice::ReadOnly));
    const QJsonObject root = QJsonDocument::fromJson(saved.readAll()).object();
    QCOMPARE(root.value(QStringLiteral("version")).toInt(), AviQtl::RustCore::currentProjectVersion());
    const QJsonObject settings = root.value(QStringLiteral("settings")).toObject();
    QCOMPARE(settings.value(QStringLiteral("width")).toInt(), AviQtl::kDefaultWidth);
    QCOMPARE(settings.value(QStringLiteral("height")).toInt(), AviQtl::kDefaultHeight);
    QCOMPARE(settings.value(QStringLiteral("fps")).toDouble(), AviQtl::kDefaultFps);
    QCOMPARE(settings.value(QStringLiteral("sampleRate")).toInt(), AviQtl::kDefaultSampleRate);
    const QJsonObject scene = root.value(QStringLiteral("scenes")).toArray().first().toObject();
    QCOMPARE(scene.value(QStringLiteral("duration")).toInt(), AviQtl::kDefaultTotalFrames);
    const QJsonObject clip = root.value(QStringLiteral("clips")).toArray().first().toObject();
    QCOMPARE(clip.value(QStringLiteral("type")).toString(), QStringLiteral("camera_control"));
    QCOMPARE(clip.value(QStringLiteral("layer")).toInt(), 127);
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
    QVERIFY(timeline.setScenes(scenes));

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

void TestProjectSerializer::rejectedTimelineStateDoesNotReplaceProjectSettings() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = dir.filePath(QStringLiteral("invalid-timeline.aviqtl"));
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    const QByteArray document = R"({
        "version": 3,
        "settings": {"width": 640, "height": 360, "fps": 24, "sampleRate": 44100},
        "scenes": [{"id": 0, "name": "Root", "duration": 300}],
        "clips": [{
            "id": 1, "sceneId": 0, "type": "text", "start": -1,
            "duration": 10, "layer": 0, "params": {}, "effects": [], "audioPlugins": []
        }]
    })";
    QCOMPARE(file.write(document), document.size());
    file.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    project.setWidth(1234);
    project.setHeight(567);
    project.setFps(48.0);
    project.setSampleRate(96000);
    const QVariantMap previousTimeline = timeline.timelineStateSnapshot();

    QString error;
    QVERIFY(!ProjectSerializer::load(path, &timeline, &project, &error));
    QVERIFY(!error.isEmpty());
    QCOMPARE(project.width(), 1234);
    QCOMPARE(project.height(), 567);
    QCOMPARE(project.fps(), 48.0);
    QCOMPARE(project.sampleRate(), 96000);
    QCOMPARE(timeline.timelineStateSnapshot(), previousTimeline);
}

void TestProjectSerializer::missingEffectsDoNotShiftRuntimeMetadata() {
    EffectMetadata known;
    known.id = QStringLiteral("review.known");
    known.name = QStringLiteral("Known");
    known.kind = QStringLiteral("effect");
    known.defaultParams = {{QStringLiteral("value"), 0.0}};
    EffectRegistry::instance().registerEffect(known);

    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = dir.filePath(QStringLiteral("missing-effect.aviqtl"));
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    const QByteArray document = R"({
        "version": 3,
        "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
        "scenes": [{"id": 0, "name": "Root", "duration": 300}],
        "clips": [{
            "id": 1, "sceneId": 0, "type": "text", "start": 0, "duration": 30,
            "layer": 0, "params": {}, "audioPlugins": [],
            "effects": [
                {"id": "review.missing", "params": {}, "reviewToken": "missing"},
                {"id": "review.known", "params": {"value": 2.0}, "reviewToken": "known"}
            ]
        }]
    })";
    QCOMPARE(file.write(document), document.size());
    file.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY2(ProjectSerializer::load(path, &timeline, &project, &error), qPrintable(error));
    const QVariantList effects = timeline.timelineStateSnapshot()
                                     .value(QStringLiteral("clips"))
                                     .toList()
                                     .first()
                                     .toMap()
                                     .value(QStringLiteral("effects"))
                                     .toList();
    QCOMPARE(effects.size(), 1);
    QCOMPARE(effects.first().toMap().value(QStringLiteral("id")).toString(), known.id);
    QCOMPARE(effects.first().toMap().value(QStringLiteral("reviewToken")).toString(),
             QStringLiteral("known"));
}

void TestProjectSerializer::setScenesRestoresRuntimeStateWhenProjectionIsRejected() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("audio"), 0, 0);
    QVERIFY(timeline.findClipById(clipId) != nullptr);
    const QVariantMap previousState = timeline.timelineStateSnapshot();
    const QList<SceneData> previousScenes = timeline.getAllScenes();
    QList<SceneData> rejectedScenes = previousScenes;
    rejectedScenes[0].clips.append(rejectedScenes[0].clips.first());

    QVERIFY(!timeline.setScenes(rejectedScenes));
    QCOMPARE(timeline.getAllScenes().size(), previousScenes.size());
    QCOMPARE(timeline.getAllScenes().first().clips.size(), previousScenes.first().clips.size());
    QCOMPARE(timeline.timelineStateSnapshot(), previousState);
}

void TestProjectSerializer::legacyProjectValuesAreNormalizedByRust() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = dir.filePath(QStringLiteral("legacy.aviqtl"));
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    const QByteArray document = R"({
        "version": 1,
        "settings": {"width": 0, "height": 50000, "fps": 0, "sampleRate": 500000},
        "scenes": [{
            "id": 7, "name": "Legacy", "duration": -1,
            "lockedLayers": [2], "gridBpm": 200
        }],
        "clips": [{
            "id": 9, "sceneId": 7, "type": "camera", "start": 3,
            "duration": 12, "layer": 500
        }]
    })";
    QCOMPARE(file.write(document), document.size());
    file.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY2(ProjectSerializer::load(path, &timeline, &project, &error), qPrintable(error));

    QCOMPARE(project.width(), AviQtl::kDefaultWidth);
    QCOMPARE(project.height(), AviQtl::kDefaultHeight);
    QCOMPARE(project.fps(), AviQtl::kDefaultFps);
    QCOMPARE(project.sampleRate(), AviQtl::kDefaultSampleRate);
    QCOMPARE(timeline.getAllScenes().size(), 1);
    const SceneData &scene = timeline.getAllScenes().first();
    QCOMPARE(scene.id, 7);
    QCOMPARE(scene.totalFrames, AviQtl::kDefaultTotalFrames);
    QVERIFY(scene.lockedLayers.isEmpty());
    QCOMPARE(scene.gridBpm, 120.0);
    QVERIFY(scene.enableSnap);
    QCOMPARE(scene.clips.size(), 1);
    QCOMPARE(scene.clips.first().type, QStringLiteral("camera_control"));
    QCOMPARE(scene.clips.first().layer, 127);
}

void TestProjectSerializer::unsupportedVersionDoesNotReplaceProjectState() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = dir.filePath(QStringLiteral("future.aviqtl"));
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    QCOMPARE(file.write(R"({"version":4})"), qint64{13});
    file.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    project.setWidth(1234);
    QString error;
    QVERIFY(!ProjectSerializer::load(path, &timeline, &project, &error));
    QVERIFY(error.contains(QStringLiteral("Unsupported")));
    QCOMPARE(project.width(), 1234);
    QCOMPARE(timeline.getAllScenes().size(), 1);
}

void TestProjectSerializer::invalidGridSettingsUseDefaults() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = dir.filePath(QStringLiteral("invalid-grid.aviqtl"));
    QFile file(path);
    QVERIFY(file.open(QIODevice::WriteOnly));
    const QByteArray document = R"({
        "version": 3,
        "settings": {"width": 1920, "height": 1080, "fps": 60, "sampleRate": 48000},
        "scenes": [{
            "id": 0, "name": "Grid", "width": 1920, "height": 1080, "fps": 60, "duration": 300,
            "gridMode": "BPM", "gridBpm": 1000000, "gridOffset": -1,
            "gridInterval": 0, "gridSubdivision": 1000000, "magneticSnapRange": 1000000
        }],
        "clips": []
    })";
    QCOMPARE(file.write(document), document.size());
    file.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY2(ProjectSerializer::load(path, &timeline, &project, &error), qPrintable(error));
    const SceneData &scene = timeline.getAllScenes().first();
    QCOMPARE(scene.gridMode, QStringLiteral("BPM"));
    QCOMPARE(scene.gridBpm, 120.0);
    QCOMPARE(scene.gridOffset, 0.0);
    QCOMPARE(scene.gridInterval, 10);
    QCOMPARE(scene.gridSubdivision, 4);
    QCOMPARE(scene.magneticSnapRange, 10);
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
