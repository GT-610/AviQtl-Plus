#include "rust_export_plan.hpp"
#include <QTest>

using namespace AviQtl::RustCore::Export;

class TestRustExportPlan : public QObject {
    Q_OBJECT

  private slots:
    void exposesAuthoritativeDefaults() {
        const auto defaults = videoDefaults();
        QCOMPARE(defaults.values.width, 1920);
        QCOMPARE(defaults.values.height, 1080);
        QCOMPARE(defaults.values.fps_num, 60'000);
        QCOMPARE(defaults.values.fps_den, 1'000);
        QCOMPARE(defaults.values.bitrate, std::int64_t{15'000'000});
        QCOMPARE(defaults.values.crf, -1);
        QCOMPARE(defaults.values.audio_bitrate, std::int64_t{192'000});
        QCOMPARE(defaults.values.start_frame, 0);
        QCOMPARE(defaults.values.end_frame, -1);
        QCOMPARE(defaults.videoCodec, QStringLiteral("libx264"));
        QCOMPARE(defaults.audioCodec, QStringLiteral("aac"));
    }

    void plansVideoConfiguration() {
        const double fps = 60'000.0 / 1'001.0;
        auto plan = planVideo(1920, 1080, 60'000, 1'001, 10, -1, 110, true, fps);
        QCOMPARE(static_cast<ConfigurationError>(plan.error), ConfigurationError::None);
        QCOMPARE(plan.start_frame, 10);
        QCOMPARE(plan.end_frame, 110);
        QCOMPARE(plan.total_frames, 100);

        plan = planVideo(1920, 1080, 60'000, 1'001, 10, -1, 110, false, fps);
        QCOMPARE(static_cast<ConfigurationError>(plan.error),
                 ConfigurationError::MissingOutputPath);
        plan = planVideo(0, 1080, 60'000, 1'001, 10, -1, 110, true, fps);
        QCOMPARE(static_cast<ConfigurationError>(plan.error),
                 ConfigurationError::InvalidOutputSize);
        plan = planVideo(1920, 1080, 60'000, 0, 10, -1, 110, true, fps);
        QCOMPARE(static_cast<ConfigurationError>(plan.error), ConfigurationError::InvalidFps);
        plan = planVideo(1920, 1080, 60'000, 1'001, 110, -1, 110, true, fps);
        QCOMPARE(static_cast<ConfigurationError>(plan.error), ConfigurationError::InvalidRange);
        plan = planVideo(1920, 1080, 60'000, 1'001, 10, -1, 110, true, 24.0);
        QCOMPARE(static_cast<ConfigurationError>(plan.error),
                 ConfigurationError::ProjectFpsMismatch);
    }

    void plansImageSequence() {
        auto plan = planImageSequence(3, -1, 12'345, 4, true, QStringLiteral("JPEG"));
        QCOMPARE(static_cast<ConfigurationError>(plan.error), ConfigurationError::None);
        QCOMPARE(plan.end_frame, 12'345);
        QCOMPARE(plan.total_frames, 12'342);
        QCOMPARE(plan.pad_digits, 5);
        QCOMPARE(static_cast<ImageFormat>(plan.image_format), ImageFormat::Jpeg);
        QCOMPARE(imageExtension(ImageFormat::Jpeg), QStringLiteral(".jpg"));
        QCOMPARE(imageEncoderName(ImageFormat::Jpeg), QByteArrayLiteral("JPEG"));

        plan = planImageSequence(3, 12'345, 12'345, 99, true, QStringLiteral("jpeg"));
        QCOMPARE(plan.pad_digits, 10);
        QCOMPARE(static_cast<ImageFormat>(plan.image_format), ImageFormat::Png);
    }

    void plansExactAudioSamples() {
        AviQtlExportAudioFramePlan first{};
        QCOMPARE(planAudioFrame(0, 48'000, 60'000, 1'001, first), Status::Ok);
        QCOMPARE(first.samples_for_frame, 800);
        QCOMPARE(first.cumulative_samples, std::int64_t{800});

        AviQtlExportAudioFramePlan second{};
        QCOMPARE(planAudioFrame(1, 48'000, 60'000, 1'001, second), Status::Ok);
        QCOMPARE(second.samples_for_frame, 801);
        QCOMPARE(second.cumulative_samples, std::int64_t{1'601});

        AviQtlExportAudioFramePlan oneMinute{};
        QCOMPARE(planAudioFrame(3'595, 48'000, 60'000, 1'001, oneMinute), Status::Ok);
        QCOMPARE(oneMinute.cumulative_samples, std::int64_t{2'879'676});

        AviQtlExportAudioFramePlan invalid{.cumulative_samples = 9,
                                           .samples_for_frame = 8,
                                           .reserved = 7};
        QCOMPARE(planAudioFrame(-1, 48'000, 60'000, 1'001, invalid),
                 Status::InvalidArgument);
        QCOMPARE(invalid.cumulative_samples, std::int64_t{9});
        QCOMPARE(invalid.samples_for_frame, 8);
        QCOMPARE(invalid.reserved, std::uint32_t{7});
    }

    void plansProgressUpdates() {
        auto plan = planProgress(5, 12, 5, 2'500);
        QCOMPARE(plan.should_emit, std::uint32_t{1});
        QCOMPARE(plan.progress, 41);
        QCOMPARE(plan.eta_seconds, 3);

        plan = planProgress(6, 12, 5, 3'000);
        QCOMPARE(plan.should_emit, std::uint32_t{0});
        plan = planProgress(12, 12, 5, 6'000);
        QCOMPARE(plan.should_emit, std::uint32_t{1});
        QCOMPARE(plan.progress, 100);
        QCOMPARE(plan.eta_seconds, 0);
    }

    void classifiesCodecsAndQueueBudget() {
        QCOMPARE(codecBackend(QStringLiteral("h264_nvenc")), CodecBackend::Cuda);
        QCOMPARE(codecBackend(QStringLiteral("hevc_videotoolbox")),
                 CodecBackend::VideoToolbox);
        QCOMPARE(codecBackend(QStringLiteral("h264_amf")), CodecBackend::Amf);
        QCOMPARE(codecBackend(QStringLiteral("libx264")), CodecBackend::Software);
        QCOMPARE(fallbackCodec(QStringLiteral("hevc_qsv")), QStringLiteral("libx265"));
        QCOMPARE(fallbackCodec(QStringLiteral("libx264")), QStringLiteral("libx264"));
        QCOMPARE(fixedGopMode(QStringLiteral("libx264")), FixedGopMode::X264);
        QCOMPARE(fixedGopMode(QStringLiteral("h264_nvenc")), FixedGopMode::Nvenc);
        QCOMPARE(fixedGopMode(QStringLiteral("h264_vaapi")), FixedGopMode::None);
        QCOMPARE(encoderQueueSize(1920, 1080, 128), std::size_t{16});
        QCOMPARE(encoderQueueSize(7680, 4320, 16), std::size_t{2});
    }
};

QTEST_MAIN(TestRustExportPlan)
#include "test_rust_export_plan.moc"
