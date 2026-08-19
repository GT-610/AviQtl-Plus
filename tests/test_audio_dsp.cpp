#include "rust_audio_dsp.hpp"
#include <QFile>
#include <QTest>
#include <QTextStream>
#include <array>
#include <cmath>
#include <limits>
#include <vector>

using namespace AviQtl::RustCore;

class TestAudioDsp : public QObject {
    Q_OBJECT

private:
    static std::vector<float> parseSamples(const QString &field, bool &ok) {
        std::vector<float> values;
        ok = true;
        if (field == QLatin1String("-")) {
            return values;
        }
        const QStringList samples = field.split(QLatin1Char(','), Qt::KeepEmptyParts);
        values.reserve(static_cast<std::size_t>(samples.size()));
        for (const QString &sample : samples) {
            bool sampleOk = false;
            const float value = sample.toFloat(&sampleOk);
            if (!sampleOk) {
                ok = false;
                return {};
            }
            values.push_back(value);
        }
        return values;
    }

    static bool samplesMatch(const std::vector<float> &actual,
                             const std::vector<float> &expected,
                             QString &error) {
        if (actual.size() != expected.size()) {
            error = QStringLiteral("size mismatch: actual=%1 expected=%2")
                        .arg(actual.size())
                        .arg(expected.size());
            return false;
        }
        for (std::size_t index = 0; index < actual.size(); ++index) {
            if (std::abs(actual[index] - expected[index]) >= 1e-6F) {
                error = QStringLiteral("sample %1: actual=%2 expected=%3")
                            .arg(index)
                            .arg(actual[index], 0, 'g', 9)
                            .arg(expected[index], 0, 'g', 9);
                return false;
            }
        }
        return true;
    }

private slots:
    void matchesCppGoldenValues() {
        QFile fixture(QString::fromUtf8(AVIQTL_AUDIO_DSP_FIXTURE));
        QVERIFY2(fixture.open(QIODevice::ReadOnly | QIODevice::Text),
                 qPrintable(QStringLiteral("Failed to open %1: %2")
                                .arg(fixture.fileName())
                                .arg(fixture.errorString())));

        QTextStream input(&fixture);
        int resampleCases = 0;
        int mixCases = 0;
        while (!input.atEnd()) {
            const QString line = input.readLine().trimmed();
            if (line.isEmpty() || line.startsWith(QLatin1Char('#'))) {
                continue;
            }
            const QStringList fields = line.simplified().split(QLatin1Char(' '), Qt::SkipEmptyParts);
            bool ok = false;
            if (fields[0] == QLatin1String("RESAMPLE")) {
                QCOMPARE(fields.size(), 4);
                const double sourceRate = fields[1].toDouble(&ok);
                QVERIFY(ok);
                const std::vector<float> source = parseSamples(fields[2], ok);
                QVERIFY(ok);
                const std::vector<float> expected = parseSamples(fields[3], ok);
                QVERIFY(ok);
                std::vector<float> actual(expected.size(), 0.0F);
                QCOMPARE(static_cast<std::uint32_t>(resampleStereoLinear(source, actual, sourceRate)),
                         static_cast<std::uint32_t>(AudioStatus::Ok));
                QString error;
                QVERIFY2(samplesMatch(actual, expected, error), qPrintable(error));
                ++resampleCases;
                continue;
            }

            QCOMPARE(fields[0], QStringLiteral("MIX"));
            QCOMPARE(fields.size(), 13);
            AudioMixParameters parameters{};
            parameters.relative_time = fields[1].toDouble(&ok);
            QVERIFY(ok);
            parameters.duration = fields[2].toDouble(&ok);
            QVERIFY(ok);
            parameters.fade_in_seconds = fields[3].toFloat(&ok);
            QVERIFY(ok);
            parameters.fade_out_seconds = fields[4].toFloat(&ok);
            QVERIFY(ok);
            parameters.volume = fields[5].toFloat(&ok);
            QVERIFY(ok);
            parameters.master_volume = fields[6].toFloat(&ok);
            QVERIFY(ok);
            parameters.pan = fields[7].toFloat(&ok);
            QVERIFY(ok);
            parameters.limiter = fields[8].toUInt(&ok);
            QVERIFY(ok);
            const std::vector<float> clip = parseSamples(fields[9], ok);
            QVERIFY(ok);
            std::vector<float> master = parseSamples(fields[10], ok);
            QVERIFY(ok);
            const std::vector<float> expectedMaster = parseSamples(fields[11], ok);
            QVERIFY(ok);
            const std::vector<float> expectedMeter = parseSamples(fields[12], ok);
            QVERIFY(ok);
            QCOMPARE(expectedMeter.size(), std::size_t{4});

            const std::array<AudioBatchTrack, 1> tracks = {{
                {
                    .samples = clip.data(),
                    .samples_length = clip.size(),
                    .parameters = parameters,
                    .clip_id = 1,
                    .mute = 0,
                    .solo = 0,
                    .reserved = 0,
                },
            }};
            std::array<AudioBatchResult, 1> results{};
            QCOMPARE(static_cast<std::uint32_t>(mixStereoBatch(tracks, master, results)),
                     static_cast<std::uint32_t>(AudioStatus::Ok));
            QString error;
            QVERIFY2(samplesMatch(master, expectedMaster, error), qPrintable(error));
            const std::vector<float> actualMeter = {
                results[0].meter.peak_left, results[0].meter.peak_right,
                results[0].meter.rms_left, results[0].meter.rms_right};
            QVERIFY2(samplesMatch(actualMeter, expectedMeter, error), qPrintable(error));
            ++mixCases;
        }

        QCOMPARE(resampleCases, 4);
        QCOMPARE(mixCases, 4);
    }

