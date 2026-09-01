#include "commands.hpp"
#include "effect_registry.hpp"
#include "image_decoder.hpp"
#include "selection_service.hpp"
#include "timeline_controller.hpp"
#include "video_frame_store.hpp"
#include "video_encoder.hpp"
#include <QColor>
#include <QCoreApplication>
#include <QDataStream>
#include <QEvent>
#include <QFile>
#include <QFileInfo>
#include <QImage>
#include <QElapsedTimer>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QSignalBlocker>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <algorithm>
#include <functional>
#include <memory>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

namespace {

bool writeSilentWav(const QString &path) {
    constexpr quint32 sampleRate = 8'000;
    constexpr quint16 channelCount = 1;
    constexpr quint16 bitsPerSample = 16;
    constexpr quint32 durationSeconds = 2;
    constexpr quint16 blockAlign = channelCount * (bitsPerSample / 8);
    constexpr quint32 byteRate = sampleRate * blockAlign;
    constexpr quint32 dataSize = sampleRate * durationSeconds * blockAlign;

    QFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        return false;
    }
    QDataStream stream(&file);
    stream.setByteOrder(QDataStream::LittleEndian);
    stream.writeRawData("RIFF", 4);
    stream << quint32(36U + dataSize);
    stream.writeRawData("WAVE", 4);
    stream.writeRawData("fmt ", 4);
    stream << quint32(16U) << quint16(1U) << channelCount << sampleRate << byteRate
           << blockAlign << bitsPerSample;
    stream.writeRawData("data", 4);
    stream << dataSize;
    const QByteArray samples(static_cast<qsizetype>(dataSize), '\0');
    return stream.writeRawData(samples.constData(), samples.size()) == samples.size() &&
           stream.status() == QDataStream::Ok;
}

} // namespace

class TestDailyEditingWorkflow : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase() { registerWorkflowEffects(); }

    void saveAndReopenDailyEdit();
    void mediaImportIsUndoable();
    void imageDecoderOwnershipIsUnified();
    void linkedVideoImportRedoKeepsClipsSynchronized();
    void audioPluginStateSurvivesClipCopies();
    void audioPluginKeyframeEvaluationIsCompatible();
    void audioPluginKeyframeMutationsAreUndoable();
    void rejectedProjectionTransactionRestoresRuntimeModel();
    void projectionSynchronizationFailureRollsBackState();
    void targetedTimelineEditsPreserveExtensions();
    void clipGeometryUndoReplaysRustTransactions();
    void clipCrudUndoReplaysRustTransactions();
    void targetedEffectTransactionsPreserveExtensionsAndOrdering();
    void targetedAudioPluginTransactionsPreserveExtensionsAndOrdering();
    void audioParameterDurationUsesRustState();
    void targetedBatchFailureRollsBackRustAndQt();
    void sceneUndoRestoresItsClipsInRustState();
    void clipboardPasteTargetsCurrentScene();
    void pasteReportsResolvedClipEditTarget();
    void catalogItemsExposeProductMetadata();
    void catalogQueryFiltersMetadataAndCategories();
    void catalogPickerLoadsAndFilters();

  private:
    static void registerWorkflowEffects();
    static const ClipData *findClip(const TimelineController &controller, int clipId);
    static int effectIndexById(const ClipData &clip, const QString &effectId);
};

void TestDailyEditingWorkflow::registerWorkflowEffects() {
    auto &registry = EffectRegistry::instance();

    EffectMetadata transform;
    transform.id = QStringLiteral("transform");
    transform.name = QStringLiteral("Transform");
    transform.version = QStringLiteral("1.0.0");
    transform.kind = QStringLiteral("effect");
    transform.categories = {QStringLiteral("Basic")};
    transform.qmlSource = QStringLiteral("Transform.qml");
    transform.defaultParams = {
        {QStringLiteral("x"), 0.0},
        {QStringLiteral("y"), 0.0},
        {QStringLiteral("z"), 0.0},
        {QStringLiteral("scale"), 100.0},
        {QStringLiteral("opacity"), 1.0},
    };
    registry.registerEffect(transform);

    EffectMetadata image;
    image.id = QStringLiteral("image");
    image.name = QStringLiteral("Image");
    image.version = QStringLiteral("1.0.0");
    image.kind = QStringLiteral("object");
    image.categories = {QStringLiteral("Media")};
    image.qmlSource = QStringLiteral("ImageObject.qml");
    image.defaultParams = {{QStringLiteral("path"), QString()}};
    registry.registerEffect(image);

    EffectMetadata text;
    text.id = QStringLiteral("text");
    text.name = QStringLiteral("Text");
    text.version = QStringLiteral("1.0.0");
    text.kind = QStringLiteral("object");
    text.categories = {QStringLiteral("Text")};
    text.qmlSource = QStringLiteral("TextObject.qml");
    text.defaultParams = {
        {QStringLiteral("text"), QStringLiteral("Text")},
        {QStringLiteral("fontSize"), 48.0},
        {QStringLiteral("color"), QStringLiteral("#ffffff")},
    };
    registry.registerEffect(text);

    EffectMetadata blur;
    blur.id = QStringLiteral("blur");
    blur.name = QStringLiteral("Blur");
    blur.version = QStringLiteral("1.0.0");
    blur.kind = QStringLiteral("effect");
    blur.categories = {QStringLiteral("Blur")};
    blur.qmlSource = QStringLiteral("Blur.qml");
    blur.defaultParams = {
        {QStringLiteral("size"), 5.0},
        {QStringLiteral("quality"), 1},
    };
    registry.registerEffect(blur);

    EffectMetadata audio;
    audio.id = QStringLiteral("audio");
    audio.name = QStringLiteral("Audio");
    audio.version = QStringLiteral("1.0.0");
    audio.kind = QStringLiteral("object");
    audio.categories = {QStringLiteral("Media")};
    audio.defaultParams = {
        {QStringLiteral("source"), QString()},
        {QStringLiteral("playMode"), QStringLiteral("normal")},
        {QStringLiteral("linkedVideo"), false},
        {QStringLiteral("startTime"), 0.0},
        {QStringLiteral("speed"), 100.0},
    };
    registry.registerEffect(audio);
}

const ClipData *TestDailyEditingWorkflow::findClip(const TimelineController &controller, int clipId) { return controller.timeline()->findClipById(clipId); }

int TestDailyEditingWorkflow::effectIndexById(const ClipData &clip, const QString &effectId) {
    for (int i = 0; i < clip.effects.size(); ++i) {
        if (clip.effects.at(i)->id() == effectId) {
            return i;
        }
    }
    return -1;
}

