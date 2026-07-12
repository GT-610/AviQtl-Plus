#include "timeline_controller.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QQuickItem>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <cmath>

using namespace AviQtl::UI;

class TestExportWorkflow : public QObject {
    Q_OBJECT

  private slots:
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

  private:
    static QVariantMap validVideoConfig(const TimelineController &controller, const QString &outputPath);
    static void expectExportFailure(QSignalSpy &spy, const QString &expectedMessage);
};

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
    QQuickItem captureItem;
    captureItem.setSize(QSizeF(64, 64));
    controller.setCompositeView(&captureItem);
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    QVariantMap cfg = validVideoConfig(controller, outputPath);
    cfg.insert(QStringLiteral("width"), 64);
    cfg.insert(QStringLiteral("height"), 64);
    cfg.insert(QStringLiteral("endFrame"), 1);
    cfg.insert(QStringLiteral("preset"), QStringLiteral("ultrafast"));
    controller.exportVideoAsync(cfg);

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 10'000);
    expectExportFailure(spy, QStringLiteral("Frame capture error: failed to capture frame 0"));
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
    QQuickItem captureItem;
    captureItem.setSize(QSizeF(64, 64));
    controller.setCompositeView(&captureItem);
    QSignalSpy spy(&controller, &TimelineController::exportFinished);

    controller.exportImageSequence(outputDir, 95, QStringLiteral("PNG"), 0, 2);

    QTRY_COMPARE_WITH_TIMEOUT(spy.count(), 1, 10'000);
    expectExportFailure(spy, QStringLiteral("Frame capture error: failed to capture frame 0"));
    QVERIFY(!QDir(outputDir).exists());
}

void TestExportWorkflow::imageSequenceRefusesToOverwriteExistingFrames() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString outputDir = dir.filePath(QStringLiteral("existing-sequence"));
    QVERIFY(QDir().mkpath(outputDir));
    const QString existingFrame = QDir(outputDir).filePath(QStringLiteral("frame_0.png"));
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

QTEST_MAIN(TestExportWorkflow)
#include "test_export_workflow.moc"
