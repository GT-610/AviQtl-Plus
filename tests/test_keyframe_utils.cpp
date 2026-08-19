#include <QColor>
#include <QFile>
#include <QTest>
#include <QTextStream>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include "keyframe_utils.hpp"
#include "rust_keyframe_document.hpp"

using namespace AviQtl::Core::KeyframeUtils;

class TestKeyframeUtils : public QObject {
    Q_OBJECT

private:
    static QVariantMap makePoint(int frame, QVariant value, const QString &interp = QStringLiteral("linear")) {
        return {{QStringLiteral("frame"), frame}, {QStringLiteral("value"), value}, {QStringLiteral("interp"), interp}};
    }

    static QVariantMap makeStructuredTrack(QVariant startValue, const QVariantList &points, int startFrame = 0) {
        QVariantMap start;
        start[QStringLiteral("frame")] = startFrame;
        start[QStringLiteral("value")] = startValue;
        start[QStringLiteral("interp")] = QStringLiteral("linear");
        return {{QStringLiteral("start"), start}, {QStringLiteral("points"), points}};
    }

    static QStringList easingNames() { return AviQtl::RustCore::easingNames(); }

private slots:
    // --- Rust keyframe document bridge ---
    void keyframeDocument_inspectSortsAndFlattens() {
        QVariantList points;
        points.append(makePoint(20, 3.0));
        points.append(makePoint(10, 2.0));
        QVariantMap track = makeStructuredTrack(1.0, points);
        const auto result = AviQtl::Core::RustKeyframeDocument::inspect(track, 1.0);
        QVERIFY(result.has_value());
        const QVariantList flat = result->flat;
        QCOMPARE(flat.size(), 3);
        QCOMPARE(flat[0].toMap()[QStringLiteral("frame")].toInt(), 0);
        QCOMPARE(flat[0].toMap()[QStringLiteral("value")].toDouble(), 1.0);
        QCOMPARE(flat[1].toMap()[QStringLiteral("frame")].toInt(), 10);
        QCOMPARE(flat[2].toMap()[QStringLiteral("frame")].toInt(), 20);
    }

    void keyframeDocument_inspectPreservesStartWithoutPoints() {
        QVariantMap track = makeStructuredTrack(5.0, QVariantList());
        const auto result = AviQtl::Core::RustKeyframeDocument::inspect(track, 5.0);
        QVERIFY(result.has_value());
        const QVariantList flat = result->flat;
        QCOMPARE(flat.size(), 1);
        QCOMPARE(flat[0].toMap()[QStringLiteral("value")].toDouble(), 5.0);
    }

    // --- evaluateTrack: numeric linear ---
    void evaluateTrack_numericLinear() {
        QVariantList track;
        track.append(makePoint(0, 0.0, QStringLiteral("linear")));
        track.append(makePoint(100, 100.0, QStringLiteral("linear")));
        QCOMPARE(evaluateTrack(track, 0, 0.0).toDouble(), 0.0);
        QCOMPARE(evaluateTrack(track, 50, 0.0).toDouble(), 50.0);
        QCOMPARE(evaluateTrack(track, 100, 0.0).toDouble(), 100.0);
        QCOMPARE(evaluateTrack(track, 25, 0.0).toDouble(), 25.0);
    }

    // --- evaluateTrack: color interpolation ---
    void evaluateTrack_colorLinear() {
        QVariantList track;
        track.append(makePoint(0, QStringLiteral("#ff0000"), QStringLiteral("linear")));
        track.append(makePoint(100, QStringLiteral("#0000ff"), QStringLiteral("linear")));

        QVariant mid = evaluateTrack(track, 50, QStringLiteral("#000000"));
        QColor c(mid.toString());
        QVERIFY(c.isValid());
        // At t=0.5: R=127 or 128, G=0, B=127 or 128
        QVERIFY(std::abs(c.red() - 127) <= 1 || std::abs(c.red() - 128) <= 1);
        QCOMPARE(c.green(), 0);
        QVERIFY(std::abs(c.blue() - 127) <= 1 || std::abs(c.blue() - 128) <= 1);
    }