void TestDailyEditingWorkflow::saveAndReopenDailyEdit() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString imagePath = dir.filePath(QStringLiteral("media.png"));
    QImage image(32, 18, QImage::Format_ARGB32);
    image.fill(QColor(QStringLiteral("#336699")));
    QVERIFY(image.save(imagePath));

    constexpr int baselineWidth = 1280;
    constexpr int baselineHeight = 720;
    constexpr double baselineFps = 30.0;
    constexpr int baselineSampleRate = 48000;
    constexpr int baselineTotalFrames = 240;

    TimelineController controller;
    controller.project()->setWidth(baselineWidth);
    controller.project()->setHeight(baselineHeight);
    controller.project()->setFps(baselineFps);
    controller.project()->setSampleRate(baselineSampleRate);
    controller.updateSceneSettings(controller.currentSceneId(), QStringLiteral("Daily Workflow"), baselineWidth, baselineHeight, baselineFps, baselineTotalFrames, QStringLiteral("Auto"), 120.0, 0.0, 10, 4, true, 10);
    QCOMPARE(controller.getSceneDuration(controller.currentSceneId()), baselineTotalFrames);

    const int imageClipId = controller.timeline()->nextClipId();
    QVariantMap importResult = controller.importMediaFile(QUrl::fromLocalFile(imagePath).toString(), 0, 0);
    QVERIFY(importResult.value(QStringLiteral("ok")).toBool());
    QCOMPARE(importResult.value(QStringLiteral("frame")).toInt(), 0);
    QCOMPARE(importResult.value(QStringLiteral("layer")).toInt(), 0);

    const auto *imageClipPtr = findClip(controller, imageClipId);
    QVERIFY2(imageClipPtr != nullptr, qPrintable(QStringLiteral("Missing image clip %1").arg(imageClipId)));
    ClipData imageClip = controller.timeline()->deepCopyClip(*imageClipPtr);
    QCOMPARE(imageClip.type, QStringLiteral("image"));
    QCOMPARE(imageClip.startFrame, 0);
    QCOMPARE(imageClip.layer, 0);
    QVERIFY(imageClip.durationFrames > 0);
    const int imageEffectIndex = effectIndexById(imageClip, QStringLiteral("image"));
    QVERIFY(imageEffectIndex >= 0);
    QCOMPARE(imageClip.effects.at(imageEffectIndex)->params().value(QStringLiteral("path")).toString(), imagePath);

    const int textClipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("text"), 30, 2);
    controller.updateClipEffectParam(textClipId, 1, QStringLiteral("text"), QStringLiteral("Daily Edit"));
    controller.updateClipEffectParam(textClipId, 1, QStringLiteral("color"), QStringLiteral("#ffee88"));
    controller.updateClipEffectParam(textClipId, 0, QStringLiteral("x"), 120.0);
    controller.updateClipEffectParam(textClipId, 0, QStringLiteral("opacity"), 0.75);
    controller.setKeyframe(textClipId, 0, QStringLiteral("x"), 0, 0.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller.setKeyframe(textClipId, 0, QStringLiteral("x"), 30, 120.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller.addEffect(textClipId, QStringLiteral("blur"));
    controller.updateClipEffectParam(textClipId, 2, QStringLiteral("size"), 12.0);
    controller.updateClipEffectParam(textClipId, 2, QStringLiteral("quality"), 2);

    const auto *textClipPtr = findClip(controller, textClipId);
    QVERIFY2(textClipPtr != nullptr, qPrintable(QStringLiteral("Missing text clip %1").arg(textClipId)));
    ClipData textClip = controller.timeline()->deepCopyClip(*textClipPtr);
    QCOMPARE(textClip.type, QStringLiteral("text"));
    QCOMPARE(textClip.startFrame, 30);
    QCOMPARE(textClip.layer, 2);
    QCOMPARE(textClip.effects.size(), 3);
    QCOMPARE(textClip.effects.at(0)->id(), QStringLiteral("transform"));
    QCOMPARE(textClip.effects.at(1)->id(), QStringLiteral("text"));
    QCOMPARE(textClip.effects.at(2)->id(), QStringLiteral("blur"));
    QCOMPARE(textClip.effects.at(1)->params().value(QStringLiteral("text")).toString(), QStringLiteral("Daily Edit"));
    QCOMPARE(textClip.effects.at(1)->params().value(QStringLiteral("color")).toString(), QStringLiteral("#ffee88"));
    QCOMPARE(textClip.effects.at(0)->params().value(QStringLiteral("opacity")).toDouble(), 0.75);
    QCOMPARE(textClip.effects.at(2)->params().value(QStringLiteral("size")).toDouble(), 12.0);
    QCOMPARE(textClip.effects.at(2)->params().value(QStringLiteral("quality")).toInt(), 2);
    QVERIFY(textClip.effects.at(2)->isEnabled());

    const QVariantList xTrack = textClip.effects.at(0)->keyframeListForUi(QStringLiteral("x"));
    QCOMPARE(xTrack.size(), 2);
    QCOMPARE(xTrack.at(0).toMap().value(QStringLiteral("frame")).toInt(), 0);
    QCOMPARE(xTrack.at(0).toMap().value(QStringLiteral("value")).toDouble(), 0.0);
    QCOMPARE(xTrack.at(1).toMap().value(QStringLiteral("frame")).toInt(), 30);
    QCOMPARE(xTrack.at(1).toMap().value(QStringLiteral("value")).toDouble(), 120.0);

    controller.copyClip(textClipId);
    const QVariantMap pasteResult = controller.pasteClip(150, 2);
    QVERIFY(pasteResult.value(QStringLiteral("ok")).toBool());
    const int pastedTextClipId = controller.timeline()->nextClipId() - 1;
    const auto *pastedTextClipPtr = findClip(controller, pastedTextClipId);
    QVERIFY2(pastedTextClipPtr != nullptr, qPrintable(QStringLiteral("Missing pasted text clip %1").arg(pastedTextClipId)));
    ClipData pastedTextClip = controller.timeline()->deepCopyClip(*pastedTextClipPtr);
    QCOMPARE(pastedTextClip.type, QStringLiteral("text"));
    QCOMPARE(pastedTextClip.startFrame, 150);
    QCOMPARE(pastedTextClip.layer, 2);
    QCOMPARE(pastedTextClip.effects.at(1)->params().value(QStringLiteral("text")).toString(), QStringLiteral("Daily Edit"));

    const QString projectPath = dir.filePath(QStringLiteral("daily-edit.aviqtl"));
    QVERIFY(controller.saveProject(projectPath));

    TimelineController loaded;
    QVERIFY(loaded.loadProject(projectPath));
    QCOMPARE(loaded.project()->width(), baselineWidth);
    QCOMPARE(loaded.project()->height(), baselineHeight);
    QCOMPARE(loaded.project()->fps(), baselineFps);
    QCOMPARE(loaded.project()->sampleRate(), baselineSampleRate);
    QCOMPARE(loaded.getSceneDuration(loaded.currentSceneId()), baselineTotalFrames);

    const auto *loadedImageClipPtr = findClip(loaded, imageClipId);
    QVERIFY2(loadedImageClipPtr != nullptr, qPrintable(QStringLiteral("Missing loaded image clip %1").arg(imageClipId)));
    ClipData loadedImageClip = loaded.timeline()->deepCopyClip(*loadedImageClipPtr);
    QCOMPARE(loadedImageClip.type, QStringLiteral("image"));
    QCOMPARE(loadedImageClip.startFrame, 0);
    QCOMPARE(loadedImageClip.layer, 0);
    const int loadedImageEffectIndex = effectIndexById(loadedImageClip, QStringLiteral("image"));
    QVERIFY(loadedImageEffectIndex >= 0);
    QCOMPARE(loadedImageClip.effects.at(loadedImageEffectIndex)->params().value(QStringLiteral("path")).toString(), imagePath);

    const auto *loadedTextClipPtr = findClip(loaded, textClipId);
    QVERIFY2(loadedTextClipPtr != nullptr, qPrintable(QStringLiteral("Missing loaded text clip %1").arg(textClipId)));
    ClipData loadedTextClip = loaded.timeline()->deepCopyClip(*loadedTextClipPtr);
    QCOMPARE(loadedTextClip.type, QStringLiteral("text"));
    QCOMPARE(loadedTextClip.startFrame, 30);
    QCOMPARE(loadedTextClip.layer, 2);
    QCOMPARE(loadedTextClip.effects.size(), 3);
    QCOMPARE(loadedTextClip.effects.at(0)->id(), QStringLiteral("transform"));
    QCOMPARE(loadedTextClip.effects.at(1)->id(), QStringLiteral("text"));
    QCOMPARE(loadedTextClip.effects.at(2)->id(), QStringLiteral("blur"));
    QCOMPARE(loadedTextClip.effects.at(1)->params().value(QStringLiteral("text")).toString(), QStringLiteral("Daily Edit"));
    QCOMPARE(loadedTextClip.effects.at(1)->params().value(QStringLiteral("color")).toString(), QStringLiteral("#ffee88"));
    QCOMPARE(loadedTextClip.effects.at(0)->params().value(QStringLiteral("opacity")).toDouble(), 0.75);
    QCOMPARE(loadedTextClip.effects.at(2)->params().value(QStringLiteral("size")).toDouble(), 12.0);
    QCOMPARE(loadedTextClip.effects.at(2)->params().value(QStringLiteral("quality")).toInt(), 2);
    QVERIFY(loadedTextClip.effects.at(2)->isEnabled());

    const QVariantList loadedXTrack = loadedTextClip.effects.at(0)->keyframeListForUi(QStringLiteral("x"));
    QCOMPARE(loadedXTrack.size(), 2);
    QCOMPARE(loadedXTrack.at(0).toMap().value(QStringLiteral("frame")).toInt(), 0);
    QCOMPARE(loadedXTrack.at(0).toMap().value(QStringLiteral("value")).toDouble(), 0.0);
    QCOMPARE(loadedXTrack.at(0).toMap().value(QStringLiteral("interp")).toString(), QStringLiteral("linear"));
    QCOMPARE(loadedXTrack.at(1).toMap().value(QStringLiteral("frame")).toInt(), 30);
    QCOMPARE(loadedXTrack.at(1).toMap().value(QStringLiteral("value")).toDouble(), 120.0);
    QCOMPARE(loadedXTrack.at(1).toMap().value(QStringLiteral("interp")).toString(), QStringLiteral("linear"));

    const auto *loadedPastedTextClipPtr = findClip(loaded, pastedTextClipId);
    QVERIFY2(loadedPastedTextClipPtr != nullptr, qPrintable(QStringLiteral("Missing loaded pasted text clip %1").arg(pastedTextClipId)));
    ClipData loadedPastedTextClip = loaded.timeline()->deepCopyClip(*loadedPastedTextClipPtr);
    QCOMPARE(loadedPastedTextClip.type, QStringLiteral("text"));
    QCOMPARE(loadedPastedTextClip.startFrame, 150);
    QCOMPARE(loadedPastedTextClip.layer, 2);
    QCOMPARE(loadedPastedTextClip.effects.at(1)->params().value(QStringLiteral("text")).toString(), QStringLiteral("Daily Edit"));
}

void TestDailyEditingWorkflow::mediaImportIsUndoable() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString imagePath = dir.filePath(QStringLiteral("undo-import.png"));
    QImage image(16, 16, QImage::Format_ARGB32);
    image.fill(Qt::cyan);
    QVERIFY(image.save(imagePath));

    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    const QVariantMap result = controller.importMediaFile(QUrl::fromLocalFile(imagePath).toString(), 12, 3);
    QVERIFY(result.value(QStringLiteral("ok")).toBool());

    const ClipData *imported = findClip(controller, clipId);
    QVERIFY(imported != nullptr);
    const int importedDuration = imported->durationFrames;
    const int imageEffectIndex = effectIndexById(*imported, QStringLiteral("image"));
    QVERIFY(imageEffectIndex >= 0);
    QCOMPARE(imported->effects.at(imageEffectIndex)->params().value(QStringLiteral("path")).toString(), imagePath);

    controller.timeline()->undo();
    QVERIFY(findClip(controller, clipId) == nullptr);

    controller.timeline()->redo();
    imported = findClip(controller, clipId);
    QVERIFY(imported != nullptr);
    QCOMPARE(imported->startFrame, result.value(QStringLiteral("frame")).toInt());
    QCOMPARE(imported->layer, result.value(QStringLiteral("layer")).toInt());
    QCOMPARE(imported->durationFrames, importedDuration);
    const int restoredImageEffectIndex = effectIndexById(*imported, QStringLiteral("image"));
    QVERIFY(restoredImageEffectIndex >= 0);
    QCOMPARE(imported->effects.at(restoredImageEffectIndex)->params().value(QStringLiteral("path")).toString(), imagePath);
}

