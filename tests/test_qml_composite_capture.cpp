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
#include <QRegularExpression>
#include <QScopeGuard>
#include <QSGRendererInterface>
#include <QElapsedTimer>
#include <QSignalSpy>
#include <QTest>
#include <QTemporaryDir>
#include <QUrl>
#include <QVector3D>
#include <algorithm>
#include <cmath>
#include <functional>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/error.h>
#include <libswscale/swscale.h>
}

using namespace AviQtl;
using namespace AviQtl::UI;

class TestSettingsManager : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap settings READ settings CONSTANT)

  public:
    explicit TestSettingsManager(QObject *parent = nullptr) : QObject(parent) {
        m_settings.insert(QStringLiteral("shortcuts"), QVariantMap{});
        m_settings.insert(QStringLiteral("showConfirmOnClose"), true);
        m_settings.insert(QStringLiteral("timelineHeaderHeight"), 28);
        m_settings.insert(QStringLiteral("previewRenderScale"), 1.0);
        m_settings.insert(QStringLiteral("previewMsaaSamples"), 0);
    }

    QVariantMap settings() const { return m_settings; }
    Q_INVOKABLE QVariant value(const QString &key, const QVariant &fallback = {}) const { return m_settings.value(key, fallback); }

  private:
    QVariantMap m_settings;
};

class TestWindowManager : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool timelineVisible READ timelineVisible WRITE setTimelineVisible NOTIFY timelineVisibleChanged)
    Q_PROPERTY(bool projectSettingsVisible READ projectSettingsVisible WRITE setProjectSettingsVisible NOTIFY projectSettingsVisibleChanged)
    Q_PROPERTY(bool objectSettingsVisible READ objectSettingsVisible WRITE setObjectSettingsVisible NOTIFY objectSettingsVisibleChanged)
    Q_PROPERTY(bool systemSettingsVisible READ systemSettingsVisible WRITE setSystemSettingsVisible NOTIFY systemSettingsVisibleChanged)

  public:
    using QObject::QObject;

    bool timelineVisible() const { return m_timelineVisible; }
    bool projectSettingsVisible() const { return m_projectSettingsVisible; }
    bool objectSettingsVisible() const { return m_objectSettingsVisible; }
    bool systemSettingsVisible() const { return m_systemSettingsVisible; }
    int quitRequests() const { return m_quitRequests; }

    Q_INVOKABLE QObject *getWindow(const QString &) const { return nullptr; }
    Q_INVOKABLE void showLauncher() {}
    Q_INVOKABLE void requestQuit() { ++m_quitRequests; }

    void setTimelineVisible(bool value) {
        if (m_timelineVisible == value)
            return;
        m_timelineVisible = value;
        emit timelineVisibleChanged();
    }
    void setProjectSettingsVisible(bool value) {
        if (m_projectSettingsVisible == value)
            return;
        m_projectSettingsVisible = value;
        emit projectSettingsVisibleChanged();
    }
    void setObjectSettingsVisible(bool value) {
        if (m_objectSettingsVisible == value)
            return;
        m_objectSettingsVisible = value;
        emit objectSettingsVisibleChanged();
    }
    void setSystemSettingsVisible(bool value) {
        if (m_systemSettingsVisible == value)
            return;
        m_systemSettingsVisible = value;
        emit systemSettingsVisibleChanged();
    }

  signals:
    void timelineVisibleChanged();
    void projectSettingsVisibleChanged();
    void objectSettingsVisibleChanged();
    void systemSettingsVisibleChanged();

  private:
    bool m_timelineVisible = false;
    bool m_projectSettingsVisible = false;
    bool m_objectSettingsVisible = false;
    bool m_systemSettingsVisible = false;
    int m_quitRequests = 0;
};

