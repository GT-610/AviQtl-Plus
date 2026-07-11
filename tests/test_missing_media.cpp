#include "effect_registry.hpp"
#include "timeline_controller.hpp"
#include <QColor>
#include <QFile>
#include <QImage>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestMissingMedia : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void loadDetectsMissingMediaByType();
    void relinkIsTypeSafeAndUndoable();

  private:
    static int effectIndex(const ClipData &clip, const QString &effectId);
    static void registerMediaEffect(const QString &id, const QString &parameter);
};

void TestMissingMedia::registerMediaEffect(const QString &id, const QString &parameter) {
    EffectMetadata metadata;
    metadata.id = id;
    metadata.name = id;
    metadata.version = QStringLiteral("1.0.0");
    metadata.kind = QStringLiteral("object");
    metadata.categories = {QStringLiteral("Media")};
    metadata.defaultParams = {{parameter, QString()}};
    EffectRegistry::instance().registerEffect(metadata);
}

void TestMissingMedia::initTestCase() {
    registerMediaEffect(QStringLiteral("image"), QStringLiteral("path"));
    registerMediaEffect(QStringLiteral("video"), QStringLiteral("path"));
    registerMediaEffect(QStringLiteral("audio"), QStringLiteral("source"));
}

int TestMissingMedia::effectIndex(const ClipData &clip, const QString &effectId) {
    for (int index = 0; index < clip.effects.size(); ++index) {
        if (clip.effects.at(index) != nullptr && clip.effects.at(index)->id() == effectId)
            return index;
    }
    return -1;
}

void TestMissingMedia::loadDetectsMissingMediaByType() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    const QString imagePath = directory.filePath(QStringLiteral("image.png"));
    QImage image(8, 8, QImage::Format_ARGB32);
    image.fill(QColor(Qt::red));
    QVERIFY(image.save(imagePath));
    const QString videoPath = directory.filePath(QStringLiteral("video.mp4"));
    const QString audioPath = directory.filePath(QStringLiteral("audio.wav"));
    QVERIFY(QFile(videoPath).open(QIODevice::WriteOnly));
    QVERIFY(QFile(audioPath).open(QIODevice::WriteOnly));

    TimelineController controller;
    const QList<QPair<QString, QString>> media = {
        {QStringLiteral("image"), imagePath},
        {QStringLiteral("video"), videoPath},
        {QStringLiteral("audio"), audioPath},
    };
    int imageClipId = -1;
    for (int layer = 0; layer < media.size(); ++layer) {
        const int clipId = controller.timeline()->nextClipId();
        controller.createObject(media.at(layer).first, 0, layer);
        const ClipData *clip = controller.timeline()->findClipById(clipId);
        QVERIFY(clip != nullptr);
        const int index = effectIndex(*clip, media.at(layer).first);
        QVERIFY(index >= 0);
        controller.updateClipEffectParam(clipId, index,
                                         media.at(layer).first == QStringLiteral("audio") ? QStringLiteral("source") : QStringLiteral("path"),
                                         media.at(layer).second);
        if (media.at(layer).first == QStringLiteral("image"))
            imageClipId = clipId;
    }

    const QString projectPath = directory.filePath(QStringLiteral("missing-media.aviqtl"));
    QVERIFY(controller.saveProject(projectPath));
    QVERIFY(QFile::remove(imagePath));
    QVERIFY(QFile::remove(videoPath));
    QVERIFY(QFile::remove(audioPath));

    TimelineController loaded;
    QVERIFY(loaded.loadProject(projectPath));
    const QVariantList missing = loaded.missingMedia();
    QCOMPARE(missing.size(), 3);
    QCOMPARE(missing.at(0).toMap().value(QStringLiteral("type")).toString(), QStringLiteral("image"));
    QCOMPARE(missing.at(1).toMap().value(QStringLiteral("type")).toString(), QStringLiteral("video"));
    QCOMPARE(missing.at(2).toMap().value(QStringLiteral("type")).toString(), QStringLiteral("audio"));

    QVERIFY(imageClipId >= 0);
    const ClipData *imageClip = loaded.timeline()->findClipById(imageClipId);
    QVERIFY(imageClip != nullptr);
    const int imageEffect = effectIndex(*imageClip, QStringLiteral("image"));
    QVERIFY(imageEffect >= 0);
    loaded.updateClipEffectParam(imageClipId, imageEffect, QStringLiteral("path"), QString());
    QTRY_COMPARE(loaded.missingMedia().size(), 2);
}

void TestMissingMedia::relinkIsTypeSafeAndUndoable() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());

    const QString oldPath = directory.filePath(QStringLiteral("old.png"));
    const QString replacementPath = directory.filePath(QStringLiteral("replacement.png"));
    const QString wrongTypePath = directory.filePath(QStringLiteral("wrong.txt"));
    QImage image(8, 8, QImage::Format_ARGB32);
    image.fill(QColor(Qt::blue));
    QVERIFY(image.save(oldPath));
    QVERIFY(image.save(replacementPath));
    QFile wrongType(wrongTypePath);
    QVERIFY(wrongType.open(QIODevice::WriteOnly));

    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    QVERIFY(controller.importMediaFile(QUrl::fromLocalFile(oldPath).toString(), 0, 0).value(QStringLiteral("ok")).toBool());
    const QString projectPath = directory.filePath(QStringLiteral("relink.aviqtl"));
    QVERIFY(controller.saveProject(projectPath));
    QVERIFY(QFile::remove(oldPath));

    TimelineController loaded;
    QVERIFY(loaded.loadProject(projectPath));
    QCOMPARE(loaded.missingMedia().size(), 1);
    QVERIFY(!loaded.relinkMedia(clipId, QUrl::fromLocalFile(wrongTypePath).toString()));
    QVERIFY(!loaded.relinkMedia(clipId, QUrl::fromLocalFile(directory.filePath(QStringLiteral("absent.png"))).toString()));
    QCOMPARE(loaded.missingMedia().size(), 1);

    QVERIFY(loaded.relinkMedia(clipId, QUrl::fromLocalFile(replacementPath).toString()));
    QCOMPARE(loaded.missingMedia().size(), 0);
    const int index = effectIndex(*loaded.timeline()->findClipById(clipId), QStringLiteral("image"));
    QCOMPARE(loaded.timeline()->findClipById(clipId)->effects.at(index)->params().value(QStringLiteral("path")).toString(), replacementPath);

    loaded.timeline()->undoStack()->undo();
    QTRY_COMPARE(loaded.missingMedia().size(), 1);
    loaded.timeline()->undoStack()->redo();
    QTRY_COMPARE(loaded.missingMedia().size(), 0);
}

QTEST_MAIN(TestMissingMedia)
#include "test_missing_media.moc"
