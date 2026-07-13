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
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QElapsedTimer>
#include <QSignalSpy>
#include <QTest>
#include <QUrl>
#include <algorithm>

using namespace AviQtl;
using namespace AviQtl::UI;

class TestQmlCompositeCapture : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void capturesCompositeView3DOutput();
    void capturesAnimatedTextAtDistinctPositions();

  private:
    static QImage grabView3D(QQuickItem *view3D);
    static QImage grabView3DUntilBright(QQuickItem *view3D, int timeoutMs = 5'000);
    static double brightPixelCenterX(const QImage &image);
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
}

QImage TestQmlCompositeCapture::grabView3D(QQuickItem *view3D) {
    const QSharedPointer<QQuickItemGrabResult> grab = view3D->grabToImage(QSize(320, 180));
    if (!grab) return {};
    QSignalSpy readySpy(grab.get(), &QQuickItemGrabResult::ready);
    return readySpy.wait(5'000) ? grab->image() : QImage();
}

QImage TestQmlCompositeCapture::grabView3DUntilBright(QQuickItem *view3D, int timeoutMs) {
    QElapsedTimer timer;
    timer.start();
    QImage image;
    do {
        image = grabView3D(view3D);
        if (!image.isNull() && brightPixelCenterX(image) >= 0.0)
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

void TestQmlCompositeCapture::capturesAnimatedTextAtDistinctPositions() {
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
    controller->setKeyframe(textClipId, 0, QStringLiteral("x"), 0, -80.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller->setKeyframe(textClipId, 0, QStringLiteral("x"), 30, 80.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
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
    const QImage firstFrame = grabView3DUntilBright(view3D);
    QSignalSpy frameSpy(controller->transport(), &TransportService::currentFrameChanged);
    controller->transport()->setCurrentFrame_seek(30);
    const QVariantMap lastRenderData = syncEcsRenderData();
    QCOMPARE(lastRenderData.value(QString::number(textClipId)).toMap().value(QStringLiteral("x")).toDouble(), 80.0);
    QTRY_COMPARE_WITH_TIMEOUT(frameSpy.count(), 1, 5'000);
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("currentFrame").toInt(), 30, 5'000);
    window.update();
    QTest::qWait(250);
    const QImage lastFrame = grabView3DUntilBright(view3D);

    QVERIFY(!firstFrame.isNull());
    QVERIFY(!lastFrame.isNull());
    const double firstCenterX = brightPixelCenterX(firstFrame);
    const double lastCenterX = brightPixelCenterX(lastFrame);
    QVERIFY(firstCenterX >= 0.0);
    QVERIFY(lastCenterX >= 0.0);
    QVERIFY2(lastCenterX > firstCenterX + 40.0, qPrintable(QStringLiteral("text center did not move: frame 0=%1, frame 30=%2").arg(firstCenterX).arg(lastCenterX)));
}

QTEST_MAIN(TestQmlCompositeCapture)
#include "test_qml_composite_capture.moc"
