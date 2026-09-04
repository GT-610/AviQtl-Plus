#include "rust_export_plan.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "video_encoder.hpp"

namespace AviQtl::UI {

namespace {
QString configurationError(const QString &reason) { return QObject::tr("Configuration error: %1").arg(reason); }

QString configurationReason(AviQtl::RustCore::Export::ConfigurationError error) {
    using Error = AviQtl::RustCore::Export::ConfigurationError;
    switch (error) {
    case Error::MissingOutputPath:
        return AviQtl::UI::TimelineController::tr("missing output path");
    case Error::InvalidOutputSize:
        return AviQtl::UI::TimelineController::tr("invalid output size");
    case Error::InvalidFps:
        return AviQtl::UI::TimelineController::tr("invalid FPS");
    case Error::InvalidRange:
        return AviQtl::UI::TimelineController::tr("export end frame must be after start frame");
    case Error::ProjectFpsMismatch:
        return AviQtl::UI::TimelineController::tr("export FPS does not match project FPS");
    case Error::None:
        break;
    }
    return {};
}
} // namespace

QStringList TimelineController::availableVideoEncoders() const {
    return AviQtl::Core::VideoEncoder::availableVideoEncoders();
}

QStringList TimelineController::availableAudioEncoders() const {
    return AviQtl::Core::VideoEncoder::availableAudioEncoders();
}

void TimelineController::exportVideoAsync(const QVariantMap &cfg) {
    AviQtl::Core::VideoEncoder::Config c;
    c.width = cfg.value(QStringLiteral("width"), c.width).toInt();
    c.height = cfg.value(QStringLiteral("height"), c.height).toInt();
    c.fps_num = cfg.value(QStringLiteral("fps_num"), c.fps_num).toInt();
    c.fps_den = cfg.value(QStringLiteral("fps_den"), c.fps_den).toInt();
    c.bitrate = cfg.value(QStringLiteral("bitrate"), c.bitrate).toLongLong();
    c.crf = cfg.value(QStringLiteral("crf"), c.crf).toInt();
    c.codecName = cfg.value(QStringLiteral("codecName"), c.codecName).toString();
    c.audioCodecName = cfg.value(QStringLiteral("audioCodecName"), c.audioCodecName).toString();
    c.audioBitrate = cfg.value(QStringLiteral("audioBitrate"), c.audioBitrate).toLongLong();
    c.outputUrl = cfg.value(QStringLiteral("outputUrl")).toString();
    c.startFrame = cfg.value(QStringLiteral("startFrame"), c.startFrame).toInt();
    c.endFrame = cfg.value(QStringLiteral("endFrame"), c.endFrame).toInt();
    c.preset = cfg.value(QStringLiteral("preset")).toString();
    c.profile = cfg.value(QStringLiteral("profile")).toString();
    c.gopSize = cfg.value(QStringLiteral("gopSize"), c.gopSize).toInt();

    const auto plan = AviQtl::RustCore::Export::planVideo(
        c.width, c.height, c.fps_num, c.fps_den, c.startFrame, c.endFrame,
        timelineDuration(), !c.outputUrl.trimmed().isEmpty(), project()->fps());
    const auto error = static_cast<AviQtl::RustCore::Export::ConfigurationError>(plan.error);
    if (error != AviQtl::RustCore::Export::ConfigurationError::None) {
        emit exportFinished(false, configurationError(configurationReason(error)));
        return;
    }
    c.startFrame = plan.start_frame;
    c.endFrame = plan.end_frame;

    if (!m_exportManager->exportVideoAsync(c)) {
        emit exportFinished(false, configurationError(tr("export is already running")));
    }
}

void TimelineController::exportImageSequence(const QString &dir, int quality, const QString &format, int startFrame, int endFrame) {
    const auto plan = AviQtl::RustCore::Export::planImageSequence(
        startFrame, endFrame, timelineDuration(), 6, !dir.trimmed().isEmpty(), format);
    const auto error = static_cast<AviQtl::RustCore::Export::ConfigurationError>(plan.error);
    if (error != AviQtl::RustCore::Export::ConfigurationError::None) {
        emit exportFinished(false, configurationError(configurationReason(error)));
        return;
    }

    if (!m_exportManager->exportImageSequence(dir, quality, format, plan.start_frame, plan.end_frame)) {
        emit exportFinished(false, configurationError(tr("export is already running")));
    }
}

void TimelineController::cancelExport() { m_exportManager->cancelExport(); }
auto TimelineController::isExporting() const -> bool { return m_exportManager->isExporting(); }

} // namespace AviQtl::UI
