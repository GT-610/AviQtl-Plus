#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"

namespace AviQtl::UI {

void TimelineController::exportVideoAsync(const QVariantMap &cfg) {
    AviQtl::Core::VideoEncoder::Config c;
    c.width = cfg.value(QStringLiteral("width"), 1920).toInt();
    c.height = cfg.value(QStringLiteral("height"), 1080).toInt();
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

    if (c.outputUrl.isEmpty() || c.width <= 0 || c.height <= 0 || c.fps_den <= 0) {
        emit exportFinished(false, tr("Invalid export configuration"));
        return;
    }
    if (c.endFrame >= 0 && c.endFrame <= c.startFrame) {
        emit exportFinished(false, tr("Export end frame must be after start frame"));
        return;
    }

    m_exportManager->exportVideoAsync(c);
}

void TimelineController::exportImageSequence(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    if (dir.isEmpty()) {
        emit exportFinished(false, tr("Invalid export configuration"));
        return;
    }
    if (!m_exportManager->exportImageSequence(dir, quality, format, startFrame, endFrame)) {
        emit exportFinished(false, tr("Failed to start image sequence export"));
    }
}

void TimelineController::cancelExport() { m_exportManager->cancelExport(); }
auto TimelineController::isExporting() const -> bool { return m_exportManager->isExporting(); }

} // namespace AviQtl::UI