void TestDailyEditingWorkflow::imageDecoderOwnershipIsUnified() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString firstPath = dir.filePath(QStringLiteral("first.png"));
    const QString secondPath = dir.filePath(QStringLiteral("second.png"));
    QImage firstImage(16, 16, QImage::Format_ARGB32);
    firstImage.fill(Qt::red);
    QVERIFY(firstImage.save(firstPath));
    QImage secondImage(16, 16, QImage::Format_ARGB32);
    secondImage.fill(Qt::blue);
    QVERIFY(secondImage.save(secondPath));

    TimelineController controller;
    VideoFrameStore frameStore;
    controller.setVideoFrameStore(&frameStore);

    const int clipId = controller.timeline()->nextClipId();
    QVERIFY(controller.importMediaFile(QUrl::fromLocalFile(firstPath).toString(), 0, 0)
                .value(QStringLiteral("ok"))
                .toBool());
    QTRY_COMPARE(controller.mediaManager()->findChildren<ImageDecoder *>().size(), 1);

    controller.requestImageLoad(clipId, firstPath);
    QCoreApplication::processEvents();
    QCOMPARE(controller.mediaManager()->findChildren<ImageDecoder *>().size(), 1);
    QCOMPARE(qobject_cast<ImageDecoder *>(controller.mediaManager()->decoderForClip(clipId))->source(),
             QUrl::fromLocalFile(firstPath));

    controller.requestImageLoad(clipId, secondPath);
    QTRY_COMPARE(controller.mediaManager()->findChildren<ImageDecoder *>().size(), 1);
    auto *replacement = qobject_cast<ImageDecoder *>(controller.mediaManager()->decoderForClip(clipId));
    QVERIFY(replacement != nullptr);
    QCOMPARE(replacement->source(), QUrl::fromLocalFile(secondPath));

    controller.deleteClip(clipId);
    QTRY_COMPARE(controller.mediaManager()->findChildren<ImageDecoder *>().size(), 0);
    QVERIFY(frameStore.frame(QString::number(clipId)).isNull());
}

void TestDailyEditingWorkflow::linkedVideoImportRedoKeepsClipsSynchronized() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString videoPath = dir.filePath(QStringLiteral("linked-import.mp4"));
    VideoEncoder encoder;
    VideoEncoder::Config config;
    config.width = 32;
    config.height = 18;
    config.fps_num = 30;
    config.fps_den = 1;
    config.codecName = QStringLiteral("libx264");
    config.outputUrl = videoPath;
    config.preset = QStringLiteral("ultrafast");
    QVERIFY(encoder.open(config));
    for (int frame = 0; frame < 30; ++frame) {
        QImage image(config.width, config.height, QImage::Format_RGBA8888);
        image.fill(Qt::darkCyan);
        QVERIFY(encoder.pushFrame(image, frame));
    }
    encoder.close();
    QVERIFY(QFileInfo(videoPath).size() > 0);

    TimelineController controller;
    controller.createObject(QStringLiteral("image"), 70, 2);
    controller.createObject(QStringLiteral("image"), 80, 3);

    const int videoClipId = controller.timeline()->nextClipId();
    const QSignalBlocker mediaRefreshBlocker(controller.timeline());
    const QVariantMap result = controller.importMediaFile(QUrl::fromLocalFile(videoPath).toString(), 0, 2);
    QVERIFY(result.value(QStringLiteral("ok")).toBool());
    const int audioClipId = videoClipId + 1;
    const int resolvedFrame = result.value(QStringLiteral("frame")).toInt();
    QCOMPARE(resolvedFrame, 0);
    const ClipData *importedVideo = findClip(controller, videoClipId);
    const ClipData *importedAudio = findClip(controller, audioClipId);
    QVERIFY(importedVideo != nullptr);
    QVERIFY(importedAudio != nullptr);
    QCOMPARE(importedVideo->startFrame, resolvedFrame);
    QCOMPARE(importedAudio->startFrame, resolvedFrame);

    controller.timeline()->undo();
    QVERIFY(findClip(controller, videoClipId) == nullptr);
    QVERIFY(findClip(controller, audioClipId) == nullptr);

    controller.timeline()->redo();
    const ClipData *videoClip = findClip(controller, videoClipId);
    const ClipData *audioClip = findClip(controller, audioClipId);
    QVERIFY(videoClip != nullptr);
    QVERIFY(audioClip != nullptr);
    QCOMPARE(videoClip->startFrame, resolvedFrame);
    QCOMPARE(audioClip->startFrame, resolvedFrame);
    QCOMPARE(videoClip->durationFrames, audioClip->durationFrames);
}

void TestDailyEditingWorkflow::audioPluginStateSurvivesClipCopies() {
    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("audio"), 0, 0);

    auto *source = controller.timeline()->findClipById(clipId);
    QVERIFY(source != nullptr);
    AudioPluginState plugin;
    plugin.id = QStringLiteral("test.plugin");
    plugin.enabled = false;
    plugin.params = {{QStringLiteral("2"), 0.75}};
    plugin.keyframeTracks = {
        {QStringLiteral("2"),
         QVariantList{
             QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 0.25}},
             QVariantMap{{QStringLiteral("frame"), 75}, {QStringLiteral("value"), 1.0}},
         }},
    };
    controller.timeline()->addAudioPlugin(clipId, plugin, plugin.id);
    source = controller.timeline()->findClipById(clipId);
    QVERIFY(source != nullptr);
    QCOMPARE(source->audioPlugins.size(), 1);
    const int splitFrame = source->startFrame + (source->durationFrames / 2);

    const ClipData copied = controller.timeline()->deepCopyClip(*source);
    QCOMPARE(copied.audioPlugins.size(), 1);
    QCOMPARE(copied.audioPlugins.first().id, plugin.id);
    QCOMPARE(copied.audioPlugins.first().enabled, plugin.enabled);
    QCOMPARE(copied.audioPlugins.first().params, plugin.params);
    QCOMPARE(copied.audioPlugins.first().keyframeTracks, plugin.keyframeTracks);

    controller.copyClip(clipId);
    const QVariantMap pasteResult = controller.pasteClip(150, 0);
    QVERIFY(pasteResult.value(QStringLiteral("ok")).toBool());
    const int pastedId = controller.timeline()->nextClipId() - 1;
    const ClipData *pasted = findClip(controller, pastedId);
    QVERIFY(pasted != nullptr);
    QCOMPARE(pasted->audioPlugins.size(), 1);
    QCOMPARE(pasted->audioPlugins.first().keyframeTracks, plugin.keyframeTracks);

    controller.timeline()->splitClip(clipId, splitFrame);
    const int splitId = controller.timeline()->nextClipId() - 1;
    const ClipData *split = findClip(controller, splitId);
    QVERIFY(split != nullptr);
    QCOMPARE(split->audioPlugins.size(), 1);
    QCOMPARE(split->audioPlugins.first().params, plugin.params);
    const QVariantMap splitTracks = split->audioPlugins.first().keyframeTracks;
    QVERIFY(splitTracks.contains(QStringLiteral("2")));
    const QVariantList splitPoints = splitTracks.value(QStringLiteral("2")).toMap()
                                         .value(QStringLiteral("points"))
                                         .toList();
    QCOMPARE(splitPoints.size(), 1);
    QCOMPARE(splitPoints.first().toMap().value(QStringLiteral("frame")).toInt(), 25);
    QCOMPARE(splitPoints.first().toMap().value(QStringLiteral("value")).toDouble(), 1.0);

    controller.timeline()->undo();
    QVERIFY(findClip(controller, splitId) == nullptr);
    const ClipData *restored = findClip(controller, clipId);
    QVERIFY(restored != nullptr);
    QCOMPARE(restored->audioPlugins.first().keyframeTracks, plugin.keyframeTracks);
    controller.timeline()->redo();
    split = findClip(controller, splitId);
    QVERIFY(split != nullptr);
    QCOMPARE(split->audioPlugins.size(), 1);
    QCOMPARE(split->audioPlugins.first().keyframeTracks, splitTracks);
}