class TestQmlCompositeCapture : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void cleanup();
    void loadsDeployedQmlAssetsWithoutRuntimeErrors();
    void previewQualitySettingsUpdateViewGeometry();
    void capturesCompositeView3DOutput();
    void capturesAnimatedTextAndMonochromeEffect();
    void exportsDeployedRectangleAndDecodesDistinctFrames();
    void discardingUnsavedProjectsCompletesApplicationQuit();
    void savingUnsavedProjectCompletesApplicationQuit();

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
    static QImage grabView3DUntil(QQuickItem *view3D, const std::function<bool(const QImage &)> &predicate, int timeoutMs = 5'000);
    static QImage grabView3DUntilVisible(QQuickItem *view3D, int minimumBrightness = 180, int timeoutMs = 5'000);
    static double brightPixelCenterX(const QImage &image);
    static QVector3D averageVisibleColor(const QImage &image, int minimumBrightness = 20);
    static QVariantMap syncEcsRenderData(QQuickItem *compositeView);
    static DecodedVideo decodeVideo(const QString &path);
    static QObject *findSaveConfirmDialog(QObject *root);
    static QObject *findSaveDialog(QObject *root);

    std::unique_ptr<QQmlEngine> m_engine;
};

namespace {
std::unique_ptr<QObject> createMainWindow(QQmlEngine &engine, Workspace &workspace, TestSettingsManager &settings, TestWindowManager &windowManager, QString *error) {
    QQmlContext *context = engine.rootContext();
    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &settings);
    context->setContextProperty(QStringLiteral("WindowManager"), &windowManager);
    context->setContextProperty(QStringLiteral("DefaultWidth"), AviQtl::kDefaultWidth);
    context->setContextProperty(QStringLiteral("DefaultHeight"), AviQtl::kDefaultHeight);
    context->setContextProperty(QStringLiteral("DefaultTotalFrames"), 1'800);
    context->setContextProperty(QStringLiteral("DefaultFps"), 60.0);
    context->setContextProperty(QStringLiteral("AviQtlAssetUrl"), QString());

    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/MainWindow.qml")));
    if (!component.isReady()) {
        *error = component.errorString();
        return {};
    }
    std::unique_ptr<QObject> root(component.create(context));
    if (!root)
        *error = component.errorString();
    return root;
}
} // namespace

void TestQmlCompositeCapture::initTestCase() {
    qmlRegisterType<Core::VideoEncoder>("AviQtl.Core", 1, 0, "VideoEncoder");
    qmlRegisterUncreatableType<TimelineController>("AviQtl.UI", 1, 0, "TimelineController", "Managed by C++");
    qmlRegisterSingletonInstance<ECSRenderBridge>("AviQtl.UI", 1, 0, "ECSRenderBridge", &ECSRenderBridge::instance());
    m_engine = std::make_unique<QQmlEngine>();

    const QString effectsDir = QStringLiteral(AVIQTL_DEPLOYED_EFFECTS_DIR);
    const QString objectsDir = QStringLiteral(AVIQTL_DEPLOYED_OBJECTS_DIR);
    QVERIFY2(QFileInfo(effectsDir).isDir(), qPrintable(QStringLiteral("Missing deployed effects directory: %1").arg(effectsDir)));
    QVERIFY2(QFileInfo(objectsDir).isDir(), qPrintable(QStringLiteral("Missing deployed objects directory: %1").arg(objectsDir)));

    auto &registry = Core::EffectRegistry::instance();
    registry.loadEffectsFromDirectory(effectsDir, QStringLiteral("built-in"));
    registry.loadEffectsFromDirectory(objectsDir, QStringLiteral("built-in"));

    for (const QString &id : {QStringLiteral("transform"), QStringLiteral("monochrome"),
                              QStringLiteral("text"), QStringLiteral("rect")}) {
        const Core::EffectMetadata metadata = registry.getEffect(id);
        QVERIFY2(!metadata.qmlSource.isEmpty(), qPrintable(QStringLiteral("Missing deployed registry entry: %1").arg(id)));
        QVERIFY2(QFileInfo::exists(QUrl(metadata.qmlSource).toLocalFile()),
                 qPrintable(QStringLiteral("Missing deployed QML for %1: %2").arg(id, metadata.qmlSource)));
    }
}

void TestQmlCompositeCapture::cleanup() {
    QQmlContext *context = m_engine->rootContext();
    context->setContextProperty(QStringLiteral("Workspace"), static_cast<QObject *>(nullptr));
    context->setContextProperty(QStringLiteral("SettingsManager"), static_cast<QObject *>(nullptr));
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(nullptr));
    m_engine->collectGarbage();
}