    void evaluateTrack_colorEndpoints() {
        QVariantList track;
        track.append(makePoint(0, QStringLiteral("#ff0000"), QStringLiteral("linear")));
        track.append(makePoint(100, QStringLiteral("#0000ff"), QStringLiteral("linear")));

        QVariant v0 = evaluateTrack(track, 0, QStringLiteral("#000000"));
        QVariant v1 = evaluateTrack(track, 100, QStringLiteral("#000000"));
        QCOMPARE(QColor(v0.toString()).red(), 255);
        QCOMPARE(QColor(v0.toString()).blue(), 0);
        QCOMPARE(QColor(v1.toString()).red(), 0);
        QCOMPARE(QColor(v1.toString()).blue(), 255);
    }

    // --- evaluateTrack: random mode ---
    void evaluateTrack_randomDeterministic() {
        QVariantList track;
        track.append(makePoint(0, 0.0, QStringLiteral("random")));
        track.append(makePoint(100, 100.0, QStringLiteral("random")));

        QVariant v1 = evaluateTrack(track, 10, 0.0);
        QVariant v2 = evaluateTrack(track, 10, 0.0);
        // Same frame should produce same random value (deterministic)
        QCOMPARE(v1.toDouble(), v2.toDouble());
        // Value should be in [0, 100]
        QVERIFY(v1.toDouble() >= 0.0);
        QVERIFY(v1.toDouble() <= 100.0);
    }

    void evaluateTrack_randomDifferentFrames() {
        QVariantList track;
        track.append(makePoint(0, 0.0, QStringLiteral("random")));
        track.append(makePoint(100, 10.0, QStringLiteral("random")));

        QVariant v0 = evaluateTrack(track, 0, 0.0);
        QVariant v50 = evaluateTrack(track, 50, 0.0);
        // Different frames may produce different values (not guaranteed but very likely)
        // At minimum, verify both are in range
        QVERIFY(v0.toDouble() >= 0.0 && v0.toDouble() <= 10.0);
        QVERIFY(v50.toDouble() >= 0.0 && v50.toDouble() <= 10.0);
    }

    // --- evaluateTrack: alternate mode ---
    void evaluateTrack_alternate() {
        QVariantList track;
        track.append(makePoint(0, 10.0, QStringLiteral("alternate")));
        track.append(makePoint(100, 20.0, QStringLiteral("alternate")));

        // stepFrames defaults to 1, so (frame - f0) / 1 % 2 == 0 -> a, else b
        QCOMPARE(evaluateTrack(track, 0, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 1, 0.0).toDouble(), 20.0);
        QCOMPARE(evaluateTrack(track, 2, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 3, 0.0).toDouble(), 20.0);
    }

    void evaluateTrack_alternateWithStepFrames() {
        QVariantList track;
        QVariantMap p0 = makePoint(0, 10.0, QStringLiteral("alternate"));
        p0[QStringLiteral("modeParams")] = QVariantMap{{QStringLiteral("stepFrames"), 5}};
        track.append(p0);
        track.append(makePoint(100, 20.0, QStringLiteral("alternate")));

        QCOMPARE(evaluateTrack(track, 0, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 4, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 5, 0.0).toDouble(), 20.0);
        QCOMPARE(evaluateTrack(track, 9, 0.0).toDouble(), 20.0);
        QCOMPARE(evaluateTrack(track, 10, 0.0).toDouble(), 10.0);
    }

    // --- evaluateTrack: custom bezier ---
    void evaluateTrack_customBezier() {
        QVariantList track;
        // Custom bezier with simple identity curve: cp1=(0.33,0.33), cp2=(0.66,0.66), end=(1,1)
        QVariantMap p0 = makePoint(0, 0.0, QStringLiteral("custom"));
        p0[QStringLiteral("points")] = QVariantList{0.33, 0.33, 0.66, 0.66, 1.0, 1.0};
        track.append(p0);
        track.append(makePoint(100, 100.0, QStringLiteral("custom")));

        QVariant mid = evaluateTrack(track, 50, 0.0);
        // Near-identity curve at t=0.5 should be ~50
        QVERIFY(std::abs(mid.toDouble() - 50.0) < 2.0);
    }