void TestDailyEditingWorkflow::rejectedProjectionTransactionRestoresRuntimeModel() {
    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("text"), 0, 0);

    auto *clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    QVERIFY(clip->effects.size() >= 2);
    auto *removedEffect = clip->effects.at(1);
    QVERIFY(removedEffect != nullptr);
    const bool previousEnabled = removedEffect->isEnabled();
    const int previousLayer = clip->layer;
    const int previousStart = clip->startFrame;
    const int previousDuration = clip->durationFrames;

    controller.timeline()->beginTimelineProjectionTransaction();
    controller.timeline()->setEffectEnabledInternal(clipId, 1, !previousEnabled);
    controller.timeline()->removeEffectInternal(clipId, 1);
    controller.timeline()->updateClipInternal(clipId, 128, previousStart, previousDuration, false,
                                              true);
    QVERIFY(!controller.timeline()->endTimelineProjectionTransaction());

    clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->layer, previousLayer);
    QCOMPARE(clip->startFrame, previousStart);
    QCOMPARE(clip->durationFrames, previousDuration);
    QVERIFY(clip->effects.size() >= 2);
    QCOMPARE(clip->effects.at(1), removedEffect);
    QCOMPARE(removedEffect->isEnabled(), previousEnabled);
}

void TestDailyEditingWorkflow::projectionSynchronizationFailureRollsBackState() {
    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("text"), 0, 0);

    auto *clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    QVERIFY(clip->effects.size() >= 2);
    auto *effect = clip->effects.first();
    QVERIFY(effect != nullptr);
    const bool previousEnabled = effect->isEnabled();
    const QVariantMap before = controller.timeline()->timelineStateSnapshot();

    auto *detached = clip->effects.takeLast();
    controller.timeline()->setEffectEnabledInternal(clipId, 0, !previousEnabled);
    QCOMPARE(controller.timeline()->timelineStateSnapshot(), before);
    QCOMPARE(effect->isEnabled(), previousEnabled);
    clip->effects.append(detached);

    detached = clip->effects.takeLast();
    controller.timeline()->beginTimelineProjectionTransaction();
    controller.timeline()->setEffectEnabledInternal(clipId, 0, !previousEnabled);
    QVERIFY(!controller.timeline()->endTimelineProjectionTransaction());
    QCOMPARE(controller.timeline()->timelineStateSnapshot(), before);
    QCOMPARE(effect->isEnabled(), previousEnabled);
    clip->effects.append(detached);
}

void TestDailyEditingWorkflow::targetedTimelineEditsPreserveExtensions() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 0, 0);
    QVERIFY(timeline.findClipById(clipId) != nullptr);

    QVariantMap document = timeline.timelineStateSnapshot();
    QVariantList scenes = document.value(QStringLiteral("scenes")).toList();
    QVariantMap scene = scenes.first().toMap();
    scene.insert(QStringLiteral("reviewSceneExtension"), QStringLiteral("scene-token"));
    scenes[0] = scene;
    document.insert(QStringLiteral("scenes"), scenes);
    QVariantList clips = document.value(QStringLiteral("clips")).toList();
    QVariantMap clip = clips.first().toMap();
    clip.insert(QStringLiteral("reviewClipExtension"), QStringLiteral("clip-token"));
    clips[0] = clip;
    document.insert(QStringLiteral("clips"), clips);
    QVERIFY(timeline.resetTimelineState(document, timeline.nextClipId(), timeline.nextSceneId()));

    timeline.updateClipInternal(clipId, 2, 12, 40, false, true);
    timeline.setClipByUpperObjectInternal(clipId, true, false);
    timeline.setLayerStateInternal(0, 3, true, 0);

    const QVariantMap updated = timeline.timelineStateSnapshot();
    const QVariantMap updatedScene = updated.value(QStringLiteral("scenes"))
                                         .toList()
                                         .first()
                                         .toMap();
    const QVariantMap updatedClip = updated.value(QStringLiteral("clips"))
                                        .toList()
                                        .first()
                                        .toMap();
    QCOMPARE(updatedScene.value(QStringLiteral("reviewSceneExtension")).toString(),
             QStringLiteral("scene-token"));
    QCOMPARE(updatedClip.value(QStringLiteral("reviewClipExtension")).toString(),
             QStringLiteral("clip-token"));
    QCOMPARE(updatedClip.value(QStringLiteral("layer")).toInt(), 2);
    QCOMPARE(updatedClip.value(QStringLiteral("start")).toInt(), 12);
    QCOMPARE(updatedClip.value(QStringLiteral("duration")).toInt(), 40);
    QVERIFY(updatedClip.value(QStringLiteral("clipByUpperObject")).toBool());
}

void TestDailyEditingWorkflow::clipGeometryUndoReplaysRustTransactions() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int firstId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 0, 0);
    const auto *first = timeline.findClipById(firstId);
    QVERIFY(first != nullptr);
    const int originalStart = first->startFrame;
    const int originalDuration = first->durationFrames;

    timeline.selectClip(firstId);
    timeline.updateClip(firstId, 1, 20, 40);
    first = timeline.findClipById(firstId);
    QVERIFY(first != nullptr);
    QCOMPARE(first->layer, 1);
    QCOMPARE(first->startFrame, 20);
    QCOMPARE(first->durationFrames, 40);

    timeline.setLayerStateInternal(0, 1, true, UpdateLayerStateCommand::Lock);
    QSignalSpy clipsChanged(&timeline, &TimelineService::clipsChanged);
    timeline.undo();
    first = timeline.findClipById(firstId);
    QVERIFY(first != nullptr);
    QCOMPARE(first->layer, 0);
    QCOMPARE(first->startFrame, originalStart);
    QCOMPARE(first->durationFrames, originalDuration);
    QCOMPARE(clipsChanged.count(), 1);
    QCOMPARE(selection.selectedClipData().value(QStringLiteral("layer")).toInt(), 0);

    clipsChanged.clear();
    timeline.redo();
    first = timeline.findClipById(firstId);
    QVERIFY(first != nullptr);
    QCOMPARE(first->layer, 1);
    QCOMPARE(first->startFrame, 20);
    QCOMPARE(first->durationFrames, 40);
    QCOMPARE(clipsChanged.count(), 1);
    QCOMPARE(selection.selectedClipData().value(QStringLiteral("durationFrames")).toInt(), 40);

    timeline.setLayerStateInternal(0, 1, false, UpdateLayerStateCommand::Lock);
    timeline.undo();
    const int secondId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 100, 0);
    timeline.applySelectionIds({firstId, secondId});
    timeline.moveSelectedClips(2, 5);

    first = timeline.findClipById(firstId);
    const auto *second = timeline.findClipById(secondId);
    QVERIFY(first != nullptr);
    QVERIFY(second != nullptr);
    QCOMPARE(first->layer, 2);
    QCOMPARE(second->layer, 2);
    const int firstMovedStart = first->startFrame;
    const int secondMovedStart = second->startFrame;

    timeline.setLayerStateInternal(0, 2, true, UpdateLayerStateCommand::Lock);
    clipsChanged.clear();
    timeline.undo();
    first = timeline.findClipById(firstId);
    second = timeline.findClipById(secondId);
    QVERIFY(first != nullptr);
    QVERIFY(second != nullptr);
    QCOMPARE(first->layer, 0);
    QCOMPARE(second->layer, 0);
    QCOMPARE(clipsChanged.count(), 1);
    QCOMPARE(selection.selectedClipData().value(QStringLiteral("layer")).toInt(), 0);

    clipsChanged.clear();
    timeline.redo();
    first = timeline.findClipById(firstId);
    second = timeline.findClipById(secondId);
    QVERIFY(first != nullptr);
    QVERIFY(second != nullptr);
    QCOMPARE(first->layer, 2);
    QCOMPARE(second->layer, 2);
    QCOMPARE(first->startFrame, firstMovedStart);
    QCOMPARE(second->startFrame, secondMovedStart);
    QCOMPARE(clipsChanged.count(), 1);
    QCOMPARE(selection.selectedClipData().value(QStringLiteral("layer")).toInt(), 2);

    const QVariantList rustClips =
        timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
    for (const int clipId : {firstId, secondId}) {
        const auto *projected = timeline.findClipById(clipId);
        QVERIFY(projected != nullptr);
        const auto rustIt = std::ranges::find_if(rustClips, [clipId](const QVariant &value) {
            return value.toMap().value(QStringLiteral("id")).toInt() == clipId;
        });
        QVERIFY(rustIt != rustClips.cend());
        const QVariantMap rustClip = rustIt->toMap();
        QCOMPARE(projected->layer, rustClip.value(QStringLiteral("layer")).toInt());
        QCOMPARE(projected->startFrame, rustClip.value(QStringLiteral("start")).toInt());
        QCOMPARE(projected->durationFrames,
                 rustClip.value(QStringLiteral("duration")).toInt());
    }
}