void TestQmlCompositeCapture::loadsDeployedQmlAssetsWithoutRuntimeErrors() {
    QTest::failOnWarning(QRegularExpression(QStringLiteral(".*(?:TypeError|ReferenceError|Final member).*$")));
    QQmlEngine &engine = *m_engine;
    Workspace workspace;
    workspace.newProject();
    QQmlContext *context = engine.rootContext();
    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &Core::SettingsManager::instance());
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(&WindowManager::instance()));
    context->setContextProperty(QStringLiteral("DefaultWidth"), AviQtl::kDefaultWidth);
    context->setContextProperty(QStringLiteral("DefaultHeight"), AviQtl::kDefaultHeight);
    context->setContextProperty(QStringLiteral("AviQtlAssetUrl"), QString());

    const QString effectsDir = QStringLiteral(AVIQTL_DEPLOYED_EFFECTS_DIR);
    const QStringList effectFiles = QDir(effectsDir).entryList({QStringLiteral("*.qml")}, QDir::Files, QDir::Name);
    QVERIFY(!effectFiles.isEmpty());

    for (const QString &effectFile : effectFiles) {
        const QString effectPath = QDir(effectsDir).filePath(effectFile);
        QVERIFY2(QFileInfo::exists(effectPath), qPrintable(QStringLiteral("Missing deployed effect: %1").arg(effectPath)));
        QQmlComponent component(&engine, QUrl::fromLocalFile(effectPath));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create(context));
        QVERIFY2(object != nullptr, qPrintable(component.errorString()));
        QCoreApplication::processEvents();
    }

    const QString objectsDir = QStringLiteral(AVIQTL_DEPLOYED_OBJECTS_DIR);
    const QStringList objectFiles = QDir(objectsDir).entryList({QStringLiteral("*.qml")}, QDir::Files, QDir::Name);
    QVERIFY(!objectFiles.isEmpty());
    for (const QString &objectFile : objectFiles) {
        const QString objectPath = QDir(objectsDir).filePath(objectFile);
        QQmlComponent component(&engine, QUrl::fromLocalFile(objectPath));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    }
}

void TestQmlCompositeCapture::previewQualitySettingsUpdateViewGeometry() {
    Core::SettingsManager &settings = Core::SettingsManager::instance();
    const QVariant originalScale = settings.value(QStringLiteral("previewRenderScale"), 1.0);
    const QVariant originalMsaa = settings.value(QStringLiteral("previewMsaaSamples"), 0);
    const auto restoreSettings = qScopeGuard([&settings, originalScale, originalMsaa]() {
        settings.setValue(QStringLiteral("previewRenderScale"), originalScale);
        settings.setValue(QStringLiteral("previewMsaaSamples"), originalMsaa);
    });
    settings.setValue(QStringLiteral("previewRenderScale"), 1.0);
    settings.setValue(QStringLiteral("previewMsaaSamples"), 0);

    QQmlEngine &engine = *m_engine;
    Workspace workspace;
    workspace.newProject();
    QQmlContext *context = engine.rootContext();
    context->setContextProperty(QStringLiteral("Workspace"), &workspace);
    context->setContextProperty(QStringLiteral("SettingsManager"), &settings);
    context->setContextProperty(QStringLiteral("WindowManager"), static_cast<QObject *>(&WindowManager::instance()));
    context->setContextProperty(QStringLiteral("DefaultWidth"), 400);
    context->setContextProperty(QStringLiteral("DefaultHeight"), 200);
    context->setContextProperty(QStringLiteral("AviQtlAssetUrl"), QString());

    QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/CompositeView.qml")));
    QVERIFY2(component.isReady(), qPrintable(component.errorString()));
    std::unique_ptr<QObject> object(component.create(context));
    QVERIFY2(object != nullptr, qPrintable(component.errorString()));
    auto *compositeView = qobject_cast<QQuickItem *>(object.get());
    QVERIFY(compositeView != nullptr);
    compositeView->setSize(QSizeF(400, 200));
    compositeView->setProperty("projectWidth", 400);
    compositeView->setProperty("projectHeight", 200);

    auto *view3D = compositeView->property("view3D").value<QQuickItem *>();
    QVERIFY(view3D != nullptr);
    QObject *environment = view3D->property("environment").value<QObject *>();
    QVERIFY(environment != nullptr);
    // QQuick3DSceneEnvironment's public QML enum values are stable: NoAA=0,
    // MSAA=2, and quality values match their sample counts (2, 4, 8).
    constexpr int noAa = 0;
    constexpr int msaa = 2;
    constexpr int medium = 2;
    constexpr int high = 4;
    constexpr int veryHigh = 8;
    QTRY_COMPARE(view3D->width(), 400.0);
    QTRY_COMPARE(view3D->height(), 200.0);
    QCOMPARE(compositeView->property("previewMsaaSamples").toInt(), 0);
    QCOMPARE(environment->property("antialiasingMode").toInt(), noAa);
    QCOMPARE(environment->property("antialiasingQuality").toInt(), medium);

    settings.setValue(QStringLiteral("previewRenderScale"), QStringLiteral("invalid"));
    QTRY_COMPARE(compositeView->property("previewRenderScale").toDouble(), 1.0);
    QTRY_COMPARE(view3D->width(), 400.0);
    QTRY_COMPARE(view3D->height(), 200.0);

    settings.setValue(QStringLiteral("previewRenderScale"), 0.5);
    settings.setValue(QStringLiteral("previewMsaaSamples"), 2);
    QTRY_COMPARE(view3D->width(), 200.0);
    QTRY_COMPARE(view3D->height(), 100.0);
    QTRY_COMPARE(view3D->scale(), 2.0);
    QTRY_COMPARE(compositeView->property("previewMsaaSamples").toInt(), 2);
    QTRY_COMPARE(environment->property("antialiasingMode").toInt(), msaa);
    QTRY_COMPARE(environment->property("antialiasingQuality").toInt(), medium);

    settings.setValue(QStringLiteral("previewMsaaSamples"), 4);
    QTRY_COMPARE(compositeView->property("previewMsaaSamples").toInt(), 4);
    QTRY_COMPARE(environment->property("antialiasingMode").toInt(), msaa);
    QTRY_COMPARE(environment->property("antialiasingQuality").toInt(), high);

    settings.setValue(QStringLiteral("previewMsaaSamples"), 8);
    QTRY_COMPARE(compositeView->property("previewMsaaSamples").toInt(), 8);
    QTRY_COMPARE(environment->property("antialiasingMode").toInt(), msaa);
    QTRY_COMPARE(environment->property("antialiasingQuality").toInt(), veryHigh);

    compositeView->setProperty("exportMode", true);
    QTRY_COMPARE(view3D->width(), 400.0);
    QTRY_COMPARE(view3D->height(), 200.0);
    QTRY_COMPARE(view3D->scale(), 1.0);
    QTRY_COMPARE(environment->property("antialiasingMode").toInt(), msaa);
    QTRY_COMPARE(environment->property("antialiasingQuality").toInt(), high);
}

