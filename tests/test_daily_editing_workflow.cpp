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
    void rejectedProjectionTransactionRestoresRuntimeModel();
    void projectionSynchronizationFailureRollsBackState();
    void audioParameterDurationUsesRustState();
    void targetedBatchFailureRollsBackRustAndQt();
    void rustFirstStructuralMutationsStayAtomic();
    void sceneUndoRestoresItsClipsInRustState();
    void pasteReportsResolvedClipEditTarget();
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
    auto *firstDecoder = qobject_cast<ImageDecoder *>(controller.mediaManager()->decoderForClip(clipId));
    QVERIFY(firstDecoder != nullptr);
    QTRY_VERIFY(firstDecoder->isReady());
    QCOMPARE(controller.mediaManager()->findChildren<ImageDecoder *>().size(), 1);
    QCOMPARE(qobject_cast<ImageDecoder *>(controller.mediaManager()->decoderForClip(clipId))->source(),
             QUrl::fromLocalFile(firstPath));

    controller.requestImageLoad(clipId, secondPath);
    QTRY_COMPARE(controller.mediaManager()->findChildren<ImageDecoder *>().size(), 1);
    auto *replacement = qobject_cast<ImageDecoder *>(controller.mediaManager()->decoderForClip(clipId));
    QVERIFY(replacement != nullptr);
    QCOMPARE(replacement->source(), QUrl::fromLocalFile(secondPath));
    QTRY_VERIFY(replacement->isReady());

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

void TestDailyEditingWorkflow::rustFirstStructuralMutationsStayAtomic() {
    SelectionService selection;
    TimelineService timeline(&selection);
    const int existingId = timeline.nextClipId();
    timeline.createClip(QStringLiteral("text"), 0, 0);
    timeline.undoStack()->clear();

    auto *existing = timeline.findClipById(existingId);
    QVERIFY(existing != nullptr);
    QVERIFY(existing->effects.size() >= 2);
    auto *detached = existing->effects.takeLast();
    const QVariantMap beforeRejectedInsertion = timeline.timelineStateSnapshot();

    ClipData rejected;
    rejected.id = 900;
    rejected.sceneId = 999;
    rejected.type = QStringLiteral("test");
    rejected.startFrame = 100;
    rejected.durationFrames = 30;
    rejected.layer = 1;
    QSignalSpy clipsChangedSpy(&timeline, &TimelineService::clipsChanged);
    QVERIFY(!timeline.addClipDirectInternal(rejected));
    QCOMPARE(timeline.timelineStateSnapshot(), beforeRejectedInsertion);
    QVERIFY(timeline.findClipById(rejected.id) == nullptr);
    QCOMPARE(clipsChangedSpy.count(), 0);
    existing = timeline.findClipById(existingId);
    QVERIFY(existing != nullptr);
    existing->effects.append(detached);

    ClipData first = rejected;
    first.id = 901;
    first.sceneId = 0;
    ClipData second = first;
    second.id = 902;
    second.startFrame = 140;
    QVERIFY(timeline.addClipsDirectInternal({first, second}));
    QCOMPARE(clipsChangedSpy.count(), 1);
    QCOMPARE(timeline.clips().at(timeline.clips().size() - 2).id, first.id);
    QCOMPARE(timeline.clips().last().id, second.id);

    const QVariantList rustClips =
        timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
    QCOMPARE(rustClips.at(rustClips.size() - 2).toMap().value(QStringLiteral("id")).toInt(),
             first.id);
    QCOMPARE(rustClips.last().toMap().value(QStringLiteral("id")).toInt(), second.id);

    const qsizetype effectObjectCount = timeline.findChildren<EffectModel *>().size();
    const QVariantMap beforeRejectedSplit = timeline.timelineStateSnapshot();
    QVERIFY(!timeline.splitClipInternal(existingId, 30, existingId));
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QCOMPARE(timeline.timelineStateSnapshot(), beforeRejectedSplit);
    QCOMPARE(timeline.findChildren<EffectModel *>().size(), effectObjectCount);

    constexpr int splitId = 903;
    QVERIFY(timeline.splitClipInternal(existingId, 30, splitId));
    QCOMPARE(timeline.clips().at(1).id, splitId);
    const QVariantList splitRustClips =
        timeline.timelineStateSnapshot().value(QStringLiteral("clips")).toList();
    QCOMPARE(splitRustClips.at(1).toMap().value(QStringLiteral("id")).toInt(), splitId);

    const qsizetype sceneCount = timeline.getAllScenes().size();
    QSignalSpy scenesChangedSpy(&timeline, &TimelineService::scenesChanged);
    timeline.createSceneInternal(0, QStringLiteral("Duplicate root"));
    QCOMPARE(timeline.getAllScenes().size(), sceneCount);
    QCOMPARE(scenesChangedSpy.count(), 0);
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