    void rejectsInvalidBuffers() {
        std::vector<float> stereo = {0.0F, 0.0F, 0.0F, 0.0F};
        std::vector<float> odd = {0.0F, 0.0F, 0.0F};
        std::vector<float> output(4, 0.0F);
        const std::vector<float> empty;
        QCOMPARE(static_cast<std::uint32_t>(resampleStereoLinear(empty, output, 1.0)),
                 static_cast<std::uint32_t>(AudioStatus::InvalidArgument));
        QCOMPARE(static_cast<std::uint32_t>(resampleStereoLinear(odd, output, 1.0)),
                 static_cast<std::uint32_t>(AudioStatus::InvalidArgument));
        QCOMPARE(static_cast<std::uint32_t>(resampleStereoLinear(stereo, stereo, 1.0)),
                 static_cast<std::uint32_t>(AudioStatus::OverlappingBuffers));
        QCOMPARE(static_cast<std::uint32_t>(resampleStereoLinear(stereo, output, -1.0)),
                 static_cast<std::uint32_t>(AudioStatus::InvalidArgument));
        QCOMPARE(static_cast<std::uint32_t>(
                     resampleStereoLinear(stereo, output, std::numeric_limits<double>::quiet_NaN())),
                 static_cast<std::uint32_t>(AudioStatus::InvalidArgument));

    }

    void mixesBatchWithSoloMuteAndPerTrackMeters() {
        const std::vector<float> soloSamples = {0.5F, -0.5F, 0.25F, -0.25F};
        const std::vector<float> skippedSamples = {1.0F, 1.0F, 1.0F, 1.0F};
        AudioMixParameters unity{
            .relative_time = 0.0,
            .duration = 1.0,
            .fade_in_seconds = 0.0F,
            .fade_out_seconds = 0.0F,
            .volume = 1.0F,
            .master_volume = 1.0F,
            .pan = 0.0F,
            .limiter = 0,
        };
        const std::array<AudioBatchTrack, 3> tracks = {{
            {
                .samples = soloSamples.data(),
                .samples_length = soloSamples.size(),
                .parameters = unity,
                .clip_id = 10,
                .mute = 0,
                .solo = 1,
                .reserved = 0,
            },
            {
                .samples = skippedSamples.data(),
                .samples_length = skippedSamples.size(),
                .parameters = unity,
                .clip_id = 11,
                .mute = 0,
                .solo = 0,
                .reserved = 0,
            },
            {
                .samples = skippedSamples.data(),
                .samples_length = skippedSamples.size(),
                .parameters = unity,
                .clip_id = 12,
                .mute = 1,
                .solo = 1,
                .reserved = 0,
            },
        }};
        std::vector<float> master(4, 0.0F);
        std::array<AudioBatchResult, 3> results{};

        QCOMPARE(static_cast<std::uint32_t>(mixStereoBatch(tracks, master, results)),
                 static_cast<std::uint32_t>(AudioStatus::Ok));
        QString error;
        QVERIFY2(samplesMatch(master, soloSamples, error), qPrintable(error));
        QCOMPARE(results[0].clip_id, 10);
        QCOMPARE(results[0].mixed, std::uint32_t{1});
        QCOMPARE(results[0].meter.peak_left, 0.5F);
        QCOMPARE(results[1].clip_id, 11);
        QCOMPARE(results[1].mixed, std::uint32_t{0});
        QCOMPARE(results[1].meter.peak_left, 0.0F);
        QCOMPARE(results[2].clip_id, 12);
        QCOMPARE(results[2].mixed, std::uint32_t{0});
    }

    void rejectsInvalidBatchResultCount() {
        const std::vector<float> samples = {0.0F, 0.0F};
        const std::array<AudioBatchTrack, 1> tracks = {{
            {
                .samples = samples.data(),
                .samples_length = samples.size(),
                .parameters = {},
                .clip_id = 1,
                .mute = 0,
                .solo = 0,
                .reserved = 0,
            },
        }};
        std::vector<float> master(2, 0.0F);
        std::span<AudioBatchResult> noResults;
        QCOMPARE(static_cast<std::uint32_t>(mixStereoBatch(tracks, master, noResults)),
                 static_cast<std::uint32_t>(AudioStatus::InvalidArgument));
    }
};

QTEST_MAIN(TestAudioDsp)
#include "test_audio_dsp.moc"
