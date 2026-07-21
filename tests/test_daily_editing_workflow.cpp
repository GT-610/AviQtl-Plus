#include "effect_registry.hpp"
#include "timeline_controller.hpp"
#include <QColor>
#include <QImage>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QTemporaryDir>
#include <QTest>
#include <QUrl>
#include <algorithm>
#include <functional>
#include <memory>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestDailyEditingWorkflow : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase() { registerWorkflowEffects(); }

    void saveAndReopenDailyEdit();
    void mediaImportIsUndoable();
    void audioPluginStateSurvivesClipCopies();
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
    plugin.keyframeTracks = {{QStringLiteral("2"), QVariantMap{{QStringLiteral("points"), QVariantList{QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 0.25}}}}}}};
    source->audioPlugins.append(plugin);
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

    controller.timeline()->undo();
    QVERIFY(findClip(controller, splitId) == nullptr);
    controller.timeline()->redo();
    split = findClip(controller, splitId);
    QVERIFY(split != nullptr);
    QCOMPARE(split->audioPlugins.size(), 1);
    QCOMPARE(split->audioPlugins.first().keyframeTracks, plugin.keyframeTracks);
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