void TestDailyEditingWorkflow::clipCrudUndoReplaysRustTransactions() {
    SelectionService selection;
    TimelineService timeline(&selection);

    const int createdId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 0, 2);
    const auto *created = timeline.findClipById(createdId);
    QVERIFY(created != nullptr);
    QCOMPARE(created->effects.size(), 2);
    const QVariantMap createdDocument = timeline.timelineStateSnapshot()
                                            .value(QStringLiteral("clips"))
                                            .toList()
                                            .first()
                                            .toMap();

    timeline.undo();
    QVERIFY(timeline.findClipById(createdId) == nullptr);
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);

    timeline.setLayerStateInternal(0, 2, true, UpdateLayerStateCommand::Lock);
    timeline.redo();
    created = timeline.findClipById(createdId);
    QVERIFY(created != nullptr);
    QCOMPARE(created->layer, 2);
    QCOMPARE(created->effects.size(), 2);
    QCOMPARE(created->effects.at(0)->id(), QStringLiteral("transform"));
    QCOMPARE(created->effects.at(1)->id(), QStringLiteral("text"));
    const QVariantList recreatedDocuments =
        timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
    const auto recreatedIt = std::ranges::find_if(
        recreatedDocuments, [createdId](const QVariant &value) {
            return value.toMap().value(QStringLiteral("id")).toInt() == createdId;
        });
    QVERIFY(recreatedIt != recreatedDocuments.cend());
    QCOMPARE(recreatedIt->toMap(), createdDocument);

    timeline.setLayerStateInternal(0, 2, false, UpdateLayerStateCommand::Lock);
    timeline.undoStack()->clear();

    QList<int> clipIds;
    for (int index = 0; index < 4; ++index) {
        const int clipId = timeline.nextClipId();
        timeline.createClip(QStringLiteral("text"), index * 100, index);
        clipIds.append(clipId);
    }
    QCOMPARE(timeline.clips().size(), 5);
    timeline.undoStack()->clear();

    const auto projectedIds = [&timeline]() {
        QList<int> ids;
        for (const auto &clip : timeline.clips()) {
            ids.append(clip.id);
        }
        return ids;
    };
    const auto rustIds = [&timeline]() {
        QList<int> ids;
        const QVariantList documents =
            timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
        for (const QVariant &document : documents) {
            ids.append(document.toMap().value(QStringLiteral("id")).toInt());
        }
        return ids;
    };
    const QList<int> originalOrder = projectedIds();
    QCOMPARE(rustIds(), originalOrder);

    timeline.deleteClipsByIds({clipIds.at(1), clipIds.at(3)});
    QList<int> deletedOrder = originalOrder;
    deletedOrder.removeAll(clipIds.at(1));
    deletedOrder.removeAll(clipIds.at(3));
    QCOMPARE(projectedIds(), deletedOrder);
    QCOMPARE(rustIds(), deletedOrder);

    timeline.undo();
    QCOMPARE(projectedIds(), originalOrder);
    QCOMPARE(rustIds(), originalOrder);
    for (int clipId : {clipIds.at(1), clipIds.at(3)}) {
        const auto *restored = timeline.findClipById(clipId);
        QVERIFY(restored != nullptr);
        QCOMPARE(restored->effects.size(), 2);
        QCOMPARE(restored->effects.at(0)->id(), QStringLiteral("transform"));
        QCOMPARE(restored->effects.at(1)->id(), QStringLiteral("text"));
    }

    timeline.redo();
    QCOMPARE(projectedIds(), deletedOrder);
    QCOMPARE(rustIds(), deletedOrder);
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);

    timeline.undo();
    QCOMPARE(projectedIds(), originalOrder);
    QCOMPARE(rustIds(), originalOrder);
    for (int clipId : {clipIds.at(1), clipIds.at(3)}) {
        const auto *restored = timeline.findClipById(clipId);
        QVERIFY(restored != nullptr);
        QCOMPARE(restored->effects.size(), 2);
    }
}

void TestDailyEditingWorkflow::targetedEffectTransactionsPreserveExtensionsAndOrdering() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 0, 0);
    timeline.addEffect(clipId, QStringLiteral("blur"));

    const auto *clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->effects.size(), 3);

    QVariantMap document = timeline.timelineStateSnapshot();
    QVariantList clips = document.value(QStringLiteral("clips")).toList();
    QVariantMap clipDocument = clips.first().toMap();
    QVariantList effects = clipDocument.value(QStringLiteral("effects")).toList();
    for (qsizetype index = 0; index < effects.size(); ++index) {
        QVariantMap effect = effects.at(index).toMap();
        effect.insert(QStringLiteral("workflowExtension"),
                      effect.value(QStringLiteral("id")).toString() + QStringLiteral("-token"));
        if (effect.value(QStringLiteral("id")).toString() == QLatin1String("text")) {
            effect.remove(QStringLiteral("keyframes"));
        }
        effects[index] = effect;
    }
    clipDocument.insert(QStringLiteral("effects"), effects);
    clips[0] = clipDocument;
    document.insert(QStringLiteral("clips"), clips);
    QVERIFY(timeline.resetTimelineState(document, timeline.nextClipId(), timeline.nextSceneId()));
    const QVariantMap before = timeline.timelineStateSnapshot();

    const auto effectDocuments = [&timeline]() {
        return timeline.timelineStateSnapshot()
            .value(QStringLiteral("clips"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("effects"))
            .toList();
    };
    const auto effectIds = [&effectDocuments]() {
        QStringList ids;
        for (const QVariant &effect : effectDocuments()) {
            ids.append(effect.toMap().value(QStringLiteral("id")).toString());
        }
        return ids;
    };

    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    auto detachedEffect = std::unique_ptr<EffectModel>(clip->effects.at(2)->clone());
    timeline.pasteEffectInternal(clipId, 0, detachedEffect.get());
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("text"),
                                       QStringLiteral("blur")}));

    SelectionService pasteSelection;
    TimelineService pasteTimeline(&pasteSelection);
    const int pasteTargetId = pasteTimeline.nextClipId();
    pasteTimeline.createClip(QStringLiteral("text"), 0, 0);
    const QVariantMap beforePaste = pasteTimeline.timelineStateSnapshot();
    pasteTimeline.undoStack()->push(
        new PasteEffectCommand(&pasteTimeline, pasteTargetId, 1, detachedEffect.get()));
    const auto *pastedClip = pasteTimeline.findClipById(pasteTargetId);
    QVERIFY(pastedClip != nullptr);
    QCOMPARE(pastedClip->effects.size(), 3);
    QCOMPARE(pastedClip->effects.at(0)->id(), QStringLiteral("transform"));
    QCOMPARE(pastedClip->effects.at(1)->id(), QStringLiteral("blur"));
    QCOMPARE(pastedClip->effects.at(2)->id(), QStringLiteral("text"));
    const QVariantMap afterPaste = pasteTimeline.timelineStateSnapshot();
    pasteTimeline.undo();
    QCOMPARE(pasteTimeline.timelineStateSnapshot(), beforePaste);
    pasteTimeline.redo();
    QCOMPARE(pasteTimeline.timelineStateSnapshot(), afterPaste);

    timeline.setEffectEnabled(clipId, 2, false);
    QVariantList changedEffects = effectDocuments();
    QVERIFY(!changedEffects.at(2).toMap().value(QStringLiteral("enabled")).toBool());
    QCOMPARE(changedEffects.at(2).toMap().value(QStringLiteral("workflowExtension")).toString(),
             QStringLiteral("blur-token"));
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);

    timeline.reorderEffects(clipId, 2, 1);
    QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("blur"),
                                       QStringLiteral("text")}));
    changedEffects = effectDocuments();
    QCOMPARE(changedEffects.at(1).toMap().value(QStringLiteral("workflowExtension")).toString(),
             QStringLiteral("blur-token"));
    QCOMPARE(changedEffects.at(2).toMap().value(QStringLiteral("workflowExtension")).toString(),
             QStringLiteral("text-token"));
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);

    timeline.removeEffect(clipId, 1);
    QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("blur")}));
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("text"),
                                       QStringLiteral("blur")}));

    timeline.removeMultipleEffects(clipId, {2, 1});
    QCOMPARE(effectIds(), QStringList({QStringLiteral("transform")}));
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("text"),
                                       QStringLiteral("blur")}));

    // Successive edits to the same parameter are expected to merge into one undo command.
    timeline.updateEffectParam(clipId, 0, QStringLiteral("x"), 12.0);
    timeline.updateEffectParam(clipId, 0, QStringLiteral("x"), 24.0);
    QVariantMap transformDocument = effectDocuments().first().toMap();
    QCOMPARE(transformDocument.value(QStringLiteral("workflowExtension")).toString(),
             QStringLiteral("transform-token"));
    QCOMPARE(transformDocument.value(QStringLiteral("params"))
                 .toMap()
                 .value(QStringLiteral("x"))
                 .toDouble(),
             24.0);
    QCOMPARE(transformDocument.value(QStringLiteral("keyframes"))
                 .toMap()
                 .value(QStringLiteral("x"))
                 .toMap()
                 .value(QStringLiteral("start"))
                 .toMap()
                 .value(QStringLiteral("value"))
                 .toDouble(),
             24.0);
    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->effects.first()->params().value(QStringLiteral("x")).toDouble(), 24.0);
    const QVariantMap afterParameterUpdate = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterParameterUpdate);
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);

    const auto transformPoints = [&effectDocuments]() {
        return effectDocuments()
            .first()
            .toMap()
            .value(QStringLiteral("keyframes"))
            .toMap()
            .value(QStringLiteral("x"))
            .toMap()
            .value(QStringLiteral("points"))
            .toList();
    };
    const auto containsFrame = [](const QVariantList &points, int frame) {
        return std::ranges::any_of(points, [frame](const QVariant &point) {
            return point.toMap().value(QStringLiteral("frame")).toInt() == frame;
        });
    };

    timeline.setKeyframe(clipId, 0, QStringLiteral("x"), 10, 10.0,
                         {{QStringLiteral("interp"), QStringLiteral("linear")}});
    QVERIFY(containsFrame(transformPoints(), 10));
    transformDocument = effectDocuments().first().toMap();
    QCOMPARE(transformDocument.value(QStringLiteral("workflowExtension")).toString(),
             QStringLiteral("transform-token"));
    const QVariantMap afterKeyframeSet = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeSet);

    timeline.moveKeyframe(clipId, 0, QStringLiteral("x"), 10, 12);
    QVERIFY(!containsFrame(transformPoints(), 10));
    QVERIFY(containsFrame(transformPoints(), 12));
    const QVariantMap afterKeyframeMove = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeSet);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeMove);

    timeline.removeKeyframe(clipId, 0, QStringLiteral("x"), 12);
    QVERIFY(!containsFrame(transformPoints(), 12));
    const QVariantMap afterKeyframeRemoval = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeMove);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeRemoval);
    timeline.undo();
    timeline.undo();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
}

