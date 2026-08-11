#include "performance_metrics.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_export_manager.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <cmath>
#include <memory>

using namespace AviQtl::UI;
using AviQtl::Core::SettingsManager;
using AviQtl::Core::VideoEncoder;
using AviQtl::Core::PerformanceCounter;
using AviQtl::Core::PerformanceMetrics;

namespace {
class QuickCaptureView {
  public:
    QuickCaptureView() : m_component(&m_engine) {}

    bool initialize(QString *errorMessage) {
        m_component.setData(R"(
            import QtQuick
            Rectangle {
                width: 64
                height: 64
                color: "#336699"
            }
        )", QUrl());
        if (!m_component.isReady()) {
            if (errorMessage != nullptr)
                *errorMessage = m_component.errorString();
            return false;
        }

        m_object.reset(m_component.create());
        m_item = qobject_cast<QQuickItem *>(m_object.get());
        if (!m_item) {
            if (errorMessage != nullptr)
                *errorMessage = QStringLiteral("Capture fixture root is not a QQuickItem");
            return false;
        }

        m_window.setGeometry(0, 0, 64, 64);
        m_item->setParentItem(m_window.contentItem());
        m_window.show();
        return true;
    }

    QQuickItem *item() const { return m_item; }
    QQuickWindow *window() { return &m_window; }

    void destroyItem() {
        m_object.reset();
        m_item = nullptr;
    }

  private:
    QQmlEngine m_engine;
    QQmlComponent m_component;
    QQuickWindow m_window;
    std::unique_ptr<QObject> m_object;
    QPointer<QQuickItem> m_item;
};
} // namespace

class TestExportWorkflow : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void cleanupTestCase();
    void videoExportRejectsEmptyPath();
    void videoExportRejectsInvalidRange();
    void videoExportRejectsMismatchedFps();
    void videoExportWithoutCompositeViewFailsBeforeCreatingOutput();
    void videoExportCaptureFailureRemovesPartialOutput();
    void imageSequenceRejectsEmptyPath();
    void imageSequenceRejectsInvalidRange();
    void imageSequenceWithoutCompositeViewFailsBeforeCreatingFrames();
    void imageSequenceCaptureFailureRemovesPartialOutput();
    void imageSequenceRefusesToOverwriteExistingFrames();
    void hiddenWindowExportsAreRestored();
    void exportStateTransitionsBeforeCompletion();
    void completionHandlerCanDestroyManager();

  private:
    static constexpr int kTestSequencePadding = 4;
    static QVariantMap validVideoConfig(const TimelineController &controller, const QString &outputPath);
    static VideoEncoder::Config validEncoderConfig(const TimelineController &controller, const QString &outputPath);
    static void expectExportFailure(QSignalSpy &spy, const QString &expectedMessage);

    QVariant m_originalSequencePadding;
};

void TestExportWorkflow::initTestCase() {
    SettingsManager &settings = SettingsManager::instance();
    m_originalSequencePadding = settings.value(QStringLiteral("exportSequencePadding"), kTestSequencePadding);
    settings.setValue(QStringLiteral("exportSequencePadding"), kTestSequencePadding);
}

void TestExportWorkflow::cleanupTestCase() { SettingsManager::instance().setValue(QStringLiteral("exportSequencePadding"), m_originalSequencePadding); }

