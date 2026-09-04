#include "timeline_export_manager.hpp"
#include "constants.hpp"
#include "engine/audio_mixer.hpp"
#include "performance_metrics.hpp"
#include "rust_export_plan.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "video_encoder.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QEventLoop>
#include <QFile>
#include <QImage>
#include <QPointer>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QTimer>
#include <algorithm>

namespace {
constexpr int kDefaultGrabTimeoutMs = 2000;
constexpr int kExportAudioChannels = 2;

QString imageSequenceFileName(int frame, int padDigits, const QString &extension) {
    return QStringLiteral("frame_") + QString::number(frame).rightJustified(padDigits, QChar::fromLatin1('0')) + extension;
}

QPointer<QQuickItem> captureTargetForView(QPointer<QQuickItem> view) {
    if (!view) {
        return {};
    }
    if (auto *v3d = view->property("view3D").value<QQuickItem *>()) {
        return v3d;
    }
    return view;
}

// RAII helper that keeps the preview window renderable during export and
// restores its original visibility together with exportMode afterwards.
class ExportModeGuard {
public:
    explicit ExportModeGuard(QPointer<QQuickItem> view)
        : m_view(view) {
        if (!m_view)
            return;
        const Qt::ConnectionType connectionType = m_view->thread() == QThread::currentThread()
                                                      ? Qt::DirectConnection
                                                      : Qt::BlockingQueuedConnection;
        QMetaObject::invokeMethod(m_view.data(), [this, view = m_view]() -> void {
            if (!view)
                return;
            view->setProperty("exportMode", true);
            m_window = view->window();
            if (!m_window)
                return;
            m_originalVisibility = m_window->visibility();
            m_wasVisible = m_window->isVisible();
            if (!m_window->isExposed())
                m_window->showNormal();
        }, connectionType);
    }

    ~ExportModeGuard() {
        QPointer<QQuickItem> view = m_view;
        QPointer<QQuickWindow> window = m_window;
        QObject *context = window ? static_cast<QObject *>(window.data()) : static_cast<QObject *>(view.data());
        if (!context)
            return;
        const Qt::ConnectionType connectionType = context->thread() == QThread::currentThread()
                                                      ? Qt::DirectConnection
                                                      : Qt::QueuedConnection;
        QMetaObject::invokeMethod(context, [view, window, visibility = m_originalVisibility, wasVisible = m_wasVisible]() -> void {
            if (view)
                view->setProperty("exportMode", false);
            if (window) {
                window->setVisibility(visibility);
                if (!wasVisible)
                    window->hide();
            }
        }, connectionType);
    }

    ExportModeGuard(const ExportModeGuard &) = delete;
    ExportModeGuard &operator=(const ExportModeGuard &) = delete;
    ExportModeGuard(ExportModeGuard &&) = default;
    ExportModeGuard &operator=(ExportModeGuard &&) = default;

private:
    QPointer<QQuickItem> m_view;
    QPointer<QQuickWindow> m_window;
    QWindow::Visibility m_originalVisibility = QWindow::Hidden;
    bool m_wasVisible = false;
};

} // namespace