void TestDailyEditingWorkflow::targetedAudioPluginTransactionsPreserveExtensionsAndOrdering() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("audio"), 0, 0);

    AudioPluginState gain;
    gain.id = QStringLiteral("test.gain");
    gain.params.insert(QStringLiteral("0"), 0.0);
    gain.keyframeTracks.insert(
        QStringLiteral("0"),
        QVariantMap{
            {QStringLiteral("start"),
             QVariantMap{{QStringLiteral("frame"), 0},
                         {QStringLiteral("value"), 0.0},
                         {QStringLiteral("interp"), QStringLiteral("linear")}}},
            {QStringLiteral("points"),
             QVariantList{QVariantMap{{QStringLiteral("frame"), 12},
                                      {QStringLiteral("value"), 1.0},
                                      {QStringLiteral("interp"),
                                       QStringLiteral("linear")}}}},
        });
    AudioPluginState delay;
    delay.id = QStringLiteral("test.delay");
    delay.params.insert(QStringLiteral("0"), 1.0);
    timeline.addAudioPlugin(clipId, gain, gain.id);
    timeline.addAudioPlugin(clipId, delay, delay.id);

    const auto *clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->audioPlugins.size(), 2);
    QVariantMap document = timeline.timelineStateSnapshot();
    QVariantList clips = document.value(QStringLiteral("clips")).toList();
    QVariantMap clipDocument = clips.first().toMap();
    QVariantList plugins = clipDocument.value(QStringLiteral("audioPlugins")).toList();
    QCOMPARE(plugins.size(), 2);
    for (qsizetype index = 0; index < plugins.size(); ++index) {
        QVariantMap plugin = plugins.at(index).toMap();
        plugin.insert(
            QStringLiteral("workflowPluginExtension"),
            plugin.value(QStringLiteral("id")).toString() + QStringLiteral("-token"));
        plugins[index] = plugin;
    }
    clipDocument.insert(QStringLiteral("audioPlugins"), plugins);
    clips[0] = clipDocument;
    document.insert(QStringLiteral("clips"), clips);
    QVERIFY(timeline.resetTimelineState(document, timeline.nextClipId(), timeline.nextSceneId()));
    timeline.undoStack()->clear();
    const QVariantMap before = timeline.timelineStateSnapshot();

    const auto pluginDocuments = [&timeline]() {
        return timeline.timelineStateSnapshot()
            .value(QStringLiteral("clips"))
            .toList()
            .first()
            .toMap()
            .value(QStringLiteral("audioPlugins"))
            .toList();
    };
    const auto pluginIds = [&pluginDocuments]() {
        QStringList ids;
        for (const QVariant &plugin : pluginDocuments()) {
            ids.append(plugin.toMap().value(QStringLiteral("id")).toString());
        }
        return ids;
    };
    const auto containsFrame = [](const QVariantList &points, int frame) {
        return std::ranges::any_of(points, [frame](const QVariant &point) {
            return point.toMap().value(QStringLiteral("frame")).toInt() == frame;
        });
    };

    timeline.setAudioPluginEnabled(clipId, 0, false);
    QVariantList changedPlugins = pluginDocuments();
    QVERIFY(!changedPlugins.first().toMap().value(QStringLiteral("enabled")).toBool());
    QCOMPARE(changedPlugins.first()
                 .toMap()
                 .value(QStringLiteral("workflowPluginExtension"))
                 .toString(),
             QStringLiteral("test.gain-token"));
    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QVERIFY(!clip->audioPlugins.first().enabled);
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);

    timeline.setAudioPluginParam(clipId, 0, 0, 0.5F);
    changedPlugins = pluginDocuments();
    QVariantMap gainDocument = changedPlugins.first().toMap();
    QCOMPARE(gainDocument.value(QStringLiteral("params"))
                 .toMap()
                 .value(QStringLiteral("0"))
                 .toDouble(),
             0.5);
    QCOMPARE(gainDocument.value(QStringLiteral("keyframes"))
                 .toMap()
                 .value(QStringLiteral("0"))
                 .toMap()
                 .value(QStringLiteral("start"))
                 .toMap()
                 .value(QStringLiteral("value"))
                 .toDouble(),
             0.5);
    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->audioPlugins.first().params.value(QStringLiteral("0")).toDouble(), 0.5);
    QCOMPARE(clip->audioPlugins.first()
                 .keyframeTracks.value(QStringLiteral("0"))
                 .toMap()
                 .value(QStringLiteral("start"))
                 .toMap()
                 .value(QStringLiteral("value"))
                 .toDouble(),
             0.5);
    const QVariantMap afterParameterUpdate = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterParameterUpdate);
    timeline.undo();

    timeline.reorderAudioPlugins(clipId, 1, 0);
    QCOMPARE(pluginIds(),
             QStringList({QStringLiteral("test.delay"), QStringLiteral("test.gain")}));
    changedPlugins = pluginDocuments();
    QCOMPARE(changedPlugins.at(0)
                 .toMap()
                 .value(QStringLiteral("workflowPluginExtension"))
                 .toString(),
             QStringLiteral("test.delay-token"));
    QCOMPARE(changedPlugins.at(1)
                 .toMap()
                 .value(QStringLiteral("workflowPluginExtension"))
                 .toString(),
             QStringLiteral("test.gain-token"));
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);

    timeline.removeAudioPlugin(clipId, 0, gain.id);
    QCOMPARE(pluginIds(), QStringList({QStringLiteral("test.delay")}));
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    QCOMPARE(pluginIds(),
             QStringList({QStringLiteral("test.gain"), QStringLiteral("test.delay")}));

    timeline.setAudioPluginKeyframe(
        clipId, 0, QStringLiteral("0"), 6, 0.75,
        {{QStringLiteral("interp"), QStringLiteral("linear")}});
    gainDocument = pluginDocuments().first().toMap();
    QVariantList points = gainDocument.value(QStringLiteral("keyframes"))
                              .toMap()
                              .value(QStringLiteral("0"))
                              .toMap()
                              .value(QStringLiteral("points"))
                              .toList();
    QVERIFY(containsFrame(points, 6));
    QCOMPARE(gainDocument.value(QStringLiteral("workflowPluginExtension")).toString(),
             QStringLiteral("test.gain-token"));
    const QVariantMap afterKeyframeSet = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeSet);

    timeline.moveAudioPluginKeyframe(clipId, 0, QStringLiteral("0"), 6, 8);
    points = pluginDocuments()
                 .first()
                 .toMap()
                 .value(QStringLiteral("keyframes"))
                 .toMap()
                 .value(QStringLiteral("0"))
                 .toMap()
                 .value(QStringLiteral("points"))
                 .toList();
    QVERIFY(!containsFrame(points, 6));
    QVERIFY(containsFrame(points, 8));
    const QVariantMap afterKeyframeMove = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeSet);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeMove);

    timeline.removeAudioPluginKeyframe(clipId, 0, QStringLiteral("0"), 8);
    points = pluginDocuments()
                 .first()
                 .toMap()
                 .value(QStringLiteral("keyframes"))
                 .toMap()
                 .value(QStringLiteral("0"))
                 .toMap()
                 .value(QStringLiteral("points"))
                 .toList();
    QVERIFY(!containsFrame(points, 8));
    const QVariantMap afterKeyframeRemoval = timeline.timelineStateSnapshot();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeMove);
    timeline.redo();
    QCOMPARE(timeline.timelineStateSnapshot(), afterKeyframeRemoval);
    timeline.undo();
    timeline.undo();
    timeline.undo();
    QCOMPARE(timeline.timelineStateSnapshot(), before);
}