QVariantMap TestExportWorkflow::validVideoConfig(const TimelineController &controller, const QString &outputPath) {
    const double fps = controller.project()->fps();
    return {
        {QStringLiteral("width"), controller.project()->width()},
        {QStringLiteral("height"), controller.project()->height()},
        {QStringLiteral("fps_num"), fps == std::floor(fps) ? static_cast<int>(fps * 1000.0) : static_cast<int>(std::round(fps * 1001.0))},
        {QStringLiteral("fps_den"), fps == std::floor(fps) ? 1000 : 1001},
        {QStringLiteral("bitrate"), 15'000'000},
        {QStringLiteral("crf"), 20},
        {QStringLiteral("codecName"), QStringLiteral("libx264")},
        {QStringLiteral("audioCodecName"), QStringLiteral("aac")},
        {QStringLiteral("audioBitrate"), 192'000},
        {QStringLiteral("outputUrl"), outputPath},
        {QStringLiteral("startFrame"), 0},
        {QStringLiteral("endFrame"), 30},
    };
}

VideoEncoder::Config TestExportWorkflow::validEncoderConfig(const TimelineController &controller, const QString &outputPath) {
    VideoEncoder::Config config;
    const double fps = controller.project()->fps();
    config.width = 64;
    config.height = 64;
    config.fps_num = fps == std::floor(fps) ? static_cast<int>(fps * 1000.0) : static_cast<int>(std::round(fps * 1001.0));
    config.fps_den = fps == std::floor(fps) ? 1000 : 1001;
    config.crf = 20;
    config.codecName = QStringLiteral("libx264");
    config.audioCodecName = QStringLiteral("aac");
    config.outputUrl = outputPath;
    config.startFrame = 0;
    config.endFrame = 2;
    config.preset = QStringLiteral("ultrafast");
    return config;
}

void TestExportWorkflow::expectExportFailure(QSignalSpy &spy, const QString &expectedMessage) {
    QCOMPARE(spy.count(), 1);
    const QList<QVariant> args = spy.takeFirst();
    QCOMPARE(args.at(0).toBool(), false);
    QCOMPARE(args.at(1).toString(), expectedMessage);
}

void TestExportWorkflow::videoExportRejectsEmptyPath() {
    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    QVariantMap cfg = validVideoConfig(controller, QString());
    controller.exportVideoAsync(cfg);

    expectExportFailure(spy, QStringLiteral("Configuration error: missing output path"));
}

void TestExportWorkflow::videoExportRejectsInvalidRange() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    QVariantMap cfg = validVideoConfig(controller, dir.filePath(QStringLiteral("invalid-range.mp4")));
    cfg.insert(QStringLiteral("startFrame"), 20);
    cfg.insert(QStringLiteral("endFrame"), 20);
    controller.exportVideoAsync(cfg);

    expectExportFailure(spy, QStringLiteral("Configuration error: export end frame must be after start frame"));
}

void TestExportWorkflow::videoExportRejectsMismatchedFps() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    QVariantMap cfg = validVideoConfig(controller, dir.filePath(QStringLiteral("wrong-fps.mp4")));
    cfg.insert(QStringLiteral("fps_num"), 24'000);
    cfg.insert(QStringLiteral("fps_den"), 1000);
    controller.exportVideoAsync(cfg);

    expectExportFailure(spy, QStringLiteral("Configuration error: export FPS does not match project FPS"));
}

void TestExportWorkflow::videoExportWithoutCompositeViewFailsBeforeCreatingOutput() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString outputPath = dir.filePath(QStringLiteral("no-view.mp4"));
    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    controller.exportVideoAsync(validVideoConfig(controller, outputPath));

    expectExportFailure(spy, QStringLiteral("Frame capture error: no preview view is available"));
    QVERIFY(!QFileInfo::exists(outputPath));
}

void TestExportWorkflow::videoExportCaptureFailureRemovesPartialOutput() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString outputPath = dir.filePath(QStringLiteral("capture-failure.mp4"));
    TimelineController controller;
    QuickCaptureView captureView;
    QString fixtureError;
    QVERIFY2(captureView.initialize(&fixtureError), qPrintable(fixtureError));
    QTRY_VERIFY_WITH_TIMEOUT(captureView.window()->isExposed(), 5'000);
    controller.setCompositeView(captureView.item());
    bool firstFrameReachedEncoder = false;
    connect(controller.transport(), &TransportService::currentFrameChanged, this, [&]() {
        if (controller.transport()->currentFrame() == 1) {
            firstFrameReachedEncoder = QFileInfo::exists(outputPath);
            captureView.destroyItem();
        }
    });
    TimelineExportManager exportManager(&controller);
    QSignalSpy spy(&exportManager, &TimelineExportManager::exportFinished);

    QVERIFY(exportManager.exportVideoAsync(validEncoderConfig(controller, outputPath)));

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(!exportManager.isExporting(), 10'000);
    expectExportFailure(spy, QStringLiteral("Frame render timeout: failed to render frame 1"));
    QVERIFY(firstFrameReachedEncoder);
    QVERIFY(!QFileInfo::exists(outputPath));
}

void TestExportWorkflow::imageSequenceRejectsEmptyPath() {
    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    controller.exportImageSequence(QString(), 95, QStringLiteral("PNG"), 0, 30);

    expectExportFailure(spy, QStringLiteral("Configuration error: missing output path"));
}

void TestExportWorkflow::imageSequenceRejectsInvalidRange() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    controller.exportImageSequence(dir.filePath(QStringLiteral("invalid-sequence")), 95, QStringLiteral("PNG"), 30, 30);

    expectExportFailure(spy, QStringLiteral("Configuration error: export end frame must be after start frame"));
}

