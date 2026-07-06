#include "effect_registry.hpp"
#include "timeline_controller.hpp"
#include <QColor>
#include <QImage>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestDailyEditingWorkflow : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase() { registerWorkflowEffects(); }

    void saveAndReopenDailyEdit();

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

QTEST_MAIN(TestDailyEditingWorkflow)
#include "test_daily_editing_workflow.moc"
