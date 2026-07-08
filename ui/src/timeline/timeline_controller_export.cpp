#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "video_encoder.hpp"
#include <cmath>

namespace AviQtl::UI {

namespace {
QString configurationError(const QString &reason) { return QObject::tr("Configuration error: %1").arg(reason); }

int resolvedEndFrame(int requestedEndFrame, int timelineDuration) { return requestedEndFrame >= 0 ? requestedEndFrame : timelineDuration; }
} // namespace

QStringList TimelineController::availableVideoEncoders() const {
    return AviQtl::Core::VideoEncoder::availableVideoEncoders();
}

QStringList TimelineController::availableAudioEncoders() const {
    return AviQtl::Core::VideoEncoder::availableAudioEncoders();
}

void TimelineController::exportVideoAsync(const QVariantMap &cfg) {
    AviQtl::Core::VideoEncoder::Config c;
    c.width = cfg.value(QStringLiteral("width"), AviQtl::kDefaultWidth).toInt();
    c.height = cfg.value(QStringLiteral("height"), AviQtl::kDefaultHeight).toInt();
    c.fps_num = cfg.value(QStringLiteral("fps_num"), 60000).toInt();
    c.fps_den = cfg.value(QStringLiteral("fps_den"), 1000).toInt();
    c.bitrate = cfg.value(QStringLiteral("bitrate"), 15'000'000).toLongLong();
    c.crf = cfg.value(QStringLiteral("crf"), -1).toInt();
    c.codecName = cfg.value(QStringLiteral("codecName"), "libx264").toString();
    c.audioCodecName = cfg.value(QStringLiteral("audioCodecName"), "aac").toString();
    c.audioBitrate = cfg.value(QStringLiteral("audioBitrate"), 192'000).toLongLong();
    c.outputUrl = cfg.value(QStringLiteral("outputUrl")).toString();
    c.startFrame = cfg.value(QStringLiteral("startFrame"), 0).toInt();
    c.endFrame = cfg.value(QStringLiteral("endFrame"), -1).toInt();
    c.preset = cfg.value(QStringLiteral("preset")).toString();
    c.profile = cfg.value(QStringLiteral("profile")).toString();

    if (c.outputUrl.trimmed().isEmpty()) {
        emit exportFinished(false, configurationError(tr("missing output path")));
        return;
    }
    if (c.width <= 0 || c.height <= 0) {
        emit exportFinished(false, configurationError(tr("invalid output size")));
        return;
    }
    if (c.fps_num <= 0 || c.fps_den <= 0) {
        emit exportFinished(false, configurationError(tr("invalid FPS")));
        return;
    }

    c.endFrame = resolvedEndFrame(c.endFrame, timelineDuration());
    if (c.startFrame < 0 || c.endFrame <= c.startFrame) {
        emit exportFinished(false, configurationError(tr("export end frame must be after start frame")));
        return;
    }

    const double projectFps = project()->fps();
    if (projectFps <= 0.0 || std::abs((c.fps_num / static_cast<double>(c.fps_den)) - projectFps) > 0.001) {
        emit exportFinished(false, configurationError(tr("export FPS does not match project FPS")));
        return;
    }

    m_exportManager->exportVideoAsync(c);
}

void TimelineController::exportImageSequence(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    if (dir.trimmed().isEmpty()) {
        emit exportFinished(false, configurationError(tr("missing output path")));
        return;
    }

    const int resolvedEnd = resolvedEndFrame(endFrame, timelineDuration());
    if (startFrame < 0 || resolvedEnd <= startFrame) {
        emit exportFinished(false, configurationError(tr("export end frame must be after start frame")));
        return;
    }

    if (!m_exportManager->exportImageSequence(dir, quality, format, startFrame, resolvedEnd)) {
        emit exportFinished(false, configurationError(tr("export is already running")));
    }
}

void TimelineController::cancelExport() { m_exportManager->cancelExport(); }
auto TimelineController::isExporting() const -> bool { return m_exportManager->isExporting(); }

} // namespace AviQtl::UI
