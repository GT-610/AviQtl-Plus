#include "bridge/ecs_render_bridge.hpp"
#include "constants.hpp"
#include "effect_registry.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_export_manager.hpp"
#include "window_manager.hpp"
#include "workspace.hpp"
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QFile>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QElapsedTimer>
#include <QSignalSpy>
#include <QTest>
#include <QTemporaryDir>
#include <QUrl>
#include <QVector3D>
#include <algorithm>
#include <cmath>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/error.h>
#include <libswscale/swscale.h>
}

using namespace AviQtl;
using namespace AviQtl::UI;

class TestQmlCompositeCapture : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void capturesCompositeView3DOutput();
    void capturesAnimatedTextAndMonochromeEffect();
    void exportsAnimatedTextAndDecodesDistinctFrames();

  private:
    struct DecodedVideo {
        QString error;
        QVector<QImage> frames;
        int width = 0;
        int height = 0;
        double fps = 0.0;
        bool hasAudio = false;
    };

    static QImage grabView3D(QQuickItem *view3D);
    static QImage grabView3DUntilVisible(QQuickItem *view3D, int minimumBrightness = 180, int timeoutMs = 5'000);
    static double brightPixelCenterX(const QImage &image);
    static QVector3D averageVisibleColor(const QImage &image, int minimumBrightness = 20);
    static QVariantMap syncEcsRenderData(QQuickItem *compositeView);
    static DecodedVideo decodeVideo(const QString &path);
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

QVariantMap TestQmlCompositeCapture::syncEcsRenderData(QQuickItem *compositeView) {
    QVariantMap renderData;
    for (const QVariant &state : ECSRenderBridge::instance().renderStates()) {
        const QVariantMap stateMap = state.toMap();
        renderData.insert(QString::number(stateMap.value(QStringLiteral("clipId")).toInt()), stateMap);
    }
    compositeView->setProperty("ecsRenderData", renderData);
    return renderData;
}

TestQmlCompositeCapture::DecodedVideo TestQmlCompositeCapture::decodeVideo(const QString &path) {
    DecodedVideo result;
    AVFormatContext *formatContext = nullptr;
    if (avformat_open_input(&formatContext, path.toUtf8().constData(), nullptr, nullptr) < 0) {
        result.error = QStringLiteral("failed to open encoded output");
        return result;
    }

    if (avformat_find_stream_info(formatContext, nullptr) < 0) {
        result.error = QStringLiteral("failed to read encoded stream information");
        avformat_close_input(&formatContext);
        return result;
    }

    const int videoStreamIndex = av_find_best_stream(formatContext, AVMEDIA_TYPE_VIDEO, -1, -1, nullptr, 0);
    if (videoStreamIndex < 0) {
        result.error = QStringLiteral("encoded output has no video stream");
        avformat_close_input(&formatContext);
        return result;
    }
    for (unsigned int i = 0; i < formatContext->nb_streams; ++i) {
        if (formatContext->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_AUDIO) {
            result.hasAudio = true;
            break;
        }
    }

    AVStream *videoStream = formatContext->streams[videoStreamIndex];
    result.width = videoStream->codecpar->width;
    result.height = videoStream->codecpar->height;
    result.fps = av_q2d(videoStream->avg_frame_rate);

    const AVCodec *codec = avcodec_find_decoder(videoStream->codecpar->codec_id);
    AVCodecContext *codecContext = codec ? avcodec_alloc_context3(codec) : nullptr;
    if (!codecContext || avcodec_parameters_to_context(codecContext, videoStream->codecpar) < 0 || avcodec_open2(codecContext, codec, nullptr) < 0) {
        result.error = QStringLiteral("failed to initialize video decoder");
        avcodec_free_context(&codecContext);
        avformat_close_input(&formatContext);
        return result;
    }

    AVPacket *packet = av_packet_alloc();
    AVFrame *frame = av_frame_alloc();
    SwsContext *scaleContext = nullptr;
    if (!packet || !frame) {
        result.error = QStringLiteral("failed to allocate decode buffers");
    }

    const auto receiveFrames = [&]() -> bool {
        while (result.error.isEmpty()) {
            const int receiveResult = avcodec_receive_frame(codecContext, frame);
            if (receiveResult == AVERROR(EAGAIN) || receiveResult == AVERROR_EOF)
                return true;
            if (receiveResult < 0) {
                result.error = QStringLiteral("failed to decode video frame");
                return false;
            }

            QImage image(frame->width, frame->height, QImage::Format_RGBA8888);
            scaleContext = sws_getCachedContext(scaleContext, frame->width, frame->height, static_cast<AVPixelFormat>(frame->format), frame->width, frame->height,
                                                AV_PIX_FMT_RGBA, SWS_BILINEAR, nullptr, nullptr, nullptr);
            if (!scaleContext || image.isNull()) {
                result.error = QStringLiteral("failed to convert decoded video frame");
                return false;
            }
            uint8_t *destinationData[] = {image.bits(), nullptr, nullptr, nullptr};
            const int destinationLines[] = {static_cast<int>(image.bytesPerLine()), 0, 0, 0};
            sws_scale(scaleContext, frame->data, frame->linesize, 0, frame->height, destinationData, destinationLines);
            result.frames.append(image);
            av_frame_unref(frame);
        }
        return false;
    };

    int readResult = 0;
    while (result.error.isEmpty() && (readResult = av_read_frame(formatContext, packet)) >= 0) {
        if (packet->stream_index == videoStreamIndex && avcodec_send_packet(codecContext, packet) < 0) {
            result.error = QStringLiteral("failed to submit encoded video packet");
        } else if (packet->stream_index == videoStreamIndex) {
            receiveFrames();
        }
        av_packet_unref(packet);
    }
    if (result.error.isEmpty() && readResult != AVERROR_EOF) {
        result.error = QStringLiteral("failed to read encoded video packet");
    }
    if (result.error.isEmpty()) {
        if (avcodec_send_packet(codecContext, nullptr) < 0)
            result.error = QStringLiteral("failed to flush video decoder");
        else
            receiveFrames();
    }

    sws_freeContext(scaleContext);
    av_frame_free(&frame);
    av_packet_free(&packet);
    avcodec_free_context(&codecContext);
    avformat_close_input(&formatContext);
    return result;
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
    // Transport starts at frame 0, so seeking directly to 0 would not emit a
    // frame change or rebake the completed fixture state.
    controller->transport()->setCurrentFrame_seek(1);
    controller->transport()->setCurrentFrame_seek(0);
    const QVariantMap firstRenderData = syncEcsRenderData(compositeView);
    QVERIFY(firstRenderData.contains(QString::number(textClipId)));
    QCOMPARE(firstRenderData.value(QString::number(textClipId)).toMap().value(QStringLiteral("x")).toDouble(), -80.0);
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("currentFrame").toInt(), 0, 5'000);
    window.update();
    QTest::qWait(150);
    const QImage firstFrame = grabView3DUntilVisible(view3D);
    QSignalSpy frameSpy(controller->transport(), &TransportService::currentFrameChanged);
    controller->transport()->setCurrentFrame_seek(30);
    const QVariantMap lastRenderData = syncEcsRenderData(compositeView);
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
    syncEcsRenderData(compositeView);
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

void TestQmlCompositeCapture::exportsAnimatedTextAndDecodesDistinctFrames() {
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
    controller->updateClipEffectParam(textClipId, 1, QStringLiteral("text"), QStringLiteral("Export"));
    controller->updateClipEffectParam(textClipId, 1, QStringLiteral("fontSize"), 40.0);
    controller->setKeyframe(textClipId, 0, QStringLiteral("x"), 0, -70.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller->setKeyframe(textClipId, 0, QStringLiteral("x"), 1, 70.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});

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
    compositeView->setProperty("sceneId", controller->currentSceneId());
    window.show();
    QTRY_VERIFY_WITH_TIMEOUT(window.isExposed(), 5'000);
    if (!QSGRendererInterface::isApiRhiBased(window.rendererInterface()->graphicsApi()))
        QSKIP("Qt Quick 3D rendering requires an RHI-based graphics API");

    controller->setCompositeView(compositeView);
    const QMetaObject::Connection ecsConnection = connect(&ECSRenderBridge::instance(), &ECSRenderBridge::renderStatesChanged, compositeView,
                                                          [compositeView]() { syncEcsRenderData(compositeView); });
    QVERIFY(ecsConnection);
    controller->transport()->setCurrentFrame_seek(1);
    controller->transport()->setCurrentFrame_seek(0);
    syncEcsRenderData(compositeView);
    window.update();
    auto *view3D = compositeView->property("view3D").value<QQuickItem *>();
    QVERIFY(view3D != nullptr);
    QVERIFY(brightPixelCenterX(grabView3DUntilVisible(view3D)) >= 0.0);

    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString outputPath = dir.filePath(QStringLiteral("animated-text.mp4"));
    Core::VideoEncoder::Config config;
    config.width = 320;
    config.height = 180;
    config.fps_num = 30;
    config.fps_den = 1;
    config.crf = 18;
    config.codecName = QStringLiteral("libx264");
    config.audioCodecName = QStringLiteral("aac");
    config.outputUrl = outputPath;
    config.startFrame = 0;
    config.endFrame = 2;
    config.preset = QStringLiteral("ultrafast");

    TimelineExportManager exportManager(controller);
    QSignalSpy exportSpy(&exportManager, &TimelineExportManager::exportFinished);
    QVERIFY(exportManager.exportVideoAsync(config));
    QTRY_COMPARE_WITH_TIMEOUT(exportSpy.count(), 1, 20'000);
    QTRY_VERIFY_WITH_TIMEOUT(!exportManager.isExporting(), 20'000);
    const QList<QVariant> exportResult = exportSpy.takeFirst();
    QVERIFY2(exportResult.at(0).toBool(), qPrintable(exportResult.at(1).toString()));
    QCOMPARE(exportResult.at(1).toString(), QStringLiteral("Export complete"));
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("exportMode").toBool(), false, 5'000);
    QVERIFY(QFileInfo::exists(outputPath));
    QVERIFY(QFile(outputPath).size() > 0);

    const DecodedVideo decoded = decodeVideo(outputPath);
    QVERIFY2(decoded.error.isEmpty(), qPrintable(decoded.error));
    QCOMPARE(decoded.width, 320);
    QCOMPARE(decoded.height, 180);
    QVERIFY(std::abs(decoded.fps - 30.0) < 0.01);
    QVERIFY(decoded.hasAudio);
    QCOMPARE(decoded.frames.size(), 2);
    const double firstCenterX = brightPixelCenterX(decoded.frames.first());
    const double lastCenterX = brightPixelCenterX(decoded.frames.last());
    QVERIFY(firstCenterX >= 0.0);
    QVERIFY(lastCenterX >= 0.0);
    QVERIFY2(lastCenterX > firstCenterX + 40.0,
             qPrintable(QStringLiteral("decoded text center did not move: frame 0=%1, frame 1=%2").arg(firstCenterX).arg(lastCenterX)));
}

QTEST_MAIN(TestQmlCompositeCapture)
#include "test_qml_composite_capture.moc"
