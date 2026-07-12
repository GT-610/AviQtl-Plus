#include "bridge/ecs_render_bridge.hpp"
#include "constants.hpp"
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
#include <QSignalSpy>
#include <QTest>

using namespace AviQtl;
using namespace AviQtl::UI;

class TestQmlCompositeCapture : public QObject {
    Q_OBJECT

  private slots:
    void capturesCompositeView3DOutput();
};

void TestQmlCompositeCapture::capturesCompositeView3DOutput() {
    qmlRegisterUncreatableType<TimelineController>("AviQtl.UI", 1, 0, "TimelineController", "Managed by C++");
    qmlRegisterSingletonInstance<ECSRenderBridge>("AviQtl.UI", 1, 0, "ECSRenderBridge", &ECSRenderBridge::instance());

    QQmlEngine engine;
    auto *context = engine.rootContext();
    Workspace workspace;
    workspace.newProject();
    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &Core::SettingsManager::instance());
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(&WindowManager::instance()));
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

QTEST_MAIN(TestQmlCompositeCapture)
#include "test_qml_composite_capture.moc"
