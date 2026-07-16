#include "bridge/ecs_render_bridge.hpp"
#include "constants.hpp"
#include "effect_registry.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "window_manager.hpp"
#include "workspace.hpp"
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QElapsedTimer>
#include <QSignalSpy>
#include <QTest>
#include <QUrl>
#include <QVector3D>
#include <algorithm>
#include <cmath>

using namespace AviQtl;
using namespace AviQtl::UI;

class TestQmlCompositeCapture : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void capturesCompositeView3DOutput();
    void capturesAnimatedTextAndMonochromeEffect();

  private:
    static QImage grabView3D(QQuickItem *view3D);
    static QImage grabView3DUntilVisible(QQuickItem *view3D, int minimumBrightness = 180, int timeoutMs = 5'000);
    static double brightPixelCenterX(const QImage &image);
    static QVector3D averageVisibleColor(const QImage &image, int minimumBrightness = 20);
};

void TestQmlCompositeCapture::initTestCase() {
    qmlRegisterUncreatableType<TimelineController>("AviQtl.UI", 1, 0, "TimelineController", "Managed by C++");
    qmlRegisterSingletonInstance<ECSRenderBridge>("AviQtl.UI", 1, 0, "ECSRenderBridge", &ECSRenderBridge::instance());

    Core::EffectMetadata transform;
    transform.id = QStringLiteral("transform");
    transform.name = QStringLiteral("Transform");
    transform.version = QStringLiteral("1.0.0");
    transform.kind = QStringLiteral("effect");
    transform.categories = {QStringLiteral("Basic")};
    transform.defaultParams = {{QStringLiteral("x"), 0.0}, {QStringLiteral("y"), 0.0}, {QStringLiteral("z"), 0.0},
                               {QStringLiteral("scale"), 100.0}, {QStringLiteral("opacity"), 1.0}};
    Core::EffectRegistry::instance().registerEffect(transform);

    Core::EffectMetadata text;
    text.id = QStringLiteral("text");
    text.name = QStringLiteral("Text");
    text.version = QStringLiteral("1.0.0");
    text.kind = QStringLiteral("object");
    text.categories = {QStringLiteral("Text")};
    text.qmlSource = QUrl::fromLocalFile(QStringLiteral(AVIQTL_SOURCE_DIR) + QStringLiteral("/ui/qml/objects/TextObject.qml")).toString();
    text.defaultParams = {{QStringLiteral("text"), QStringLiteral("Text")}, {QStringLiteral("fontSize"), 48.0}, {QStringLiteral("color"), QStringLiteral("#ffffff")}};
    Core::EffectRegistry::instance().registerEffect(text);

    Core::EffectMetadata monochrome;
    monochrome.id = QStringLiteral("monochrome");
    monochrome.name = QStringLiteral("Monochrome");
    monochrome.version = QStringLiteral("1.0.0");
    monochrome.kind = QStringLiteral("effect");
    monochrome.categories = {QStringLiteral("Color")};
    // Built-in effects are deployed beside the executable together with their
    // compiled shaders; loading from there exercises the production layout.
    const QString monochromeQmlPath = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("effects/Monochrome.qml"));
    QVERIFY2(QFileInfo::exists(monochromeQmlPath), qPrintable(QStringLiteral("Missing deployed effect: %1").arg(monochromeQmlPath)));
    monochrome.qmlSource = QUrl::fromLocalFile(monochromeQmlPath).toString();
    monochrome.defaultParams = {{QStringLiteral("strength"), 100.0}, {QStringLiteral("color"), QStringLiteral("#ffffff")},
                                {QStringLiteral("preserveLuma"), true}};
    Core::EffectRegistry::instance().registerEffect(monochrome);
}

QImage TestQmlCompositeCapture::grabView3D(QQuickItem *view3D) {
    const QSharedPointer<QQuickItemGrabResult> grab = view3D->grabToImage(QSize(320, 180));
    if (!grab) return {};
    QSignalSpy readySpy(grab.get(), &QQuickItemGrabResult::ready);
    return readySpy.wait(5'000) ? grab->image() : QImage();
}

QImage TestQmlCompositeCapture::grabView3DUntilVisible(QQuickItem *view3D, int minimumBrightness, int timeoutMs) {
    QElapsedTimer timer;
    timer.start();
    QImage image;
    do {
        image = grabView3D(view3D);
        if (!image.isNull() && averageVisibleColor(image, minimumBrightness).x() >= 0.0F)
            return image;
        QTest::qWait(50);
        if (view3D->window())
            view3D->window()->update();
    } while (timer.elapsed() < timeoutMs);
    return image;
}

double TestQmlCompositeCapture::brightPixelCenterX(const QImage &image) {
    double weightedX = 0.0;
    double weight = 0.0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QColor pixel = image.pixelColor(x, y);
            const int brightness = std::max({pixel.red(), pixel.green(), pixel.blue()});
            if (brightness > 180) {
                weightedX += static_cast<double>(x) * brightness;
                weight += brightness;
            }
        }
    }
    return weight > 0.0 ? weightedX / weight : -1.0;
}