void TestDailyEditingWorkflow::audioParameterDurationUsesRustState() {
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const QString audioPath = directory.filePath(QStringLiteral("duration.wav"));
    QVERIFY(writeSilentWav(audioPath));

    SelectionService selection;
    TimelineService timeline(&selection);
    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("audio"), 0, 0);

    const auto rustClip = [&timeline, clipId]() {
        const QVariantList clips =
            timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
        for (const QVariant &value : clips) {
            const QVariantMap clip = value.toMap();
            if (clip.value(QStringLiteral("id")).toInt() == clipId) {
                return clip;
            }
        }
        return QVariantMap{};
    };

    auto *clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->effects.size(), 1);
    QCOMPARE(clip->effects.first()->id(), QStringLiteral("audio"));
    timeline.updateEffectParam(clipId, 0, QStringLiteral("source"), audioPath);
    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QVERIFY(!rustClip().isEmpty());
    QCOMPARE(clip->durationFrames, rustClip().value(QStringLiteral("duration")).toInt());

    timeline.undoStack()->clear();
    const QVariantMap before = timeline.timelineStateSnapshot();
    const int beforeDuration = clip->durationFrames;
    timeline.updateEffectParam(clipId, 0, QStringLiteral("speed"), 200.0);

    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    const QVariantMap after = timeline.timelineStateSnapshot();
    const int afterDuration = rustClip().value(QStringLiteral("duration")).toInt();
    QVERIFY(afterDuration != beforeDuration);
    QCOMPARE(clip->durationFrames, afterDuration);

    timeline.undo();
    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(timeline.timelineStateSnapshot(), before);
    QCOMPARE(clip->durationFrames, beforeDuration);
    QCOMPARE(clip->durationFrames, rustClip().value(QStringLiteral("duration")).toInt());

    timeline.redo();
    clip = timeline.findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(timeline.timelineStateSnapshot(), after);
    QCOMPARE(clip->durationFrames, afterDuration);
    QCOMPARE(clip->durationFrames, rustClip().value(QStringLiteral("duration")).toInt());
}

void TestDailyEditingWorkflow::targetedBatchFailureRollsBackRustAndQt() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const QVariantMap previousState = timeline.timelineStateSnapshot();
    const QList<SceneData> previousScenes = timeline.getAllScenes();

    ClipData duplicate;
    duplicate.id = 900;
    duplicate.sceneId = 0;
    duplicate.type = QStringLiteral("text");
    duplicate.startFrame = 0;
    duplicate.durationFrames = 30;
    duplicate.layer = 0;
    QSignalSpy clipsChangedSpy(&timeline, &TimelineService::clipsChanged);
    QVERIFY(!timeline.addClipsDirectInternal({duplicate, duplicate}));

    QCOMPARE(timeline.timelineStateSnapshot(), previousState);
    QCOMPARE(timeline.getAllScenes().size(), previousScenes.size());
    QCOMPARE(timeline.getAllScenes().first().clips.size(),
             previousScenes.first().clips.size());
    QCOMPARE(clipsChangedSpy.count(), 0);

    ClipData missingScene = duplicate;
    missingScene.id = 901;
    missingScene.sceneId = 999;
    QVERIFY(!timeline.addClipsDirectInternal({duplicate, missingScene}));
    QCOMPARE(timeline.timelineStateSnapshot(), previousState);
    QCOMPARE(timeline.getAllScenes().first().clips.size(),
             previousScenes.first().clips.size());
    QCOMPARE(clipsChangedSpy.count(), 0);
}

void TestDailyEditingWorkflow::sceneUndoRestoresItsClipsInRustState() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int sceneId = timeline.nextSceneId();
    timeline.createScene(QStringLiteral("Nested"));
    QCOMPARE(timeline.currentSceneId(), sceneId);
    const int clipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 5, 1);
    QVERIFY(timeline.findClipById(clipId) != nullptr);
    const int trailingSceneId = timeline.nextSceneId();
    timeline.createScene(QStringLiteral("Trailing"));
    QCOMPARE(timeline.currentSceneId(), trailingSceneId);
    const QVariantMap beforeRemoval = timeline.timelineStateSnapshot();

    timeline.removeScene(sceneId);
    QVERIFY(timeline.findClipById(clipId) == nullptr);
    timeline.undo();

    QCOMPARE(timeline.timelineStateSnapshot(), beforeRemoval);
    QCOMPARE(timeline.getAllScenes().size(), 3);
    QCOMPARE(timeline.getAllScenes().at(1).id, sceneId);
    QCOMPARE(timeline.getAllScenes().at(2).id, trailingSceneId);
    const auto sceneIt = std::ranges::find_if(
        timeline.getAllScenes(), [sceneId](const SceneData &scene) { return scene.id == sceneId; });
    QVERIFY(sceneIt != timeline.getAllScenes().end());
    QCOMPARE(sceneIt->clips.size(), 1);
    QCOMPARE(sceneIt->clips.first().id, clipId);
}

void TestDailyEditingWorkflow::clipboardPasteTargetsCurrentScene() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int sourceClipId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 0, 0);
    timeline.copyClip(sourceClipId);

    const int destinationSceneId = timeline.nextSceneId();
    timeline.createScene(QStringLiteral("Destination"));
    QCOMPARE(timeline.currentSceneId(), destinationSceneId);
    const int pastedClipId = timeline.nextClipId();
    timeline.pasteClip(10, 2);

    const auto *pasted = timeline.findClipById(pastedClipId);
    QVERIFY(pasted != nullptr);
    QCOMPARE(pasted->sceneId, destinationSceneId);
    QCOMPARE(timeline.clips(destinationSceneId).size(), 1);
    QCOMPARE(timeline.clips(0).size(), 1);

    timeline.undo();
    QVERIFY(timeline.findClipById(pastedClipId) == nullptr);
    timeline.redo();
    pasted = timeline.findClipById(pastedClipId);
    QVERIFY(pasted != nullptr);
    QCOMPARE(pasted->sceneId, destinationSceneId);
    QVariantMap rustClip;
    const QVariantList rustClips =
        timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
    for (const QVariant &candidate : rustClips) {
        if (candidate.toMap().value(QStringLiteral("id")).toInt() == pastedClipId) {
            rustClip = candidate.toMap();
            break;
        }
    }
    QVERIFY(!rustClip.isEmpty());
    QCOMPARE(rustClip.value(QStringLiteral("sceneId")).toInt(), destinationSceneId);
}