void TestExportWorkflow::imageSequenceWithoutCompositeViewFailsBeforeCreatingFrames() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString outputDir = dir.filePath(QStringLiteral("sequence"));
    TimelineController controller;
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    controller.exportImageSequence(outputDir, 95, QStringLiteral("PNG"), 0, 2);

    expectExportFailure(spy, QStringLiteral("Frame capture error: no preview view is available"));
    QVERIFY(!QDir(outputDir).exists());
}

void TestExportWorkflow::imageSequenceCaptureFailureRemovesPartialOutput() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString outputDir = dir.filePath(QStringLiteral("capture-failure-sequence"));
    TimelineController controller;
    QuickCaptureView captureView;
    QString fixtureError;
    QVERIFY2(captureView.initialize(&fixtureError), qPrintable(fixtureError));
    QTRY_VERIFY_WITH_TIMEOUT(captureView.window()->isExposed(), 5'000);
    controller.setCompositeView(captureView.item());
    const QString firstFramePath = QDir(outputDir).filePath(QStringLiteral("frame_%1.png").arg(0, kTestSequencePadding, 10, QLatin1Char('0')));
    bool firstFrameWasWritten = false;
    connect(controller.transport(), &TransportService::currentFrameChanged, this, [&]() {
        if (controller.transport()->currentFrame() == 1) {
            firstFrameWasWritten = QFileInfo::exists(firstFramePath);
            captureView.destroyItem();
        }
    });
    TimelineExportManager exportManager(&controller);
    QSignalSpy spy(&exportManager, &TimelineExportManager::exportFinished);

    QVERIFY(exportManager.exportImageSequence(outputDir, 95, QStringLiteral("PNG"), 0, 2));

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 10'000);
    QTRY_VERIFY_WITH_TIMEOUT(!exportManager.isExporting(), 10'000);
    expectExportFailure(spy, QStringLiteral("Frame render timeout: failed to render frame 1"));
    QVERIFY(firstFrameWasWritten);
    QVERIFY(!QFileInfo::exists(firstFramePath));
    QVERIFY(!QDir(outputDir).exists());
}

void TestExportWorkflow::imageSequenceRefusesToOverwriteExistingFrames() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString outputDir = dir.filePath(QStringLiteral("existing-sequence"));
    QVERIFY(QDir().mkpath(outputDir));
    const QString existingFrame = QDir(outputDir).filePath(QStringLiteral("frame_%1.png").arg(0, kTestSequencePadding, 10, QLatin1Char('0')));
    QFile sentinel(existingFrame);
    QVERIFY(sentinel.open(QIODevice::WriteOnly));
    const QByteArray sentinelData("existing frame data");
    QCOMPARE(sentinel.write(sentinelData), sentinelData.size());
    sentinel.close();

    TimelineController controller;
    QQuickItem captureItem;
    captureItem.setSize(QSizeF(64, 64));
    controller.setCompositeView(&captureItem);
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    controller.exportImageSequence(outputDir, 95, QStringLiteral("PNG"), 0, 2);

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 10'000);
    expectExportFailure(spy, QStringLiteral("Output error: output file already exists: %1").arg(existingFrame));
    QFile preservedFrame(existingFrame);
    QVERIFY(preservedFrame.open(QIODevice::ReadOnly));
    QCOMPARE(preservedFrame.readAll(), sentinelData);
}

