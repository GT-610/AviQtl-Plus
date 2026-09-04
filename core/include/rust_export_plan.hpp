#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QString>
#include <QStringView>
#include <cstdint>

namespace AviQtl::RustCore::Export {

enum class ConfigurationError : std::uint32_t {
    None = AVIQTL_EXPORT_CONFIGURATION_OK,
    MissingOutputPath = AVIQTL_EXPORT_CONFIGURATION_MISSING_OUTPUT_PATH,
    InvalidOutputSize = AVIQTL_EXPORT_CONFIGURATION_INVALID_OUTPUT_SIZE,
    InvalidFps = AVIQTL_EXPORT_CONFIGURATION_INVALID_FPS,
    InvalidRange = AVIQTL_EXPORT_CONFIGURATION_INVALID_RANGE,
    ProjectFpsMismatch = AVIQTL_EXPORT_CONFIGURATION_PROJECT_FPS_MISMATCH,
};

enum class ImageFormat : std::uint32_t {
    Png = AVIQTL_EXPORT_IMAGE_FORMAT_PNG,
    Jpeg = AVIQTL_EXPORT_IMAGE_FORMAT_JPEG,
};

enum class CodecBackend : std::uint32_t {
    Software = AVIQTL_EXPORT_CODEC_BACKEND_SOFTWARE,
    Cuda = AVIQTL_EXPORT_CODEC_BACKEND_CUDA,
    Vaapi = AVIQTL_EXPORT_CODEC_BACKEND_VAAPI,
    Qsv = AVIQTL_EXPORT_CODEC_BACKEND_QSV,
    D3d11va = AVIQTL_EXPORT_CODEC_BACKEND_D3D11VA,
    Dxva2 = AVIQTL_EXPORT_CODEC_BACKEND_DXVA2,
    VideoToolbox = AVIQTL_EXPORT_CODEC_BACKEND_VIDEOTOOLBOX,
    Amf = AVIQTL_EXPORT_CODEC_BACKEND_AMF,
};

enum class FixedGopMode : std::uint32_t {
    None = AVIQTL_EXPORT_FIXED_GOP_NONE,
    X264 = AVIQTL_EXPORT_FIXED_GOP_X264,
    X265 = AVIQTL_EXPORT_FIXED_GOP_X265,
    Nvenc = AVIQTL_EXPORT_FIXED_GOP_NVENC,
};

struct VideoDefaults {
    AviQtlExportVideoDefaults values;
    QString videoCodec;
    QString audioCodec;
};

inline QString staticString(const std::uint8_t *(*function)(std::size_t *)) {
    std::size_t length = 0;
    const std::uint8_t *value = function(&length);
    return value == nullptr
               ? QString()
               : QString::fromUtf8(reinterpret_cast<const char *>(value),
                                   static_cast<qsizetype>(length));
}

inline VideoDefaults videoDefaults() {
    AviQtlExportVideoDefaults values{};
    aviqtl_export_video_defaults(&values);
    return VideoDefaults{
        values,
        staticString(aviqtl_export_default_video_codec),
        staticString(aviqtl_export_default_audio_codec),
    };
}

inline AviQtlExportVideoPlan planVideo(int width, int height, int fpsNum, int fpsDen,
                                      int startFrame, int endFrame, int timelineDuration,
                                      bool outputPathPresent, double projectFps) {
    const AviQtlExportVideoRequest request{
        width,
        height,
        fpsNum,
        fpsDen,
        startFrame,
        endFrame,
        timelineDuration,
        outputPathPresent ? 1U : 0U,
        projectFps,
    };
    AviQtlExportVideoPlan output{};
    if (aviqtl_export_plan_video(&request, &output) != AVIQTL_RUST_CORE_STATUS_OK)
        output.error = AVIQTL_EXPORT_CONFIGURATION_INVALID_FPS;
    return output;
}

inline AviQtlExportImageSequencePlan planImageSequence(
    int startFrame, int endFrame, int timelineDuration, int configuredPadding,
    bool outputPathPresent, QStringView format) {
    const AviQtlExportImageSequenceRequest request{
        startFrame,
        endFrame,
        timelineDuration,
        configuredPadding,
        outputPathPresent ? 1U : 0U,
    };
    const QByteArray encodedFormat = format.toString().toUtf8();
    AviQtlExportImageSequencePlan output{};
    if (aviqtl_export_plan_image_sequence(
            &request, reinterpret_cast<const std::uint8_t *>(encodedFormat.constData()),
            static_cast<std::size_t>(encodedFormat.size()), &output) !=
        AVIQTL_RUST_CORE_STATUS_OK) {
        output.error = AVIQTL_EXPORT_CONFIGURATION_INVALID_RANGE;
    }
    return output;
}

inline AviQtlExportAudioFramePlan planAudioFrame(int frameIndex, int sampleRate, int fpsNum,
                                                 int fpsDen) {
    AviQtlExportAudioFramePlan output{};
    aviqtl_export_plan_audio_frame(frameIndex, sampleRate, fpsNum, fpsDen, &output);
    return output;
}

inline AviQtlExportProgressPlan planProgress(int done, int totalFrames, int interval,
                                             std::int64_t elapsedMs) {
    AviQtlExportProgressPlan output{};
    aviqtl_export_plan_progress(done, totalFrames, interval, elapsedMs, &output);
    return output;
}

inline QByteArray utf8(QStringView value) { return value.toString().toUtf8(); }

inline CodecBackend codecBackend(QStringView codecName) {
    const QByteArray encoded = utf8(codecName);
    return static_cast<CodecBackend>(aviqtl_export_codec_backend(
        reinterpret_cast<const std::uint8_t *>(encoded.constData()),
        static_cast<std::size_t>(encoded.size())));
}

inline QString fallbackCodec(QStringView codecName) {
    const QByteArray encoded = utf8(codecName);
    std::size_t length = 0;
    const std::uint8_t *fallback = aviqtl_export_codec_fallback(
        reinterpret_cast<const std::uint8_t *>(encoded.constData()),
        static_cast<std::size_t>(encoded.size()), &length);
    return fallback == nullptr
               ? codecName.toString()
               : QString::fromUtf8(reinterpret_cast<const char *>(fallback),
                                   static_cast<qsizetype>(length));
}

inline FixedGopMode fixedGopMode(QStringView codecName) {
    const QByteArray encoded = utf8(codecName);
    return static_cast<FixedGopMode>(aviqtl_export_fixed_gop_mode(
        reinterpret_cast<const std::uint8_t *>(encoded.constData()),
        static_cast<std::size_t>(encoded.size())));
}

inline std::size_t encoderQueueSize(int width, int height, int budgetMb) {
    return aviqtl_export_encoder_queue_size(width, height, budgetMb);
}

inline QString imageExtension(ImageFormat format) {
    return format == ImageFormat::Jpeg ? QStringLiteral(".jpg") : QStringLiteral(".png");
}

inline QByteArray imageEncoderName(ImageFormat format) {
    return format == ImageFormat::Jpeg ? QByteArrayLiteral("JPEG") : QByteArrayLiteral("PNG");
}

} // namespace AviQtl::RustCore::Export
