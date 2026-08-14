#include "keyframe_utils.hpp"
#include "rust_keyframe_adapter.hpp"
#include "rust_keyframe_core.hpp"
#include <QFile>
#include <QTest>
#include <QTextStream>
#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <vector>

using namespace AviQtl::Core::KeyframeUtils;
using namespace AviQtl::RustCore;

namespace {

NumericKeyframe makeKeyframe(std::int32_t frame, double value, NumericInterpolation interpolation) {
    return {
        .frame = frame,
        .interpolation = static_cast<std::uint32_t>(interpolation),
        .step_frames = 1,
        .custom_points_offset = 0,
        .custom_points_length = 0,
        .reserved = 0,
        .value = value,
        .amplitude = 0.8,
        .period = 0.42,
    };
}

QVariantMap makePoint(int frame, double value, const QString &interpolation,
                      int stepFrames = 1) {
    return {
        {QStringLiteral("frame"), frame},
        {QStringLiteral("value"), value},
        {QStringLiteral("interp"), interpolation},
        {QStringLiteral("modeParams"), QVariantMap{{QStringLiteral("stepFrames"), stepFrames}}},
    };
}

} // namespace

class TestKeyframeBatch : public QObject {
    Q_OBJECT

private slots:
    void matchesCppGoldenValuesForEveryEasing() {
        constexpr std::size_t easingCount = 42;
        const std::array<int, 5> sampleFrames = {10, 25, 50, 75, 90};
        std::array<std::array<double, 5>, easingCount> expected{};
        std::array<bool, easingCount> seen{};

        QFile fixture(QString::fromUtf8(AVIQTL_KEYFRAME_EASING_FIXTURE));
        QVERIFY2(fixture.open(QIODevice::ReadOnly | QIODevice::Text),
                 qPrintable(fixture.errorString()));
        QTextStream input(&fixture);
        while (!input.atEnd()) {
            const QString line = input.readLine().trimmed();
            if (line.isEmpty() || line.startsWith(QLatin1Char('#')))
                continue;
            const QStringList fields = line.simplified().split(QLatin1Char(' '), Qt::SkipEmptyParts);
            QCOMPARE(fields.size(), qsizetype{6});
            bool ok = false;
            const int kind = fields[0].toInt(&ok);
            QVERIFY(ok && kind >= 0 && kind < static_cast<int>(easingCount));
            seen[static_cast<std::size_t>(kind)] = true;
            for (std::size_t sample = 0; sample < sampleFrames.size(); ++sample) {
                expected[static_cast<std::size_t>(kind)][sample] =
                    fields[static_cast<qsizetype>(sample + 1)].toDouble(&ok);
                QVERIFY(ok);
            }
        }
        QVERIFY(std::all_of(seen.cbegin(), seen.cend(), [](bool value) { return value; }));

        const std::vector<double> customPoints = {
            0.1, 0.2, 0.4, 0.5, 0.6, 0.7,
            0.7, 0.8, 0.9, 0.95, 1.0, 1.0,
        };
        std::array<std::array<NumericKeyframe, 2>, easingCount> keyframes{};
        std::array<NumericTrackView, easingCount> tracks{};
        for (std::size_t kind = 0; kind < easingCount; ++kind) {
            keyframes[kind] = {
                makeKeyframe(0, 0.0, static_cast<NumericInterpolation>(kind)),
                makeKeyframe(100, 100.0, NumericInterpolation::Linear),
            };
            if (kind == static_cast<std::size_t>(NumericInterpolation::Custom)) {
                keyframes[kind][0].custom_points_length = static_cast<std::uint32_t>(customPoints.size());
            }
            tracks[kind] = {
                .keyframes = keyframes[kind].data(),
                .keyframes_length = keyframes[kind].size(),
                .custom_points = customPoints.data(),
                .custom_points_length = customPoints.size(),
                .fallback_value = -1.0,
            };
        }

        std::array<double, easingCount> output{};
        for (std::size_t sample = 0; sample < sampleFrames.size(); ++sample) {
            QCOMPARE(evaluateNumericTracks(tracks, sampleFrames[sample], output),
                     NumericBatchStatus::Ok);
            for (std::size_t kind = 0; kind < easingCount; ++kind) {
                const double wanted = expected[kind][sample] * 100.0;
                QVERIFY2(std::abs(output[kind] - wanted) < 1e-10,
                         qPrintable(QStringLiteral("kind=%1 frame=%2 actual=%3 expected=%4")
                                        .arg(kind)
                                        .arg(sampleFrames[sample])
                                        .arg(output[kind], 0, 'g', 17)
                                        .arg(wanted, 0, 'g', 17)));
            }
        }
    }