QImage TestQmlCompositeCapture::grabView3D(QQuickItem *view3D) {
    const QSharedPointer<QQuickItemGrabResult> grab = view3D->grabToImage(QSize(320, 180));
    if (!grab) return {};
    QSignalSpy readySpy(grab.get(), &QQuickItemGrabResult::ready);
    return readySpy.wait(5'000) ? grab->image() : QImage();
}

QImage TestQmlCompositeCapture::grabView3DUntilVisible(QQuickItem *view3D, int minimumBrightness, int timeoutMs) {
    return grabView3DUntil(view3D, [minimumBrightness](const QImage &image) { return averageVisibleColor(image, minimumBrightness).x() >= 0.0F; }, timeoutMs);
}

QImage TestQmlCompositeCapture::grabView3DUntil(QQuickItem *view3D, const std::function<bool(const QImage &)> &predicate, int timeoutMs) {
    QElapsedTimer timer;
    timer.start();
    QImage image;
    do {
        if (view3D->window())
            view3D->window()->update();
        image = grabView3D(view3D);
        if (!image.isNull() && predicate(image))
            return image;
        QCoreApplication::processEvents();
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
    QQmlEngine &engine = *m_engine;
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
    QQmlEngine &engine = *m_engine;
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
    const QImage firstFrame = grabView3DUntilVisible(view3D);
    QVERIFY(!firstFrame.isNull());
    const double firstCenterX = brightPixelCenterX(firstFrame);
    QVERIFY(firstCenterX >= 0.0);

    QSignalSpy frameSpy(controller->transport(), &TransportService::currentFrameChanged);
    controller->transport()->setCurrentFrame_seek(30);
    const QVariantMap lastRenderData = syncEcsRenderData(compositeView);
    QCOMPARE(lastRenderData.value(QString::number(textClipId)).toMap().value(QStringLiteral("x")).toDouble(), 80.0);
    QTRY_COMPARE_WITH_TIMEOUT(frameSpy.count(), 1, 5'000);
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("currentFrame").toInt(), 30, 5'000);
    const QImage lastFrame = grabView3DUntil(view3D, [firstCenterX](const QImage &image) { return brightPixelCenterX(image) > firstCenterX + 40.0; });

    QVERIFY(!lastFrame.isNull());
    const double lastCenterX = brightPixelCenterX(lastFrame);
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
    const QImage monochromeFrame = grabView3DUntil(view3D, [](const QImage &image) {
        const QVector3D color = averageVisibleColor(image, 20);
        return color.x() >= 0.0F && std::abs(color.x() - color.y()) < 8.0F && std::abs(color.x() - color.z()) < 8.0F;
    });
    QVERIFY(!monochromeFrame.isNull());
    const QVector3D colorAfterEffect = averageVisibleColor(monochromeFrame);
    const QString afterMessage = QStringLiteral("monochrome channels differ: r=%1 g=%2 b=%3").arg(colorAfterEffect.x()).arg(colorAfterEffect.y()).arg(colorAfterEffect.z());
    QVERIFY(colorAfterEffect.x() >= 0.0F);
    QVERIFY2(std::abs(colorAfterEffect.x() - colorAfterEffect.y()) < 8.0F, qPrintable(afterMessage));
    QVERIFY2(std::abs(colorAfterEffect.x() - colorAfterEffect.z()) < 8.0F, qPrintable(afterMessage));
}

void TestQmlCompositeCapture::exportsDeployedRectangleAndDecodesDistinctFrames() {
    QQmlEngine &engine = *m_engine;
    Workspace workspace;
    TestSettingsManager settings;
    TestWindowManager windowManager;
    workspace.newProject();
    TimelineController *controller = workspace.currentTimeline();
    QVERIFY(controller != nullptr);
    controller->project()->setWidth(320);
    controller->project()->setHeight(180);
    controller->project()->setFps(30.0);

    QString error;
    std::unique_ptr<QObject> root = createMainWindow(engine, workspace, settings, windowManager, &error);
    QVERIFY2(root != nullptr, qPrintable(error));
    auto *mainWindow = qobject_cast<QQuickWindow *>(root.get());
    QVERIFY(mainWindow != nullptr);
    QTRY_VERIFY_WITH_TIMEOUT(mainWindow->isExposed(), 5'000);

    const int rectClipId = controller->timeline()->nextClipId();
    constexpr int clipStartFrame = 140;
    controller->createObject(QStringLiteral("rect"), clipStartFrame, 0);
    controller->updateClipEffectParam(rectClipId, 1, QStringLiteral("sizeW"), 100.0);
    controller->updateClipEffectParam(rectClipId, 1, QStringLiteral("sizeH"), 80.0);
    controller->updateClipEffectParam(rectClipId, 1, QStringLiteral("color"), QStringLiteral("#00ff00"));
    controller->setKeyframe(rectClipId, 0, QStringLiteral("x"), 0, -70.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});
    controller->setKeyframe(rectClipId, 0, QStringLiteral("x"), 1, 70.0, {{QStringLiteral("interp"), QStringLiteral("linear")}});

    const QVariantList clipModel = controller->getSceneClips(controller->currentSceneId());
    QCOMPARE(clipModel.size(), 1);
    const QVariantMap clipData = clipModel.first().toMap();
    QCOMPARE(clipData.value(QStringLiteral("id")).toInt(), rectClipId);
    QCOMPARE(clipData.value(QStringLiteral("type")).toString(), QStringLiteral("rect"));
    const QUrl rectQmlSource(clipData.value(QStringLiteral("qmlSource")).toString());
    QVERIFY2(rectQmlSource.isLocalFile(), qPrintable(rectQmlSource.toString()));
    QVERIFY2(QFileInfo::exists(rectQmlSource.toLocalFile()), qPrintable(rectQmlSource.toLocalFile()));
    QVERIFY2(rectQmlSource.toLocalFile().startsWith(QStringLiteral(AVIQTL_DEPLOYED_OBJECTS_DIR)),
             qPrintable(rectQmlSource.toLocalFile()));
    const QList<QObject *> effectModels = clipData.value(QStringLiteral("effectModels")).value<QList<QObject *>>();
    QCOMPARE(effectModels.size(), 2);
    QCOMPARE(effectModels.at(0)->property("id").toString(), QStringLiteral("transform"));
    QCOMPARE(effectModels.at(1)->property("id").toString(), QStringLiteral("rect"));

    auto *compositeView = controller->compositeView();
    QVERIFY(compositeView != nullptr);
    controller->transport()->setCurrentFrame_seek(clipStartFrame - 1);
    controller->transport()->setCurrentFrame_seek(clipStartFrame);
    const QString clipKey = QString::number(rectClipId);
    QTRY_VERIFY_WITH_TIMEOUT(ECSRenderBridge::instance().renderStateMap().contains(clipKey), 5'000);
    QTRY_VERIFY_WITH_TIMEOUT(compositeView->property("ecsRenderData").toMap().contains(clipKey), 5'000);
    QCOMPARE(compositeView->property("ecsRenderData").toMap().value(clipKey).toMap().value(QStringLiteral("x")).toDouble(), -70.0);
    QTRY_COMPARE_WITH_TIMEOUT(compositeView->property("clipModel").toList().size(), 1, 5'000);

    QObject *clipNode = nullptr;
    for (QObject *candidate : compositeView->findChildren<QObject *>()) {
        if (candidate->metaObject()->indexOfProperty("clipIdRole") >= 0 &&
            candidate->property("clipIdRole").toInt() == rectClipId) {
            clipNode = candidate;
            break;
        }
    }
    QVERIFY(clipNode != nullptr);
    QCOMPARE(clipNode->property("clipQmlSourceRole").toUrl(), rectQmlSource);

    QObject *nodeLoader = nullptr;
    for (QObject *candidate : clipNode->findChildren<QObject *>()) {
        if (candidate->metaObject()->indexOfProperty("componentFactory") >= 0 &&
            candidate->property("source").toUrl() == rectQmlSource) {
            nodeLoader = candidate;
            break;
        }
    }
    QVERIFY(nodeLoader != nullptr);
    QTRY_COMPARE_WITH_TIMEOUT(nodeLoader->property("status").toInt(), static_cast<int>(QQmlComponent::Ready), 5'000);
    QObject *rectItem = nodeLoader->property("item").value<QObject *>();
    QVERIFY(rectItem != nullptr);
    QObject *displayOutput = rectItem->property("displayOutput").value<QObject *>();
    QVERIFY(displayOutput != nullptr);
    QVERIFY(displayOutput->property("sourceItem").value<QObject *>() != nullptr);

    if (!QSGRendererInterface::isApiRhiBased(mainWindow->rendererInterface()->graphicsApi()))
        QSKIP("Qt Quick 3D rendering requires an RHI-based graphics API");

    mainWindow->update();
    auto *view3D = compositeView->property("view3D").value<QQuickItem *>();
    QVERIFY(view3D != nullptr);
    QVERIFY(brightPixelCenterX(grabView3DUntilVisible(view3D)) >= 0.0);

    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString outputPath = dir.filePath(QStringLiteral("deployed-rectangle.mp4"));
    const QVariantMap config = {
        {QStringLiteral("width"), 320},
        {QStringLiteral("height"), 180},
        {QStringLiteral("fps_num"), 30},
        {QStringLiteral("fps_den"), 1},
        {QStringLiteral("crf"), 18},
        {QStringLiteral("codecName"), QStringLiteral("libx264")},
        {QStringLiteral("audioCodecName"), QStringLiteral("aac")},
        {QStringLiteral("outputUrl"), outputPath},
        {QStringLiteral("startFrame"), clipStartFrame},
        {QStringLiteral("endFrame"), clipStartFrame + 2},
        {QStringLiteral("preset"), QStringLiteral("ultrafast")},
    };

    QSignalSpy exportSpy(controller, &TimelineController::exportFinished);
    controller->exportVideoAsync(config);
    QTRY_COMPARE_WITH_TIMEOUT(exportSpy.count(), 1, 20'000);
    QTRY_VERIFY_WITH_TIMEOUT(!controller->isExporting(), 20'000);
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
             qPrintable(QStringLiteral("decoded rectangle center did not move: frame 0=%1, frame 1=%2").arg(firstCenterX).arg(lastCenterX)));
}

QObject *TestQmlCompositeCapture::findSaveConfirmDialog(QObject *root) {
    const QList<QObject *> objects = root->findChildren<QObject *>();
    const auto it = std::ranges::find_if(objects, [](QObject *object) { return object->metaObject()->indexOfProperty("pendingAction") >= 0; });
    return it == objects.cend() ? nullptr : *it;
}

QObject *TestQmlCompositeCapture::findSaveDialog(QObject *root) {
    const QList<QObject *> objects = root->findChildren<QObject *>();
    const auto it = std::ranges::find_if(objects, [](QObject *object) { return object->metaObject()->indexOfProperty("_nextAction") >= 0; });
    return it == objects.cend() ? nullptr : *it;
}

void TestQmlCompositeCapture::discardingUnsavedProjectsCompletesApplicationQuit() {
    QQmlEngine &engine = *m_engine;
    Workspace workspace;
    TestSettingsManager settings;
    TestWindowManager windowManager;

    workspace.newProject();
    workspace.currentTimeline()->createScene(QStringLiteral("First dirty project"));
    workspace.newProject();
    workspace.currentTimeline()->createScene(QStringLiteral("Second dirty project"));
    QCOMPARE(workspace.tabs().size(), 2);
    QVERIFY(workspace.tabs().at(0).toMap().value(QStringLiteral("hasUnsavedChanges")).toBool());
    QVERIFY(workspace.tabs().at(1).toMap().value(QStringLiteral("hasUnsavedChanges")).toBool());

    QString error;
    std::unique_ptr<QObject> root = createMainWindow(engine, workspace, settings, windowManager, &error);
    QVERIFY2(root != nullptr, qPrintable(error));
    auto *mainWindow = qobject_cast<QQuickWindow *>(root.get());
    QVERIFY(mainWindow != nullptr);
    QObject *saveConfirmDialog = findSaveConfirmDialog(root.get());
    QVERIFY(saveConfirmDialog != nullptr);

    QVERIFY(!mainWindow->close());
    QCOMPARE(workspace.currentIndex(), 0);
    QVERIFY(QMetaObject::invokeMethod(saveConfirmDialog, "discarded"));
    QCOMPARE(workspace.currentIndex(), 1);
    QCOMPARE(windowManager.quitRequests(), 0);

    QVERIFY(QMetaObject::invokeMethod(saveConfirmDialog, "discarded"));
    QCOMPARE(windowManager.quitRequests(), 1);
    QCOMPARE(root->property("quitInProgress").toBool(), true);
}

void TestQmlCompositeCapture::savingUnsavedProjectCompletesApplicationQuit() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString projectUrl = QUrl::fromLocalFile(dir.filePath(QStringLiteral("quit-after-save.aviqtl"))).toString();

    QQmlEngine &engine = *m_engine;
    Workspace workspace;
    TestSettingsManager settings;
    TestWindowManager windowManager;
    workspace.newProject();
    workspace.currentTimeline()->createScene(QStringLiteral("Unsaved scene"));
    QVERIFY(workspace.currentTimeline()->hasUnsavedChanges());
    QVERIFY(workspace.currentTimeline()->currentProjectUrl().isEmpty());

    QString error;
    std::unique_ptr<QObject> root = createMainWindow(engine, workspace, settings, windowManager, &error);
    QVERIFY2(root != nullptr, qPrintable(error));
    auto *mainWindow = qobject_cast<QQuickWindow *>(root.get());
    QVERIFY(mainWindow != nullptr);
    QObject *saveConfirmDialog = findSaveConfirmDialog(root.get());
    QVERIFY(saveConfirmDialog != nullptr);
    QObject *saveDialog = findSaveDialog(root.get());
    QVERIFY(saveDialog != nullptr);

    QVERIFY(!mainWindow->close());
    QVERIFY(QMetaObject::invokeMethod(saveConfirmDialog, "accepted"));
    QCOMPARE(windowManager.quitRequests(), 0);
    QVERIFY(saveDialog->setProperty("file", QUrl(projectUrl)));
    QVERIFY(QMetaObject::invokeMethod(saveDialog, "accepted"));
    QCOMPARE(windowManager.quitRequests(), 1);
    QCOMPARE(root->property("quitInProgress").toBool(), true);
    QVERIFY(!workspace.currentTimeline()->hasUnsavedChanges());
    QVERIFY(QFileInfo::exists(QUrl(projectUrl).toLocalFile()));
}

QTEST_MAIN(TestQmlCompositeCapture)
#include "test_qml_composite_capture.moc"