    // --- evaluateTrack: non-numeric fallback ---
    void evaluateTrack_noneInterpReturnsFirst() {
        QVariantList track;
        track.append(QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 10.0}, {QStringLiteral("interp"), QStringLiteral("none")}});
        track.append(QVariantMap{{QStringLiteral("frame"), 100}, {QStringLiteral("value"), 20.0}, {QStringLiteral("interp"), QStringLiteral("none")}});
        // none interp: returns v0 when frame < f1
        QCOMPARE(evaluateTrack(track, 50, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 99, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 100, 0.0).toDouble(), 20.0);
    }

    // --- resolveTrack ---
    void resolveTrack_structured() {
        QVariantList points;
        points.append(makePoint(50, 2.0));
        QVariantMap track = makeStructuredTrack(1.0, points);
        QVariantList resolved = resolveTrack(QVariant(track), 0.0, 200);
        // Should be [start(frame=0), point(frame=50)]
        QCOMPARE(resolved.size(), 2);
        QCOMPARE(resolved[0].toMap()[QStringLiteral("frame")].toInt(), 0);
        QCOMPARE(resolved[0].toMap()[QStringLiteral("value")].toDouble(), 1.0);
        QCOMPARE(resolved[1].toMap()[QStringLiteral("frame")].toInt(), 50);
    }

    void resolveTrack_flat() {
        QVariantList flat;
        flat.append(makePoint(10, 1.0));
        flat.append(makePoint(0, 0.0));
        QVariantList resolved = resolveTrack(QVariant(flat), -1.0, 100);
        QCOMPARE(resolved.size(), 2);
        QCOMPARE(resolved[0].toMap()[QStringLiteral("frame")].toInt(), 0);
        QCOMPARE(resolved[1].toMap()[QStringLiteral("frame")].toInt(), 10);
    }

    // --- resolveAllTracks ---
    void resolveAllTracks_multipleParams() {
        QVariantMap params;
        params[QStringLiteral("x")] = 0.0;
        params[QStringLiteral("y")] = 0.0;

        QVariantMap tracks;
        tracks[QStringLiteral("x")] = makeStructuredTrack(10.0, QVariantList{makePoint(100, 20.0)});
        tracks[QStringLiteral("y")] = makeStructuredTrack(30.0, QVariantList{makePoint(50, 40.0)});

        auto resolved = resolveAllTracks(params, tracks, 200);
        QCOMPARE(resolved.size(), 2);
        QVERIFY(resolved.contains(QStringLiteral("x")));
        QVERIFY(resolved.contains(QStringLiteral("y")));
        QCOMPARE(resolved[QStringLiteral("x")].size(), 2);
        QCOMPARE(resolved[QStringLiteral("y")].size(), 2);
    }

    void resolveAllTracks_empty() {
        auto resolved = resolveAllTracks(QVariantMap(), QVariantMap(), 100);
        QVERIFY(resolved.isEmpty());
    }

    // --- evaluateResolvedParam ---
    void evaluateResolvedParam_withCache() {
        QVariantMap params;
        params[QStringLiteral("opacity")] = 0.0;

        QVariantMap tracks;
        tracks[QStringLiteral("opacity")] = makeStructuredTrack(1.0, QVariantList{makePoint(100, 0.0)});

        auto resolved = resolveAllTracks(params, tracks, 200);

        QCOMPARE(evaluateResolvedParam(params, resolved, QStringLiteral("opacity"), 0).toDouble(), 1.0);
        QCOMPARE(evaluateResolvedParam(params, resolved, QStringLiteral("opacity"), 50).toDouble(), 0.5);
        QCOMPARE(evaluateResolvedParam(params, resolved, QStringLiteral("opacity"), 100).toDouble(), 0.0);
    }

    void evaluateResolvedParam_missingParam() {
        QVariantMap params;
        params[QStringLiteral("x")] = 42.0;
        auto resolved = resolveAllTracks(params, QVariantMap(), 100);
        QCOMPARE(evaluateResolvedParam(params, resolved, QStringLiteral("x"), 0).toDouble(), 42.0);
    }

    // --- Rust keyframe normalization ---
    void keyframeDocument_normalizeClipsBeyondDuration() {
        QVariantList points;
        points.append(makePoint(50, 1.0));
        points.append(makePoint(200, 2.0));
        QVariantMap track = makeStructuredTrack(0.0, points);

        const auto result = AviQtl::Core::RustKeyframeDocument::normalize(track, 0.0, 100);
        QVERIFY(result.has_value());
        const QVariantList resultPoints = result->track[QStringLiteral("points")].toList();
        QCOMPARE(resultPoints.size(), 1);
        QCOMPARE(resultPoints[0].toMap()[QStringLiteral("frame")].toInt(), 50);
    }

    void keyframeDocument_normalizePreservesWithinDuration() {
        QVariantList points;
        points.append(makePoint(30, 1.0));
        points.append(makePoint(60, 2.0));
        QVariantMap track = makeStructuredTrack(0.0, points);

        const auto result = AviQtl::Core::RustKeyframeDocument::normalize(track, 0.0, 100);
        QVERIFY(result.has_value());
        const QVariantList resultPoints = result->track[QStringLiteral("points")].toList();
        QCOMPARE(resultPoints.size(), 2);
    }

    void keyframeDocument_normalizeFlatLegacy() {
        QVariantList flat;
        flat.append(makePoint(0, 0.0));
        flat.append(makePoint(50, 1.0));
        flat.append(makePoint(200, 2.0));

        const auto result = AviQtl::Core::RustKeyframeDocument::normalize(flat, 0.0, 100);
        QVERIFY(result.has_value());
        const QVariantList resultPoints = result->track[QStringLiteral("points")].toList();
        // frame=0 goes into start, frame=50 stays, frame=200 is clipped
        QCOMPARE(resultPoints.size(), 1);
        QCOMPARE(resultPoints[0].toMap()[QStringLiteral("frame")].toInt(), 50);
    }

    // --- Rust-owned easing names ---
    void easingFunctions_allPresent() {
        const QStringList names = easingNames();
        QCOMPARE(names.size(), 42);
        for (const QString &name : names)
            QVERIFY2(AviQtl::RustCore::easingKindForName(name).has_value(),
                     qPrintable(QStringLiteral("Missing easing: ") + name));
        QVERIFY(!AviQtl::RustCore::easingKindForName(QStringLiteral("unknown")).has_value());
    }

    void easingFunctions_endpoints() {
        std::vector<double> p;
        // Every easing should map 0->0 and 1->1 (except custom which depends on params)
        QStringList standard = {
            QStringLiteral("linear"), QStringLiteral("ease_in_sine"), QStringLiteral("ease_out_sine"),
            QStringLiteral("ease_in_quad"), QStringLiteral("ease_out_quad"),
            QStringLiteral("ease_in_cubic"), QStringLiteral("ease_out_cubic"),
            QStringLiteral("ease_in_expo"), QStringLiteral("ease_out_expo"),
            QStringLiteral("ease_in_circ"), QStringLiteral("ease_out_circ"),
            QStringLiteral("ease_in_back"), QStringLiteral("ease_out_back"),
            QStringLiteral("ease_out_bounce"), QStringLiteral("ease_in_bounce")
        };
        for (const auto &name : standard) {
            double v0 = AviQtl::RustCore::evaluateEasing(name, 0.0, p, 1.0, 0.3);
            double v1 = AviQtl::RustCore::evaluateEasing(name, 1.0, p, 1.0, 0.3);
            QVERIFY2(std::abs(v0) < 1e-6, qPrintable(name + QStringLiteral(": f(0) = %1").arg(v0)));
            QVERIFY2(std::abs(v1 - 1.0) < 1e-6, qPrintable(name + QStringLiteral(": f(1) = %1").arg(v1)));
        }
    }

    void easingFunctions_midpoints() {
        std::vector<double> p;
        // Monotonically increasing easings: f(0.5) should be between 0 and 1
        QStringList monotone = {
            QStringLiteral("linear"), QStringLiteral("ease_in_sine"), QStringLiteral("ease_out_sine"),
            QStringLiteral("ease_in_quad"), QStringLiteral("ease_out_quad"),
            QStringLiteral("ease_in_cubic"), QStringLiteral("ease_out_cubic"),
            QStringLiteral("ease_in_expo"), QStringLiteral("ease_out_expo")
        };
        for (const auto &name : monotone) {
            double v = AviQtl::RustCore::evaluateEasing(name, 0.5, p, 1.0, 0.3);
            QVERIFY2(v > 0.0 && v < 1.0, qPrintable(name + QStringLiteral(": f(0.5) = %1").arg(v)));
        }
        // linear at 0.5 should be exactly 0.5
        QCOMPARE(AviQtl::RustCore::evaluateEasing(QStringLiteral("linear"), 0.5, p, 1.0, 0.3), 0.5);
    }

    void rustEasing_matchesCppGoldenValues() {
        const QStringList names = easingNames();
        const std::vector<double> samples = {0.1, 0.25, 0.5, 0.75, 0.9};
        const std::vector<double> noPoints;
        const QVariantMap modeParams{
            {QStringLiteral("amplitude"), 0.8},
            {QStringLiteral("period"), 0.42},
        };
        const std::vector<double> customPoints = {
            0.1, 0.2, 0.4, 0.5, 0.6, 0.7,
            0.7, 0.8, 0.9, 0.95, 1.0, 1.0,
        };

        QFile fixture(QString::fromUtf8(AVIQTL_KEYFRAME_EASING_FIXTURE));
        QVERIFY2(fixture.open(QIODevice::ReadOnly | QIODevice::Text),
                 qPrintable(QStringLiteral("Failed to open %1: %2")
                                .arg(fixture.fileName())
                                .arg(fixture.errorString())));

        QTextStream input(&fixture);
        const int easingCount = static_cast<int>(names.size());
        std::vector<bool> seen(static_cast<std::size_t>(easingCount), false);
        int caseCount = 0;
        while (!input.atEnd()) {
            const QString line = input.readLine().trimmed();
            if (line.isEmpty() || line.startsWith(QLatin1Char('#'))) {
                continue;
            }

            const QStringList fields = line.simplified().split(QLatin1Char(' '), Qt::SkipEmptyParts);
            QCOMPARE(fields.size(), static_cast<qsizetype>(samples.size() + 1));

            bool ok = false;
            const int kind = fields[0].toInt(&ok);
            QVERIFY2(ok, qPrintable(QStringLiteral("Invalid easing kind: %1").arg(fields[0])));
            QVERIFY2(kind >= 0 && kind < easingCount,
                     qPrintable(QStringLiteral("Easing kind out of range: %1").arg(kind)));
            QVERIFY2(!seen[static_cast<std::size_t>(kind)],
                     qPrintable(QStringLiteral("Duplicate easing kind: %1").arg(kind)));
            seen[static_cast<std::size_t>(kind)] = true;

            const QString &name = names.at(kind);
            const auto &points = name == QStringLiteral("custom") ? customPoints : noPoints;
            for (std::size_t index = 0; index < samples.size(); ++index) {
                const double expected = fields[static_cast<qsizetype>(index + 1)].toDouble(&ok);
                QVERIFY2(ok, qPrintable(QStringLiteral("Invalid golden value for kind %1").arg(kind)));
                const double actual = AviQtl::RustCore::evaluateEasing(
                    name, samples[index], points,
                    modeParams.value(QStringLiteral("amplitude")).toDouble(),
                    modeParams.value(QStringLiteral("period")).toDouble());
                QVERIFY2(std::abs(actual - expected) < 1e-12,
                         qPrintable(QStringLiteral("%1 at t=%2: Rust=%3 C++ golden=%4")
                                        .arg(name)
                                        .arg(samples[index], 0, 'g', 17)
                                        .arg(actual, 0, 'g', 17)
                                        .arg(expected, 0, 'g', 17)));
            }
            ++caseCount;
        }

        QCOMPARE(caseCount, easingCount);
        QVERIFY(std::all_of(seen.cbegin(), seen.cend(), [](bool value) { return value; }));
    }
};

QTEST_MAIN(TestKeyframeUtils)
#include "test_keyframe_utils.moc"