void TestDailyEditingWorkflow::audioPluginKeyframeEvaluationIsCompatible() {
    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("audio"), 0, 0);

    auto *clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    AudioPluginState plugin;
    plugin.id = QStringLiteral("test.keyframes");
    plugin.params = {
        {QStringLiteral("0"), 0.0},
        {QStringLiteral("1"), 0},
        {QStringLiteral("2"), false},
        {QStringLiteral("3"), 0.0},
    };
    plugin.keyframeTracks.insert(
        QStringLiteral("0"),
        QVariantList{
            QVariantMap{{QStringLiteral("frame"), 20}, {QStringLiteral("value"), 20.0}},
            QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 0.0}},
            QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), 10.0}},
        });
    plugin.keyframeTracks.insert(
        QStringLiteral("1"),
        QVariantMap{
            {QStringLiteral("start"), QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 1}}},
            {QStringLiteral("points"), QVariantList{
                 QVariantMap{{QStringLiteral("frame"), 20}, {QStringLiteral("value"), 9}},
                 QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), 5}},
             }},
        });
    plugin.keyframeTracks.insert(
        QStringLiteral("2"),
        QVariantList{
            QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), false}},
            QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), true}},
        });
    plugin.keyframeTracks.insert(
        QStringLiteral("3"),
        QVariantList{
            QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 2.0},
                        {QStringLiteral("interp"), QStringLiteral("none")}},
            QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), 8.0},
                        {QStringLiteral("interp"), QStringLiteral("linear")}},
        });
    clip->audioPlugins.append(plugin);

    const QVariantList sorted = controller.audioPluginKeyframeListForUi(clipId, 0, QStringLiteral("0"));
    QCOMPARE(sorted.size(), 3);
    QCOMPARE(sorted.at(0).toMap().value(QStringLiteral("frame")).toInt(), 0);
    QCOMPARE(sorted.at(2).toMap().value(QStringLiteral("frame")).toInt(), 20);
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("0"), -1).toDouble(), 0.0);
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("0"), 15).toDouble(), 15.0);
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("0"), 25).toDouble(), 20.0);
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("1"), 4).toInt(), 1);
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("1"), 5).toInt(), 5);
    QVERIFY(!controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("2"), 4).toBool());
    QVERIFY(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("2"), 5).toBool());
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("3"), 5).toDouble(), 2.0);
    QCOMPARE(controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("3"), 10).toDouble(), 8.0);

    QVariantList largeTrack;
    largeTrack.reserve(10'000);
    for (int frame = 0; frame < 10'000; ++frame) {
        largeTrack.append(QVariantMap{{QStringLiteral("frame"), frame}, {QStringLiteral("value"), frame * 0.5}});
    }
    clip->audioPlugins[0].keyframeTracks.insert(QStringLiteral("0"), largeTrack);
    clip->audioPlugins[0].invalidateKeyframeCache();
    QElapsedTimer timer;
    timer.start();
    double checksum = 0.0;
    for (int i = 0; i < 200; ++i) {
        checksum += controller.audioPluginEvaluatedParam(clipId, 0, QStringLiteral("0"), 9'500 + (i % 400)).toDouble();
    }
    QVERIFY(checksum > 0.0);
    qInfo() << "audio_plugin_keyframes points=10000 evaluations=200 elapsed_ms=" << timer.elapsed();
}

void TestDailyEditingWorkflow::audioPluginKeyframeMutationsAreUndoable() {
    TimelineController controller;
    const int clipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("audio"), 0, 0);

    auto *clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    AudioPluginState plugin;
    plugin.id = QStringLiteral("test.mutations");
    plugin.params.insert(QStringLiteral("0"), 0);
    controller.timeline()->addAudioPlugin(clipId, plugin, plugin.id);
    clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->audioPlugins.size(), 1);
    controller.timeline()->undoStack()->clear();

    const QVariantList customPoints{0.2, 0.1, 0.7, 0.9, 1.0, 1.0};
    const QVariantMap modeParams{{QStringLiteral("stepFrames"), 3}};
    controller.timeline()->setAudioPluginKeyframe(
        clipId, 0, QStringLiteral("0"), 0, 1,
        {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller.timeline()->setAudioPluginKeyframe(
        clipId, 0, QStringLiteral("0"), 10, 100,
        {{QStringLiteral("interp"), QStringLiteral("custom")},
         {QStringLiteral("points"), customPoints},
         {QStringLiteral("modeParams"), modeParams}});

    clip = controller.timeline()->findClipById(clipId);
    QVERIFY(clip != nullptr);
    QCOMPARE(clip->audioPlugins.first().params.value(QStringLiteral("0")).typeId(),
             QMetaType::Int);
    QVariantList points = controller.audioPluginKeyframeListForUi(
        clipId, 0, QStringLiteral("0"));
    QCOMPARE(points.size(), 2);
    QCOMPARE(points.at(1).toMap().value(QStringLiteral("value")).typeId(), QMetaType::Int);
    QCOMPARE(points.at(1).toMap().value(QStringLiteral("points")).toList(), customPoints);

    controller.timeline()->moveAudioPluginKeyframe(clipId, 0, QStringLiteral("0"), 10, 8);
    points = controller.audioPluginKeyframeListForUi(clipId, 0, QStringLiteral("0"));
    QCOMPARE(points.at(1).toMap().value(QStringLiteral("frame")).toInt(), 8);
    controller.timeline()->undo();
    points = controller.audioPluginKeyframeListForUi(clipId, 0, QStringLiteral("0"));
    QCOMPARE(points.at(1).toMap().value(QStringLiteral("frame")).toInt(), 10);
    controller.timeline()->redo();

    controller.timeline()->removeAudioPluginKeyframe(clipId, 0, QStringLiteral("0"), 8);
    QCOMPARE(controller.audioPluginKeyframeListForUi(clipId, 0, QStringLiteral("0")).size(), 1);
    controller.timeline()->undo();
    points = controller.audioPluginKeyframeListForUi(clipId, 0, QStringLiteral("0"));
    QCOMPARE(points.size(), 2);
    QCOMPARE(points.at(1).toMap().value(QStringLiteral("frame")).toInt(), 8);
    QCOMPARE(points.at(1).toMap().value(QStringLiteral("modeParams")).toMap(), modeParams);
}

void TestDailyEditingWorkflow::pasteReportsResolvedClipEditTarget() {
    TimelineController controller;

    const int sourceClipId = controller.timeline()->nextClipId();
    controller.createObject(QStringLiteral("text"), 0, 1);

    const auto *sourceClipPtr = findClip(controller, sourceClipId);
    QVERIFY2(sourceClipPtr != nullptr, qPrintable(QStringLiteral("Missing source text clip %1").arg(sourceClipId)));
    const ClipData sourceClip = controller.timeline()->deepCopyClip(*sourceClipPtr);
    QVERIFY(sourceClip.durationFrames > 0);

    controller.copyClip(sourceClipId);
    const int requestedFrame = sourceClip.startFrame + (sourceClip.durationFrames / 2);
    const int requestedLayer = sourceClip.layer;
    const QVariantMap pasteResult = controller.pasteClip(requestedFrame, requestedLayer);
    QVERIFY(pasteResult.value(QStringLiteral("ok")).toBool());
    QCOMPARE(pasteResult.value(QStringLiteral("frame")).toInt(), sourceClip.startFrame + sourceClip.durationFrames);
    QCOMPARE(pasteResult.value(QStringLiteral("layer")).toInt(), requestedLayer);
    QCOMPARE(pasteResult.value(QStringLiteral("duration")).toInt(), sourceClip.durationFrames);
    QCOMPARE(pasteResult.value(QStringLiteral("nextFrame")).toInt(), sourceClip.startFrame + (sourceClip.durationFrames * 2));

    const int pastedClipId = controller.timeline()->nextClipId() - 1;
    const auto *pastedClipPtr = findClip(controller, pastedClipId);
    QVERIFY2(pastedClipPtr != nullptr, qPrintable(QStringLiteral("Missing pasted text clip %1").arg(pastedClipId)));
    const ClipData pastedClip = controller.timeline()->deepCopyClip(*pastedClipPtr);
    QCOMPARE(pastedClip.startFrame, pasteResult.value(QStringLiteral("frame")).toInt());
    QCOMPARE(pastedClip.layer, requestedLayer);
}

void TestDailyEditingWorkflow::catalogItemsExposeProductMetadata() {
    const QVariantList objects = TimelineController::getAvailableObjects();

    QVariantMap textItem;
    std::function<bool(const QVariantList &)> findText = [&](const QVariantList &nodes) -> bool {
        for (const QVariant &entry : nodes) {
            const QVariantMap node = entry.toMap();
            if (node.value(QStringLiteral("isCategory")).toBool()) {
                if (findText(node.value(QStringLiteral("children")).toList())) {
                    return true;
                }
                continue;
            }
            if (node.value(QStringLiteral("id")).toString() == QStringLiteral("text")) {
                textItem = node;
                return true;
            }
        }
        return false;
    };

    QVERIFY(findText(objects));
    QCOMPARE(textItem.value(QStringLiteral("name")).toString(), QStringLiteral("Text"));
    QCOMPARE(textItem.value(QStringLiteral("kind")).toString(), QStringLiteral("object"));
    QCOMPARE(textItem.value(QStringLiteral("version")).toString(), QStringLiteral("1.0.0"));
    QCOMPARE(textItem.value(QStringLiteral("source")).toString(), QStringLiteral("built-in"));
    QVERIFY(textItem.value(QStringLiteral("categories")).toStringList().contains(QStringLiteral("Text")));
}

void TestDailyEditingWorkflow::catalogQueryFiltersMetadataAndCategories() {
    EffectMetadata packagedGlow;
    packagedGlow.id = QStringLiteral("workflow.packaged-glow");
    packagedGlow.name = QStringLiteral("Workflow Glow");
    packagedGlow.version = QStringLiteral("2.1.0");
    packagedGlow.kind = QStringLiteral("effect");
    packagedGlow.categories = {QStringLiteral("Color/Glow")};
    packagedGlow.source = QStringLiteral("package");
    packagedGlow.packageId = QStringLiteral("workflow.catalog-pack");
    packagedGlow.sourcePath = QStringLiteral("packages/workflow/glow.json");
    EffectRegistry::instance().registerEffect(packagedGlow);

    const QVariantList byName = TimelineController::queryCatalog(QStringLiteral("effect"), QStringLiteral("workflow glow"));
    QCOMPARE(byName.size(), 1);
    QCOMPARE(byName.first().toMap().value(QStringLiteral("id")).toString(), packagedGlow.id);

    const QVariantList byPackage = TimelineController::queryCatalog(QStringLiteral("effect"), QStringLiteral("catalog-pack"));
    QCOMPARE(byPackage.size(), 1);
    QCOMPARE(byPackage.first().toMap().value(QStringLiteral("source")).toString(), QStringLiteral("package"));

    const QVariantList builtIns = TimelineController::queryCatalog(QStringLiteral("effect"), QStringLiteral("built-in"));
    QVERIFY(std::ranges::any_of(builtIns, [](const QVariant &entry) { return entry.toMap().value(QStringLiteral("id")).toString() == QStringLiteral("blur"); }));

    const QVariantList byParentCategory = TimelineController::queryCatalog(QStringLiteral("effect"), QString(), QStringLiteral("Color"));
    QVERIFY(std::ranges::any_of(byParentCategory, [&packagedGlow](const QVariant &entry) { return entry.toMap().value(QStringLiteral("id")).toString() == packagedGlow.id; }));
    QVERIFY(TimelineController::getCatalogCategories(QStringLiteral("effect")).contains(QStringLiteral("Color/Glow")));
    QVERIFY(TimelineController::queryCatalog(QStringLiteral("object"), QStringLiteral("workflow glow")).isEmpty());
}

void TestDailyEditingWorkflow::catalogPickerLoadsAndFilters() {
    TimelineController controller;
    QQmlEngine engine;
    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/common/CatalogPickerDialog.qml")));
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    std::unique_ptr<QObject> picker(component.create());
    QVERIFY2(picker != nullptr, qPrintable(component.errorString()));

    QVERIFY(picker->setProperty("controller", QVariant::fromValue(static_cast<QObject *>(&controller))));
    QVERIFY(picker->setProperty("currentKind", QStringLiteral("effect")));
    QVERIFY(picker->setProperty("searchText", QStringLiteral("blur")));
    QVERIFY(QMetaObject::invokeMethod(picker.get(), "refresh"));

    const QVariantList items = picker->property("catalogItems").toList();
    QCOMPARE(items.size(), 1);
    QCOMPARE(items.first().toMap().value(QStringLiteral("id")).toString(), QStringLiteral("blur"));
}

QTEST_MAIN(TestDailyEditingWorkflow)
#include "test_daily_editing_workflow.moc"