namespace AviQtl::UI {

TimelineExportManager::TimelineExportManager(TimelineController *controller, QObject *parent) : QObject(parent), m_controller(controller) {}

TimelineExportManager::~TimelineExportManager() {
    if (m_exportThread) {
        m_cancelRequested = true;
        m_exportThread->wait();
    }
}

bool TimelineExportManager::beginExport() {
    bool expected = false;
    if (!m_exporting.compare_exchange_strong(expected, true))
        return false;
    emit exportingChanged(true);
    return true;
}

void TimelineExportManager::finishExport(bool success, const QString &message) {
    if (m_exporting.exchange(false))
        emit exportingChanged(false);
    emit exportFinished(success, message);
}

bool TimelineExportManager::exportVideoAsync(const AviQtl::Core::VideoEncoder::Config &config) {
    if (m_exporting.load()) {
        return false;
    }

    const QPointer<QQuickItem> view = m_controller->compositeView();
    if (!captureTargetForView(view)) {
        emit exportFinished(false, tr("Frame capture error: no preview view is available"));
        return true;
    }

    if (!beginExport())
        return false;

    m_cancelRequested = false;

    m_exportThread = QThread::create([this, config]() -> void { runExport(config); });
    connect(m_exportThread, &QThread::finished, m_exportThread, &QObject::deleteLater);
    m_exportThread->start();
    return true;
}

void TimelineExportManager::cancelExport() { m_cancelRequested = true; }

void TimelineExportManager::runExport(const AviQtl::Core::VideoEncoder::Config &config) {
    QPointer<QQuickItem> view = m_controller->compositeView();
    QPointer<QQuickItem> targetItem = captureTargetForView(view);

    if (!targetItem) {
        finishExport(false, tr("Frame capture error: no preview view is available"));
        return;
    }

    const auto exportPlan = AviQtl::RustCore::Export::planVideo(
        config.width, config.height, config.fps_num, config.fps_den, config.startFrame,
        config.endFrame, m_controller->timelineDuration(), !config.outputUrl.trimmed().isEmpty(),
        m_controller->project()->fps());
    if (static_cast<AviQtl::RustCore::Export::ConfigurationError>(exportPlan.error) !=
        AviQtl::RustCore::Export::ConfigurationError::None) {
        finishExport(false, tr("Encoder error: initialization failed"));
        return;
    }

    ExportModeGuard exportModeGuard(view);

    AviQtl::Core::VideoEncoder encoder;
    if (!encoder.open(config)) {
        finishExport(false, tr("Encoder error: initialization failed"));
        return;
    }

    const AviQtl::Core::SettingsManager &settings = AviQtl::Core::SettingsManager::instance();
    const int sr = std::max(
        1, settings.intValue(QStringLiteral("defaultProjectSampleRate"),
                             AviQtl::kDefaultSampleRate));
    // AudioMixer currently produces stereo interleaved samples regardless of the
    // playback-device channel layout. Keep the export stream consistent with it.
    if (!encoder.addAudioStream(sr, kExportAudioChannels)) {
        encoder.close();
        QFile::remove(config.outputUrl);
        finishExport(false, tr("Encoder error: audio stream initialization failed"));
        return;
    }

    const double fps = m_controller->project()->fps();
    const int startFrame = exportPlan.start_frame;
    const int endFrame = exportPlan.end_frame;
    const int totalFrames = exportPlan.total_frames;

    emit exportStarted(totalFrames);

    const int grabTimeout =
        settings.intValue(QStringLiteral("exportFrameGrabTimeoutMs"), kDefaultGrabTimeoutMs);
    const int progInterval =
        std::max(1, settings.intValue(QStringLiteral("exportProgressInterval"), 5));

    QElapsedTimer timer;
    timer.start();

    for (int frame = startFrame; frame < endFrame; ++frame) {
        if (m_cancelRequested.load()) {
            encoder.close();
            QFile::remove(config.outputUrl);
            finishExport(false, tr("Export cancelled"));
            return;
        }

        QMetaObject::invokeMethod(m_controller->transport(), [this, frame]() -> void { m_controller->transport()->setCurrentFrame(frame); }, Qt::BlockingQueuedConnection);

        {
            AviQtl::Core::ScopedPerformanceTimer waitTimer(AviQtl::Core::PerformanceCounter::ExportFrameWaitNanoseconds);
            if (!waitForRenderFrame(targetItem, grabTimeout)) {
                encoder.close();
                QFile::remove(config.outputUrl);
                finishExport(false, tr("Frame render timeout: failed to render frame %1").arg(frame));
                return;
            }
        }

        QImage img;
        {
            AviQtl::Core::ScopedPerformanceTimer grabTimer(AviQtl::Core::PerformanceCounter::ExportFrameGrabNanoseconds);
            img = grabFrame(targetItem, QSize(config.width, config.height), grabTimeout);
        }
        if (img.isNull()) {
            qWarning() << "Frame grab failed for frame" << frame;
            encoder.close();
            QFile::remove(config.outputUrl);
            finishExport(false, tr("Frame grab error: failed to capture frame %1").arg(frame));
            return;
        }

        bool frameQueued = false;
        {
            AviQtl::Core::ScopedPerformanceTimer queueTimer(AviQtl::Core::PerformanceCounter::ExportEncoderQueueNanoseconds);
            frameQueued = encoder.pushFrame(img, frame - startFrame);
        }
        if (!frameQueued) {
            encoder.close();
            QFile::remove(config.outputUrl);
            finishExport(false, tr("Encoder error: failed to queue video frame %1").arg(frame));
            return;
        }
        AviQtl::Core::PerformanceMetrics::instance().add(AviQtl::Core::PerformanceCounter::ExportFrames);

        AviQtlExportAudioFramePlan audioPlan{};
        if (AviQtl::RustCore::Export::planAudioFrame(
                frame - startFrame, sr, config.fps_num, config.fps_den, audioPlan) !=
            AviQtl::RustCore::Export::Status::Ok) {
            encoder.close();
            QFile::remove(config.outputUrl);
            finishExport(false,
                         tr("Encoder error: audio planning failed for frame %1").arg(frame));
            return;
        }
        const int samplesNeeded = audioPlan.samples_for_frame;

        if (samplesNeeded > 0) {
            std::vector<float> audio;
            m_controller->mediaManager()->audioMixer()->mix(frame, fps, samplesNeeded, audio);
            if (!encoder.pushAudio(audio.data(), static_cast<int>(audio.size()))) {
                encoder.close();
                QFile::remove(config.outputUrl);
                finishExport(false, tr("Encoder error: failed to queue audio for frame %1").arg(frame));
                return;
            }
        }

        const int done = frame - startFrame + 1;
        const auto progressPlan = AviQtl::RustCore::Export::planProgress(
            done, totalFrames, progInterval, timer.elapsed());
        if (progressPlan.should_emit != 0) {
            emit exportProgressChanged(progressPlan.progress, progressPlan.current_frame,
                                       progressPlan.total_frames, progressPlan.eta_seconds);
        }
    }

    encoder.close();
    finishExport(true, tr("Export complete"));
}

bool TimelineExportManager::exportImageSequence(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    if (m_exporting.load()) {
        return false;
    }

    const QPointer<QQuickItem> view = m_controller->compositeView();
    if (!captureTargetForView(view)) {
        emit exportFinished(false, tr("Frame capture error: no preview view is available"));
        return true;
    }

    if (!beginExport())
        return false;

    m_cancelRequested = false;

    m_exportThread = QThread::create([this, dir, quality, format, startFrame, endFrame]() -> void { runImageSequenceExport(dir, quality, format, startFrame, endFrame); });
    connect(m_exportThread, &QThread::finished, m_exportThread, &QObject::deleteLater);
    m_exportThread->start();
    return true;
}

QImage TimelineExportManager::grabFrame(QPointer<QQuickItem> targetItem, const QSize &size, int timeoutMs) const {
    QImage img;
    if (!targetItem) {
        return img;
    }
    QSharedPointer<QQuickItemGrabResult> grab;
    QMetaObject::invokeMethod(targetItem.data(), [targetItem, &grab, size]() -> void {
        grab = targetItem->grabToImage(size.isValid() ? size : QSize());
    }, Qt::BlockingQueuedConnection);

    if (!grab) {
        return img;
    }

    QEventLoop loop;
    bool ready = false;
    connect(grab.get(), &QQuickItemGrabResult::ready, &loop, [&ready, &loop]() {
        ready = true;
        loop.quit();
    });
    QTimer::singleShot(timeoutMs, &loop, &QEventLoop::quit);
    loop.exec();

    if (ready) {
        img = grab->image();
    }
    return img;
}

bool TimelineExportManager::waitForRenderFrame(QPointer<QQuickItem> targetItem, int timeoutMs) const {
    if (!targetItem)
        return false;

    QPointer<QQuickWindow> window;
    QEventLoop loop;
    QMetaObject::Connection synchronizedConnection;
    QMetaObject::Connection renderedConnection;
    bool synchronized = false;
    bool rendered = false;
    QMetaObject::invokeMethod(targetItem.data(), [&]() -> void {
        if (!targetItem)
            return;
        window = targetItem->window();
        if (!window)
            return;
        synchronizedConnection = connect(window.data(), &QQuickWindow::afterSynchronizing, &loop, [&]() -> void {
            synchronized = true;
        }, Qt::QueuedConnection);
        renderedConnection = connect(window.data(), &QQuickWindow::afterRendering, &loop, [&]() -> void {
            if (!synchronized)
                return;
            rendered = true;
            loop.quit();
        }, Qt::QueuedConnection);
        if (!window->isExposed())
            window->showNormal();
        window->update();
    }, Qt::BlockingQueuedConnection);

    if (!window)
        return false;

    QTimer timeoutTimer;
    timeoutTimer.setSingleShot(true);
    connect(&timeoutTimer, &QTimer::timeout, &loop, &QEventLoop::quit);
    timeoutTimer.start(std::max(1, timeoutMs));
    loop.exec();
    disconnect(synchronizedConnection);
    disconnect(renderedConnection);
    return rendered;
}

void TimelineExportManager::runImageSequenceExport(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    QDir outputDir(dir);
    const AviQtl::Core::SettingsManager &settings = AviQtl::Core::SettingsManager::instance();
    const auto exportPlan = AviQtl::RustCore::Export::planImageSequence(
        startFrame, endFrame, m_controller->timelineDuration(),
        settings.intValue(QStringLiteral("exportSequencePadding"), 6),
        !dir.trimmed().isEmpty(), format);
    if (static_cast<AviQtl::RustCore::Export::ConfigurationError>(exportPlan.error) !=
        AviQtl::RustCore::Export::ConfigurationError::None) {
        finishExport(false, tr("Output error: invalid image sequence configuration"));
        return;
    }
    startFrame = exportPlan.start_frame;
    endFrame = exportPlan.end_frame;
    const int totalFrames = exportPlan.total_frames;
    const int padDigits = exportPlan.pad_digits;
    const auto plannedImageFormat =
        static_cast<AviQtl::RustCore::Export::ImageFormat>(exportPlan.image_format);
    const QString extension = AviQtl::RustCore::Export::imageExtension(plannedImageFormat);
    const QByteArray imageFormat =
        AviQtl::RustCore::Export::imageEncoderName(plannedImageFormat);
    const int saveQuality = quality;

    emit exportStarted(totalFrames);

    QPointer<QQuickItem> view = m_controller->compositeView();
    QPointer<QQuickItem> targetItem = captureTargetForView(view);

    if (!targetItem) {
        finishExport(false, tr("Frame capture error: no preview view is available"));
        return;
    }

    const bool createdOutputDir = !outputDir.exists();
    if (createdOutputDir) {
        if (!outputDir.mkpath(QStringLiteral("."))) {
            finishExport(false, tr("Output error: cannot create output directory"));
            return;
        }
    }
    QStringList writtenFiles;
    const auto cleanupPartialOutput = [&outputDir, &writtenFiles, createdOutputDir]() {
        for (const QString &filePath : writtenFiles) {
            QFile::remove(filePath);
        }
        if (createdOutputDir) {
            QDir parentDir(outputDir.absolutePath());
            const QString outputDirName = parentDir.dirName();
            if (parentDir.cdUp()) {
                parentDir.rmdir(outputDirName);
            }
        }
    };
    const auto outputPathForFrame = [&outputDir, padDigits, &extension](int frame) {
        return outputDir.filePath(imageSequenceFileName(frame, padDigits, extension));
    };

    for (int frame = startFrame; frame < endFrame; ++frame) {
        const QString filePath = outputPathForFrame(frame);
        if (QFile::exists(filePath)) {
            cleanupPartialOutput();
            finishExport(false, tr("Output error: output file already exists: %1").arg(filePath));
            return;
        }
    }

    ExportModeGuard exportModeGuard(view);

    const int grabTimeout =
        settings.intValue(QStringLiteral("exportFrameGrabTimeoutMs"), kDefaultGrabTimeoutMs);
    const int progInterval =
        std::max(1, settings.intValue(QStringLiteral("exportProgressInterval"), 5));

    QElapsedTimer timer;
    timer.start();

    for (int frame = startFrame; frame < endFrame; ++frame) {
        if (m_cancelRequested.load()) {
            cleanupPartialOutput();
            finishExport(false, tr("Export cancelled"));
            return;
        }

        QMetaObject::invokeMethod(m_controller->transport(), [this, frame]() -> void { m_controller->transport()->setCurrentFrame(frame); }, Qt::BlockingQueuedConnection);

        {
            AviQtl::Core::ScopedPerformanceTimer waitTimer(AviQtl::Core::PerformanceCounter::ExportFrameWaitNanoseconds);
            if (!waitForRenderFrame(targetItem, grabTimeout)) {
                cleanupPartialOutput();
                finishExport(false, tr("Frame render timeout: failed to render frame %1").arg(frame));
                return;
            }
        }

        QImage img;
        {
            AviQtl::Core::ScopedPerformanceTimer grabTimer(AviQtl::Core::PerformanceCounter::ExportFrameGrabNanoseconds);
            img = grabFrame(targetItem, QSize(), grabTimeout);
        }

        if (img.isNull()) {
            qWarning() << "Frame grab failed for frame" << frame;
            cleanupPartialOutput();
            finishExport(false, tr("Frame grab error: failed to capture frame %1").arg(frame));
            return;
        }

        {
            const QString filePath = outputPathForFrame(frame);
            if (!img.save(filePath, imageFormat, saveQuality)) {
                qWarning() << "Failed to save frame" << frame << "to" << filePath;
                QFile::remove(filePath);
                cleanupPartialOutput();
                finishExport(false, tr("Output error: failed to save frame %1").arg(frame));
                return;
            }
            writtenFiles.push_back(filePath);
        }
        AviQtl::Core::PerformanceMetrics::instance().add(AviQtl::Core::PerformanceCounter::ExportFrames);

        const int done = frame - startFrame + 1;
        const auto progressPlan = AviQtl::RustCore::Export::planProgress(
            done, totalFrames, progInterval, timer.elapsed());
        if (progressPlan.should_emit != 0) {
            emit exportProgressChanged(progressPlan.progress, progressPlan.current_frame,
                                       progressPlan.total_frames, progressPlan.eta_seconds);
        }
    }

    finishExport(true, tr("Export complete"));
}

} // namespace AviQtl::UI