void TestExportWorkflow::hiddenWindowExportsAreRestored() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    TimelineController controller;
    QuickCaptureView captureView;
    QString fixtureError;
    QVERIFY2(captureView.initialize(&fixtureError), qPrintable(fixtureError));
    QTRY_VERIFY_WITH_TIMEOUT(captureView.window()->isExposed(), 5'000);
    captureView.window()->hide();
    QTRY_VERIFY_WITH_TIMEOUT(!captureView.window()->isVisible(), 5'000);
    controller.setCompositeView(captureView.item());

    TimelineExportManager exportManager(&controller);
    const QString sequenceDir = dir.filePath(QStringLiteral("hidden-sequence"));
    QSignalSpy imageSpy(&exportManager, &TimelineExportManager::exportFinished);
    QVERIFY(exportManager.exportImageSequence(sequenceDir, 95, QStringLiteral("PNG"), 0, 1));
    QTRY_COMPARE_WITH_TIMEOUT(imageSpy.count(), 1, 10'000);
    QCOMPARE(imageSpy.takeFirst().at(0).toBool(), true);
    QVERIFY(QFileInfo::exists(QDir(sequenceDir).filePath(QStringLiteral("frame_0000.png"))));
    QTRY_VERIFY_WITH_TIMEOUT(!captureView.window()->isVisible(), 5'000);

    const QString videoPath = dir.filePath(QStringLiteral("hidden-video.mp4"));
    VideoEncoder::Config videoConfig = validEncoderConfig(controller, videoPath);
    videoConfig.endFrame = 1;
    QSignalSpy videoSpy(&exportManager, &TimelineExportManager::exportFinished);
    QVERIFY(exportManager.exportVideoAsync(videoConfig));
    QTRY_COMPARE_WITH_TIMEOUT(videoSpy.count(), 1, 10'000);
    QCOMPARE(videoSpy.takeFirst().at(0).toBool(), true);
    QVERIFY(QFileInfo(videoPath).size() > 0);
    QTRY_VERIFY_WITH_TIMEOUT(!captureView.window()->isVisible(), 5'000);
}

void TestExportWorkflow::exportStateTransitionsBeforeCompletion() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    TimelineController controller;
    QuickCaptureView captureView;
    QString fixtureError;
    QVERIFY2(captureView.initialize(&fixtureError), qPrintable(fixtureError));
    QTRY_VERIFY_WITH_TIMEOUT(captureView.window()->isExposed(), 5'000);
    controller.setCompositeView(captureView.item());

    TimelineExportManager exportManager(&controller);
    PerformanceMetrics::instance().reset();

    QStringList events;
    connect(&exportManager, &TimelineExportManager::exportingChanged, this, [&events](bool exporting) { events.append(exporting ? QStringLiteral("active") : QStringLiteral("inactive")); });
    connect(&exportManager, &TimelineExportManager::exportFinished, this, [&events]() { events.append(QStringLiteral("finished")); });
    QSignalSpy finishedSpy(&exportManager, &TimelineExportManager::exportFinished);

    QVERIFY(exportManager.exportImageSequence(dir.filePath(QStringLiteral("sequence")), 95, QStringLiteral("PNG"), 0, 1));
    QVERIFY(exportManager.isExporting());
    QCOMPARE(events, QStringList{QStringLiteral("active")});

    QTRY_COMPARE_WITH_TIMEOUT(finishedSpy.count(), 1, 10'000);
    QTRY_COMPARE_WITH_TIMEOUT(events.size(), 3, 10'000);
    QVERIFY(!exportManager.isExporting());
    QCOMPARE(events, QStringList({QStringLiteral("active"), QStringLiteral("inactive"), QStringLiteral("finished")}));
    const auto metrics = PerformanceMetrics::instance().snapshot();
    QCOMPARE(metrics.value(PerformanceCounter::ExportFrames), quint64{1});
    QVERIFY(metrics.value(PerformanceCounter::ExportFrameWaitNanoseconds) > 0);
    QVERIFY(metrics.value(PerformanceCounter::ExportFrameGrabNanoseconds) > 0);
}

void TestExportWorkflow::completionHandlerCanDestroyManager() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    TimelineController controller;
    QuickCaptureView captureView;
    QString fixtureError;
    QVERIFY2(captureView.initialize(&fixtureError), qPrintable(fixtureError));
    QTRY_VERIFY_WITH_TIMEOUT(captureView.window()->isExposed(), 5'000);
    controller.setCompositeView(captureView.item());

    auto *exportManager = new TimelineExportManager(&controller);
    QPointer<TimelineExportManager> managerGuard(exportManager);
    bool completionHandled = false;
    connect(exportManager, &TimelineExportManager::exportFinished, this, [&completionHandled, exportManager]() {
        completionHandled = true;
        delete exportManager;
    });

    QVERIFY(exportManager->exportImageSequence(dir.filePath(QStringLiteral("sequence")), 95, QStringLiteral("PNG"), 0, 1));
    QTRY_VERIFY_WITH_TIMEOUT(completionHandled, 10'000);
    QVERIFY(managerGuard.isNull());
}

QTEST_MAIN(TestExportWorkflow)
#include "test_export_workflow.moc"