    void matchesCppModesAndEndpointBehavior() {
        struct ModeCase {
            NumericInterpolation interpolation;
            QString name;
            int stepFrames;
        };
        const std::array<ModeCase, 3> cases = {{
            {NumericInterpolation::None, QStringLiteral("none"), 1},
            {NumericInterpolation::Random, QStringLiteral("random"), 5},
            {NumericInterpolation::Alternate, QStringLiteral("alternate"), 5},
        }};
        const std::array<int, 8> frames = {0, 1, 4, 5, 49, 50, 99, 100};

        for (const ModeCase &mode : cases) {
            auto first = makeKeyframe(0, 10.0, mode.interpolation);
            first.step_frames = static_cast<std::uint32_t>(mode.stepFrames);
            const std::array<NumericKeyframe, 2> keys = {
                first,
                makeKeyframe(100, 20.0, NumericInterpolation::Linear),
            };
            const std::array<NumericTrackView, 1> tracks = {{
                {.keyframes = keys.data(),
                 .keyframes_length = keys.size(),
                 .custom_points = nullptr,
                 .custom_points_length = 0,
                 .fallback_value = -1.0},
            }};
            QVariantList cppTrack;
            cppTrack.append(makePoint(0, 10.0, mode.name, mode.stepFrames));
            cppTrack.append(makePoint(100, 20.0, QStringLiteral("linear")));

            for (const int frame : frames) {
                std::array<double, 1> output{};
                QCOMPARE(evaluateNumericTracks(tracks, frame, output), NumericBatchStatus::Ok);
                QCOMPARE(output[0], evaluateTrack(cppTrack, frame, 0.0).toDouble());
            }
        }

        const std::array<NumericKeyframe, 4> duplicates = {
            makeKeyframe(0, 0.0, NumericInterpolation::Linear),
            makeKeyframe(50, 10.0, NumericInterpolation::Linear),
            makeKeyframe(50, 20.0, NumericInterpolation::Linear),
            makeKeyframe(100, 100.0, NumericInterpolation::Linear),
        };
        const std::array<NumericTrackView, 2> tracks = {{
            {.keyframes = duplicates.data(),
             .keyframes_length = duplicates.size(),
             .custom_points = nullptr,
             .custom_points_length = 0,
             .fallback_value = -1.0},
            {.keyframes = nullptr,
             .keyframes_length = 0,
             .custom_points = nullptr,
             .custom_points_length = 0,
             .fallback_value = 7.0},
        }};
        std::array<double, 2> output{};
        QCOMPARE(evaluateNumericTracks(tracks, 50, output), NumericBatchStatus::Ok);
        QCOMPARE(output[0], 10.0);
        QCOMPARE(output[1], 7.0);
        QCOMPARE(evaluateNumericTracks(tracks, 51, output), NumericBatchStatus::Ok);
        QVERIFY(output[0] > 20.0);
    }

    void rejectsInvalidViewsWithoutPartialWrites() {
        const std::array<NumericKeyframe, 2> valid = {
            makeKeyframe(0, 0.0, NumericInterpolation::Linear),
            makeKeyframe(10, 10.0, NumericInterpolation::Linear),
        };
        const std::array<NumericKeyframe, 2> unsorted = {
            makeKeyframe(10, 0.0, NumericInterpolation::Linear),
            makeKeyframe(5, 10.0, NumericInterpolation::Linear),
        };
        const std::array<NumericTrackView, 2> tracks = {{
            {.keyframes = valid.data(), .keyframes_length = valid.size(),
             .custom_points = nullptr, .custom_points_length = 0, .fallback_value = 0.0},
            {.keyframes = unsorted.data(), .keyframes_length = unsorted.size(),
             .custom_points = nullptr, .custom_points_length = 0, .fallback_value = 0.0},
        }};
        std::array<double, 2> output = {91.0, 92.0};
        QCOMPARE(evaluateNumericTracks(tracks, 5, output), NumericBatchStatus::InvalidArgument);
        QCOMPARE(output[0], 91.0);
        QCOMPARE(output[1], 92.0);

        NumericKeyframe badCustom = makeKeyframe(0, 0.0, NumericInterpolation::Custom);
        badCustom.custom_points_offset = 3;
        badCustom.custom_points_length = 6;
        const std::array<NumericKeyframe, 2> badKeys = {
            badCustom,
            makeKeyframe(10, 10.0, NumericInterpolation::Linear),
        };
        const std::array<double, 6> points{};
        const NumericTrackView badTrack = {
            .keyframes = badKeys.data(),
            .keyframes_length = badKeys.size(),
            .custom_points = points.data(),
            .custom_points_length = points.size(),
            .fallback_value = 0.0,
        };
        double value = 42.0;
        QCOMPARE(aviqtl_numeric_keyframe_batch_evaluate(&badTrack, 1, 5, &value, 1),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT});
        QCOMPARE(value, 42.0);

        QCOMPARE(aviqtl_numeric_keyframe_batch_evaluate(nullptr, 1, 5, &value, 1),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT});
        QCOMPARE(aviqtl_numeric_keyframe_batch_evaluate(&badTrack, 1, 5, &value, 0),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT});
    }

    void rejectsIncompleteCustomPointGroupsDuringConversion() {
        QVariantMap customStart = makePoint(0, 0.0, QStringLiteral("custom"));
        customStart.insert(QStringLiteral("points"), QVariantList{0.2, 0.3, 0.7, 0.8});
        const QVariantList invalidTrack = {
            customStart,
            makePoint(10, 10.0, QStringLiteral("linear")),
        };
        QVERIFY(!AviQtl::Core::RustKeyframes::buildNumericTrack(invalidTrack).has_value());

        customStart.insert(QStringLiteral("points"),
                           QVariantList{0.2, 0.3, 0.7, 0.8, 1.0, 1.0});
        const QVariantList validTrack = {
            customStart,
            makePoint(10, 10.0, QStringLiteral("linear")),
        };
        QVERIFY(AviQtl::Core::RustKeyframes::buildNumericTrack(validTrack).has_value());
    }

    void rejectsOverlappingOutput() {
        std::array<NumericKeyframe, 2> keys = {
            makeKeyframe(0, 0.0, NumericInterpolation::Linear),
            makeKeyframe(10, 10.0, NumericInterpolation::Linear),
        };
        const NumericTrackView track = {
            .keyframes = keys.data(),
            .keyframes_length = keys.size(),
            .custom_points = nullptr,
            .custom_points_length = 0,
            .fallback_value = 0.0,
        };
        QCOMPARE(aviqtl_numeric_keyframe_batch_evaluate(
                     &track, 1, 5, reinterpret_cast<double *>(keys.data()), 1),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS});
    }
};

QTEST_MAIN(TestKeyframeBatch)
#include "test_keyframe_batch.moc"
