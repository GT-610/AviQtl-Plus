#include "timeline_export_manager.hpp"
#include "constants.hpp"
#include "engine/audio_mixer.hpp"
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
#include <QScopeGuard>
#include <QTimer>
#include <algorithm>
#include <utility>

namespace {
constexpr int kDefaultGrabTimeoutMs = 2000;
constexpr int kFrameSettleWaitMs = 8;
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

// RAII helper that ensures exportMode is reset on the GUI item.
class ExportModeGuard {
public:
    explicit ExportModeGuard(QPointer<QQuickItem> view)
        : m_view(view) {}

    ~ExportModeGuard() {
        if (m_view) {
            QMetaObject::invokeMethod(m_view.data(), [v = m_view.data()]() -> void { v->setProperty("exportMode", false); }, Qt::BlockingQueuedConnection);
        }
    }

    ExportModeGuard(const ExportModeGuard &) = delete;
    ExportModeGuard &operator=(const ExportModeGuard &) = delete;
    ExportModeGuard(ExportModeGuard &&) = default;
    ExportModeGuard &operator=(ExportModeGuard &&) = default;

private:
    QPointer<QQuickItem> m_view;
};

} // namespace

namespace AviQtl::UI {

TimelineExportManager::TimelineExportManager(TimelineController *controller, QObject *parent) : QObject(parent), m_controller(controller) {}

TimelineExportManager::TimelineExportManager(TimelineController *controller, FrameGrabber frameGrabber, QObject *parent)
    : QObject(parent), m_controller(controller), m_frameGrabber(std::move(frameGrabber)) {}

TimelineExportManager::~TimelineExportManager() {
    if (m_exportThread) {
        m_cancelRequested = true;
        m_exportThread->wait();
    }
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

    m_cancelRequested = false;

    m_exportThread = QThread::create([this, config]() -> void { runExport(config); });
    connect(m_exportThread, &QThread::finished, m_exportThread, &QObject::deleteLater);
    m_exportThread->start();
    return true;
}

void TimelineExportManager::cancelExport() { m_cancelRequested = true; }

void TimelineExportManager::runExport(const AviQtl::Core::VideoEncoder::Config &config) {
    m_exporting = true;

    QPointer<QQuickItem> view = m_controller->compositeView();
    QPointer<QQuickItem> targetItem = captureTargetForView(view);
    auto exportingGuard = qScopeGuard([this]() -> void { m_exporting = false; });

    if (!targetItem) {
        emit exportFinished(false, tr("Frame capture error: no preview view is available"));
        return;
    }

    if (view) {
        QMetaObject::invokeMethod(view.data(), [v = view.data()]() -> void { v->setProperty("exportMode", true); }, Qt::BlockingQueuedConnection);
    }
    ExportModeGuard exportModeGuard(view);

    AviQtl::Core::VideoEncoder encoder;
    if (!encoder.open(config)) {
        emit exportFinished(false, tr("Encoder error: initialization failed"));
        return;
    }

    const AviQtl::Core::SettingsManager &settings = AviQtl::Core::SettingsManager::instance();
    const int sr = std::max(1, settings.value(QStringLiteral("defaultProjectSampleRate"), AviQtl::kDefaultSampleRate).toInt());
    // AudioMixer currently produces stereo interleaved samples regardless of the
    // playback-device channel layout. Keep the export stream consistent with it.
    if (!encoder.addAudioStream(sr, kExportAudioChannels)) {
        encoder.close();
        QFile::remove(config.outputUrl);
        emit exportFinished(false, tr("Encoder error: audio stream initialization failed"));
        return;
    }

    const double fps = m_controller->project()->fps();
    const int startFrame = config.startFrame;
    const int endFrame = config.endFrame >= 0 ? config.endFrame : m_controller->timelineDuration();
    const int totalFrames = endFrame - startFrame;

    emit exportStarted(totalFrames);

    const int grabTimeout = settings.value(QStringLiteral("exportFrameGrabTimeoutMs"), kDefaultGrabTimeoutMs).toInt();
    const int progInterval = std::max(1, settings.value(QStringLiteral("exportProgressInterval"), 5).toInt());

    // Accumulate audio sample time with integer numerators to avoid drift.
    int64_t audioSampleAccumulator = 0;
    const int64_t audioSampleNum = sr;

    QElapsedTimer timer;
    timer.start();
    qint64 elapsedMs = 0;

    for (int frame = startFrame; frame < endFrame; ++frame) {
        if (m_cancelRequested.load()) {
            encoder.close();
            QFile::remove(config.outputUrl);
            emit exportFinished(false, tr("Export cancelled"));
            return;
        }

        QMetaObject::invokeMethod(m_controller->transport(), [this, frame]() -> void { m_controller->transport()->setCurrentFrame(frame); }, Qt::BlockingQueuedConnection);

        // Give the render thread a moment to produce the new frame before grabbing.
        QThread::msleep(kFrameSettleWaitMs);

        QImage img = grabFrame(targetItem, QSize(config.width, config.height), grabTimeout);
        if (img.isNull()) {
            qWarning() << "Frame grab failed for frame" << frame;
            encoder.close();
            QFile::remove(config.outputUrl);
            emit exportFinished(false, tr("Frame capture error: failed to capture frame %1").arg(frame));
            return;
        }

        if (!encoder.pushFrame(img, frame - startFrame)) {
            encoder.close();
            QFile::remove(config.outputUrl);
            emit exportFinished(false, tr("Encoder error: failed to queue video frame %1").arg(frame));
            return;
        }

        // Calculate audio samples for this frame using fractional accumulation to prevent A/V drift.
        // Keep the denominator as a scaled integer to preserve fractional frame rates.
        const int64_t scaledFpsDen = static_cast<int64_t>(fps * 1000.0);
        const int64_t nextAudioSample = ((frame - startFrame + 1LL) * audioSampleNum * 1000LL) / scaledFpsDen;
        const int samplesNeeded = static_cast<int>(nextAudioSample - audioSampleAccumulator);
        audioSampleAccumulator = nextAudioSample;

        if (samplesNeeded > 0) {
            const auto &audio = m_controller->mediaManager()->audioMixer()->mix(frame, fps, samplesNeeded);
            if (!encoder.pushAudio(audio.data(), static_cast<int>(audio.size()))) {
                encoder.close();
                QFile::remove(config.outputUrl);
                emit exportFinished(false, tr("Encoder error: failed to queue audio for frame %1").arg(frame));
                return;
            }
        }

        const int done = frame - startFrame + 1;
        if (done % progInterval == 0 || done == totalFrames) {
            elapsedMs = timer.elapsed();
            const double msPerFrame = static_cast<double>(elapsedMs) / done;
            const int remainingFrames = totalFrames - done;
            const int etaSeconds = static_cast<int>((msPerFrame * remainingFrames) / 1000.0);
            emit exportProgressChanged(done * 100 / totalFrames, done, totalFrames, etaSeconds);
        }
    }

    encoder.close();
    emit exportFinished(true, tr("Export complete"));
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

    m_exporting = true;
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
    if (m_frameGrabber) {
        return m_frameGrabber(size, timeoutMs);
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

void TimelineExportManager::runImageSequenceExport(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    m_exporting = true;
    auto exportingGuard = qScopeGuard([this]() -> void { m_exporting = false; });

    QDir outputDir(dir);
    const int totalFrames = endFrame - startFrame;
    const int padDigits = static_cast<int>(QString::number(endFrame).length());
    const QString extension = (format == QStringLiteral("JPEG")) ? QStringLiteral(".jpg") : QStringLiteral(".png");
    const QByteArray imageFormat = (format == QStringLiteral("JPEG")) ? "JPEG" : "PNG";
    const int saveQuality = quality;

    emit exportStarted(totalFrames);

    QPointer<QQuickItem> view = m_controller->compositeView();
    QPointer<QQuickItem> targetItem = captureTargetForView(view);

    if (!targetItem) {
        emit exportFinished(false, tr("Frame capture error: no preview view is available"));
        return;
    }

    const bool createdOutputDir = !outputDir.exists();
    if (createdOutputDir) {
        if (!outputDir.mkpath(QStringLiteral("."))) {
            emit exportFinished(false, tr("Output error: cannot create output directory"));
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

    for (int frame = startFrame; frame < endFrame; ++frame) {
        const QString filePath = outputDir.filePath(imageSequenceFileName(frame, padDigits, extension));
        if (QFile::exists(filePath)) {
            cleanupPartialOutput();
            emit exportFinished(false, tr("Output error: output file already exists: %1").arg(filePath));
            return;
        }
    }

    if (view) {
        QMetaObject::invokeMethod(view.data(), [v = view.data()]() -> void { v->setProperty("exportMode", true); }, Qt::BlockingQueuedConnection);
    }
    ExportModeGuard exportModeGuard(view);

    const AviQtl::Core::SettingsManager &settings = AviQtl::Core::SettingsManager::instance();
    const int grabTimeout = settings.value(QStringLiteral("exportFrameGrabTimeoutMs"), kDefaultGrabTimeoutMs).toInt();
    const int progInterval = std::max(1, settings.value(QStringLiteral("exportProgressInterval"), 5).toInt());

    QElapsedTimer timer;
    timer.start();
    qint64 elapsedMs = 0;

    for (int frame = startFrame; frame < endFrame; ++frame) {
        if (m_cancelRequested.load()) {
            cleanupPartialOutput();
            emit exportFinished(false, tr("Export cancelled"));
            return;
        }

        QMetaObject::invokeMethod(m_controller->transport(), [this, frame]() -> void { m_controller->transport()->setCurrentFrame(frame); }, Qt::BlockingQueuedConnection);

        // Give the render thread a moment to produce the new frame before grabbing.
        QThread::msleep(kFrameSettleWaitMs);

        QImage img = grabFrame(targetItem, QSize(), grabTimeout);

        if (img.isNull()) {
            qWarning() << "Frame grab failed for frame" << frame;
            cleanupPartialOutput();
            emit exportFinished(false, tr("Frame capture error: failed to capture frame %1").arg(frame));
            return;
        }

        {
            const QString filename = imageSequenceFileName(frame, padDigits, extension);
            const QString filePath = outputDir.filePath(filename);
            if (!img.save(filePath, imageFormat, saveQuality)) {
                qWarning() << "Failed to save frame" << frame << "to" << filePath;
                QFile::remove(filePath);
                cleanupPartialOutput();
                emit exportFinished(false, tr("Output error: failed to save frame %1").arg(frame));
                return;
            }
            writtenFiles.push_back(filePath);
        }

        const int done = frame - startFrame + 1;
        if (done % progInterval == 0 || done == totalFrames) {
            elapsedMs = timer.elapsed();
            const double msPerFrame = static_cast<double>(elapsedMs) / done;
            const int remainingFrames = totalFrames - done;
            const int etaSeconds = static_cast<int>((msPerFrame * remainingFrames) / 1000.0);
            emit exportProgressChanged(done * 100 / totalFrames, done, totalFrames, etaSeconds);
        }
    }

    emit exportFinished(true, tr("Export complete"));
}

} // namespace AviQtl::UI