QVector3D TestQmlCompositeCapture::averageVisibleColor(const QImage &image, int minimumBrightness) {
    QVector3D sum;
    int count = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QColor pixel = image.pixelColor(x, y);
            if (std::max({pixel.red(), pixel.green(), pixel.blue()}) >= minimumBrightness) {
                sum += QVector3D(pixel.red(), pixel.green(), pixel.blue());
                ++count;
            }
        }
    }
    return count > 0 ? sum / static_cast<float>(count) : QVector3D(-1.0F, -1.0F, -1.0F);
}

void TestQmlCompositeCapture::capturesCompositeView3DOutput() {
    QQmlEngine engine;
    auto *context = engine.rootContext();
    Workspace workspace;
    workspace.newProject();
    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &Core::SettingsManager::instance());
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(&WindowManager::instance()));
    context->setContextProperty(QStringLiteral("ECSRenderBridge"), &ECSRenderBridge::instance());
    context->setContextProperty(QStringLiteral("DefaultWidth"), AviQtl::kDefaultWidth);
    context->setContextProperty(QStringLiteral("DefaultHeight"), AviQtl::kDefaultHeight);
    context->setContextProperty(QStringLiteral("AviQtlAssetUrl"), QString());

    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/CompositeView.qml")));
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    std::unique_ptr<QObject> object(component.create(context));
    QVERIFY2(object != nullptr, qPrintable(component.errorString()));
    auto *compositeView = qobject_cast<QQuickItem *>(object.get());
    QVERIFY(compositeView != nullptr);

    QQuickWindow window;
    window.setGeometry(0, 0, 320, 180);
    compositeView->setParentItem(window.contentItem());
    compositeView->setWidth(320);
    compositeView->setHeight(180);
    compositeView->setProperty("projectWidth", 320);
    compositeView->setProperty("projectHeight", 180);
    compositeView->setProperty("exportMode", true);
    window.show();

    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 5'000);
    QTest::qWait(100);

    auto *view3D = compositeView->property("view3D").value<QQuickItem *>();
    QVERIFY(view3D != nullptr);
    const QSharedPointer<QQuickItemGrabResult> grab = view3D->grabToImage(QSize(320, 180));
    QVERIFY(grab != nullptr);
    QSignalSpy readySpy(grab.get(), &QQuickItemGrabResult::ready);
    QTRY_COMPARE_WITH_TIMEOUT(readySpy.count(), 1, 5'000);

    const QImage image = grab->image();
    QVERIFY(!image.isNull());
    // grabToImage returns physical pixels; preserve a logical-size assertion on HiDPI displays.
    QCOMPARE(image.deviceIndependentSize(), QSizeF(320, 180));
    const QColor center = image.pixelColor(image.width() / 2, image.height() / 2);
    QVERIFY(center.red() <= 20);
    QVERIFY(center.green() <= 20);
    QVERIFY(center.blue() <= 20);
}

