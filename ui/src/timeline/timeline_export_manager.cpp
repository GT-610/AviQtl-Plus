#include "timeline_export_manager.hpp"
#include "engine/audio_mixer.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "video_encoder.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QEventLoop>
#include <QImage>
#include <QPointer>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QScopeGuard>
#include <QTimer>
#include <algorithm>

namespace {
constexpr int kDefaultGrabTimeoutMs = 2000;
constexpr int kFrameSettleWaitMs = 8;

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

TimelineExportManager::~TimelineExportManager() {
    if (m_exportThread) {
        m_cancelRequested = true;
        m_exportThread->wait();
    }
}

void TimelineExportManager::exportVideoAsync(const AviQtl::Core::VideoEncoder::Config &config) {
    if (m_exporting.load()) {
        return;
    }
    m_cancelRequested = false;

    m_exportThread = QThread::create([this, config]() -> void { runExport(config); });
    connect(m_exportThread, &QThread::finished, m_exportThread, &QObject::deleteLater);
    m_exportThread->start();
}

void TimelineExportManager::cancelExport() { m_cancelRequested = true; }

void TimelineExportManager::runExport(const AviQtl::Core::VideoEncoder::Config &config) {
    m_exporting = true;

    QPointer<QQuickItem> view = m_controller->compositeView();
    auto exportingGuard = qScopeGuard([this]() -> void { m_exporting = false; });

    if (view) {
        QMetaObject::invokeMethod(view.data(), [v = view.data()]() -> void { v->setProperty("exportMode", true); }, Qt::BlockingQueuedConnection);
    }
    ExportModeGuard exportModeGuard(view);

    AviQtl::Core::VideoEncoder encoder;
    if (!encoder.open(config)) {
        emit exportFinished(false, tr("Encoder initialization failed"));
        return;
    }

    const AviQtl::Core::SettingsManager &settings = AviQtl::Core::SettingsManager::instance();
    const int sr = std::max(1, settings.value(QStringLiteral("defaultProjectSampleRate"), 48000).toInt());
    const int ch = std::max(1, settings.value(QStringLiteral("audioChannels"), 2).toInt());
    encoder.addAudioStream(sr, ch);

    const double fps = m_controller->project()->fps();
    const int startFrame = config.startFrame;
    const int endFrame = config.endFrame >= 0 ? config.endFrame : m_controller->timelineDuration();
    const int totalFrames = endFrame - startFrame;

    emit exportStarted(totalFrames);

    QPointer<QQuickItem> targetItem;
    if (view) {
        if (auto *v3d = view->property("view3D").value<QQuickItem *>()) {
            targetItem = v3d;
        } else {
            targetItem = view.data();
        }
    }

    if (!targetItem) {
        emit exportFinished(false, tr("Failed to capture frame %1").arg(startFrame));
        return;
    }

    const int grabTimeout = settings.value(QStringLiteral("exportFrameGrabTimeoutMs"), kDefaultGrabTimeoutMs).toInt();
    const int progInterval = settings.value(QStringLiteral("exportProgressInterval"), 5).toInt();

    // Accumulate audio sample time with integer numerators to avoid drift.
    int64_t audioSampleAccumulator = 0;
    const int64_t audioSampleNum = sr;
    const int64_t audioSampleDen = qRound(fps);

    for (int frame = startFrame; frame < endFrame; ++frame) {
        if (m_cancelRequested.load()) {
            emit exportFinished(false, tr("Cancelled"));
            return;
        }

        QMetaObject::invokeMethod(m_controller->transport(), [this, frame]() -> void { m_controller->transport()->setCurrentFrame(frame); }, Qt::BlockingQueuedConnection);

        // Give the render thread a moment to produce the new frame before grabbing.
        QThread::msleep(kFrameSettleWaitMs);

        QImage img = grabFrame(targetItem, QSize(config.width, config.height), grabTimeout);
        if (img.isNull()) {
            qWarning() << "Frame grab returned null image for frame" << frame << "- substituting black frame";
            img = QImage(config.width, config.height, QImage::Format_RGBA8888);
            img.fill(Qt::black);
        }

        encoder.pushFrame(img, frame - startFrame);

        // Calculate audio samples for this frame using fractional accumulation to prevent A/V drift.
        const int64_t nextAudioSample = ((frame - startFrame + 1LL) * audioSampleNum) / audioSampleDen;
        const int samplesNeeded = static_cast<int>(nextAudioSample - audioSampleAccumulator);
        audioSampleAccumulator = nextAudioSample;

        if (samplesNeeded > 0) {
            auto audio = m_controller->mediaManager()->audioMixer()->mix(frame, fps, samplesNeeded);
            encoder.pushAudio(audio.data(), samplesNeeded);
        }

        const int done = frame - startFrame + 1;
        if (done % progInterval == 0 || done == totalFrames) {
            emit exportProgressChanged(done * 100 / totalFrames, done, totalFrames);
        }
    }

    encoder.close();
    emit exportFinished(true, tr("Export complete"));
}

bool TimelineExportManager::exportImageSequence(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    if (m_exporting.load()) {
        return false;
    }

    QDir outputDir(dir);
    if (!outputDir.exists() && !outputDir.mkpath(QStringLiteral("."))) {
        emit exportFinished(false, tr("Cannot create output directory"));
        return false;
    }

    if (endFrame < 0) {
        endFrame = m_controller->timelineDuration();
    }
    startFrame = std::max(0, startFrame);
    if (startFrame >= endFrame) {
        return false;
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
    QDir outputDir(dir);
    const int totalFrames = endFrame - startFrame;
    const int padDigits = static_cast<int>(QString::number(endFrame).length());

    emit exportStarted(totalFrames);

    QPointer<QQuickItem> view = m_controller->compositeView();
    QPointer<QQuickItem> targetItem;
    if (view) {
        if (auto *v3d = view->property("view3D").value<QQuickItem *>()) {
            targetItem = v3d;
        } else {
            targetItem = view.data();
        }
    }

    if (!targetItem) {
        emit exportFinished(false, tr("Failed to capture frame %1").arg(startFrame));
        return;
    }

    if (view) {
        QMetaObject::invokeMethod(view.data(), [v = view.data()]() -> void { v->setProperty("exportMode", true); }, Qt::BlockingQueuedConnection);
    }
    ExportModeGuard exportModeGuard(view);

    const AviQtl::Core::SettingsManager &settings = AviQtl::Core::SettingsManager::instance();
    const int grabTimeout = settings.value(QStringLiteral("exportFrameGrabTimeoutMs"), kDefaultGrabTimeoutMs).toInt();
    const int progInterval = settings.value(QStringLiteral("exportProgressInterval"), 5).toInt();

    for (int frame = startFrame; frame < endFrame; ++frame) {
        if (m_cancelRequested.load()) {
            emit exportFinished(false, tr("Cancelled"));
            return;
        }

        QMetaObject::invokeMethod(m_controller->transport(), [this, frame]() -> void { m_controller->transport()->setCurrentFrame(frame); }, Qt::BlockingQueuedConnection);

        // Give the render thread a moment to produce the new frame before grabbing.
        QThread::msleep(kFrameSettleWaitMs);

        QImage img = grabFrame(targetItem, QSize(), grabTimeout);

        if (img.isNull()) {
            qWarning() << "Frame grab returned null image for frame" << frame << "- substituting black frame";
            img = QImage(static_cast<int>(targetItem->width()), static_cast<int>(targetItem->height()), QImage::Format_RGBA8888);
            img.fill(Qt::black);
        }

        {
            const QString ext = (format == QStringLiteral("JPEG")) ? QStringLiteral(".jpg") : QStringLiteral(".png");
            const QByteArray fmt = (format == QStringLiteral("JPEG")) ? "JPEG" : "PNG";
            const int saveQuality = (format == QStringLiteral("JPEG")) ? quality : std::clamp(quality / 11, 0, 9);
            const QString filename = QStringLiteral("frame_") + QString::number(frame).rightJustified(padDigits, QChar::fromLatin1('0')) + ext;
            const QString filePath = outputDir.filePath(filename);
            if (!img.save(filePath, fmt, saveQuality)) {
                qWarning() << "Failed to save frame" << frame << "to" << filePath;
                emit exportFinished(false, tr("Failed to save frame %1").arg(frame));
                return;
            }
        }

        const int done = frame - startFrame + 1;
        if (done % progInterval == 0 || done == totalFrames) {
            emit exportProgressChanged(done * 100 / totalFrames, done, totalFrames);
        }
    }

    emit exportFinished(true, tr("Export complete"));
}

} // namespace AviQtl::UI