void TestQmlCompositeCapture::capturesAnimatedTextAndMonochromeEffect() {
    QQmlEngine engine;
    auto *context = engine.rootContext();
    Workspace workspace;
    workspace.newProject();
    TimelineController *controller = workspace.currentTimeline();
    QVERIFY(controller != nullptr);
    controller->project()->setWidth(320);
    controller->project()->setHeight(180);
    controller->project()->setFps(30.0);

    const int textClipId = controller->timeline()->nextClipId();
    controller->createObject(QStringLiteral("text"), 0, 0);
    controller->updateClipEffectParam(textClipId, 1, QStringLiteral("text"), QStringLiteral("Render"));
    controller->updateClipEffectParam(textClipId, 1, QStringLiteral("fontSize"), 40.0);
    controller->updateClipEffectParam(textClipId, 1, QStringLiteral("color"), QStringLiteral("#ff0000"));
    controller->setKeyframe(textClipId, 0, QStringLiteral("x"), 0, -80.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller->setKeyframe(textClipId, 0, QStringLiteral("x"), 30, 80.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller->addEffect(textClipId, QStringLiteral("monochrome"));
    controller->setEffectEnabled(textClipId, 2, false);
    QCOMPARE(controller->evaluateClipParams(textClipId, 0).value(QStringLiteral("x")).toDouble(), -80.0);
    QCOMPARE(controller->evaluateClipParams(textClipId, 30).value(QStringLiteral("x")).toDouble(), 80.0);

    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &Core::SettingsManager::instance());
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(&WindowManager::instance()));
    context->setContextProperty(QStringLiteral("ECSRenderBridge"), &ECSRenderBridge::instance());
    context->setContextProperty(QStringLiteral("DefaultWidth"), 320);
    context->setContextProperty(QStringLiteral("DefaultHeight"), 180);
    context->setContextProperty(QStringLiteral("AviQtlAssetUrl"), QString());

    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/CompositeView.qml")));
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    std::unique_ptr<QObject> object(component.create(context));
    QVERIFY2(object != nullptr, qPrintable(component.errorString()));
    auto *compositeView = qobject_cast<QQuickItem *>(object.get());
    QVERIFY(compositeView != nullptr);
    compositeView->setProperty("clipModel", controller->getSceneClips(controller->currentSceneId()));

    QQuickWindow window;
    window.setGeometry(0, 0, 320, 180);
    compositeView->setParentItem(window.contentItem());
    compositeView->setSize(QSizeF(320, 180));
    compositeView->setProperty("projectWidth", 320);
    compositeView->setProperty("projectHeight", 180);
    compositeView->setProperty("exportMode", true);
    compositeView->setProperty("sceneId", controller->currentSceneId());
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 5'000);
    // The offscreen platform can expose a window while using the software scene
    // graph, where Qt Quick 3D intentionally renders no View3D content.
    if (!QSGRendererInterface::isApiRhiBased(window.rendererInterface()->graphicsApi()))
        QSKIP("Qt Quick 3D rendering requires an RHI-based graphics API");
    QTest::qWait(200);

    auto *view3D = compositeView->property("view3D").value<QQuickItem *>();
    QVERIFY(view3D != nullptr);
    // CompositeView normally receives this map from its SceneRenderer/MainWindow
    // owner. The standalone fixture must mirror that integration step explicitly.
    const auto syncEcsRenderData = [compositeView] {
        QVariantMap renderData;
        for (const QVariant &state : ECSRenderBridge::instance().renderStates()) {
            const QVariantMap stateMap = state.toMap();
            renderData.insert(QString::number(stateMap.value(QStringLiteral("clipId")).toInt()), stateMap);
        }
        compositeView->setProperty("ecsRenderData", renderData);
        return renderData;
    };
    // Transport starts at frame 0, so seeking directly to 0 would not emit a
    // frame change or rebake the completed fixture state.
    controller->transport()->setCurrentFrame_seek(1);
    controller->transport()->setCurrentFrame_seek(0);
    const QVariantMap firstRenderData = syncEcsRenderData();
    QVERIFY(firstRenderData.contains(QString::number(textClipId)));
    QCOMPARE(firstRenderData.value(QString::number(textClipId)).toMap().value(QStringLiteral("x")).toDouble(), -80.0);
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("currentFrame").toInt(), 0, 5'000);
    window.update();
    QTest::qWait(150);
    const QImage firstFrame = grabView3DUntilVisible(view3D);
    QSignalSpy frameSpy(controller->transport(), &TransportService::currentFrameChanged);
    controller->transport()->setCurrentFrame_seek(30);
    const QVariantMap lastRenderData = syncEcsRenderData();
    QCOMPARE(lastRenderData.value(QString::number(textClipId)).toMap().value(QStringLiteral("x")).toDouble(), 80.0);
    QTRY_COMPARE_WITH_TIMEOUT(frameSpy.count(), 1, 5'000);
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("currentFrame").toInt(), 30, 5'000);
    window.update();
    QTest::qWait(250);
    const QImage lastFrame = grabView3DUntilVisible(view3D);

    QVERIFY(!firstFrame.isNull());
    QVERIFY(!lastFrame.isNull());
    const double firstCenterX = brightPixelCenterX(firstFrame);
    const double lastCenterX = brightPixelCenterX(lastFrame);
    QVERIFY(firstCenterX >= 0.0);
    QVERIFY(lastCenterX >= 0.0);
    QVERIFY2(lastCenterX > firstCenterX + 40.0, qPrintable(QStringLiteral("text center did not move: frame 0=%1, frame 30=%2").arg(firstCenterX).arg(lastCenterX)));

    const QVector3D colorBeforeEffect = averageVisibleColor(lastFrame);
    const QString beforeMessage = QStringLiteral("unexpected source color: r=%1 g=%2 b=%3").arg(colorBeforeEffect.x()).arg(colorBeforeEffect.y()).arg(colorBeforeEffect.z());
    QVERIFY2(colorBeforeEffect.x() > colorBeforeEffect.y() + 80.0F, qPrintable(beforeMessage));
    QVERIFY2(colorBeforeEffect.x() > colorBeforeEffect.z() + 80.0F, qPrintable(beforeMessage));

    controller->setEffectEnabled(textClipId, 2, true);
    controller->transport()->setCurrentFrame_seek(29);
    controller->transport()->setCurrentFrame_seek(30);
    syncEcsRenderData();
    window.update();
    QTest::qWait(250);
    const QImage monochromeFrame = grabView3DUntilVisible(view3D, 20);
    QVERIFY(!monochromeFrame.isNull());
    const QVector3D colorAfterEffect = averageVisibleColor(monochromeFrame);
    const QString afterMessage = QStringLiteral("monochrome channels differ: r=%1 g=%2 b=%3").arg(colorAfterEffect.x()).arg(colorAfterEffect.y()).arg(colorAfterEffect.z());
    QVERIFY(colorAfterEffect.x() >= 0.0F);
    QVERIFY2(std::abs(colorAfterEffect.x() - colorAfterEffect.y()) < 8.0F, qPrintable(afterMessage));
    QVERIFY2(std::abs(colorAfterEffect.x() - colorAfterEffect.z()) < 8.0F, qPrintable(afterMessage));
}

QTEST_MAIN(TestQmlCompositeCapture)
#include "test_qml_composite_capture.moc"
