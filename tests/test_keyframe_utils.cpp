#include <QColor>
#include <QTest>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include "keyframe_utils.hpp"

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

private slots:
    // --- sortPoints ---
    void sortPoints_alreadySorted() {
        QVariantList pts;
        pts.append(makePoint(0, 1.0));
        pts.append(makePoint(10, 2.0));
        pts.append(makePoint(20, 3.0));
        QVariantList sorted = sortPoints(pts);
        QCOMPARE(sorted.size(), 3);
        QCOMPARE(sorted[0].toMap()[QStringLiteral("frame")].toInt(), 0);
        QCOMPARE(sorted[2].toMap()[QStringLiteral("frame")].toInt(), 20);
    }

    void sortPoints_unsorted() {
        QVariantList pts;
        pts.append(makePoint(20, 3.0));
        pts.append(makePoint(0, 1.0));
        pts.append(makePoint(10, 2.0));
        QVariantList sorted = sortPoints(pts);
        QCOMPARE(sorted[0].toMap()[QStringLiteral("frame")].toInt(), 0);
        QCOMPARE(sorted[1].toMap()[QStringLiteral("frame")].toInt(), 10);
        QCOMPARE(sorted[2].toMap()[QStringLiteral("frame")].toInt(), 20);
    }

    void sortPoints_empty() {
        QVariantList sorted = sortPoints(QVariantList());
        QVERIFY(sorted.isEmpty());
    }

    // --- flattenStructuredTrack ---
    void flattenStructuredTrack_basic() {
        QVariantList points;
        points.append(makePoint(20, 3.0));
        points.append(makePoint(10, 2.0));
        QVariantMap track = makeStructuredTrack(1.0, points);
        QVariantList flat = flattenStructuredTrack(track);
        QCOMPARE(flat.size(), 3);
        QCOMPARE(flat[0].toMap()[QStringLiteral("frame")].toInt(), 0);
        QCOMPARE(flat[0].toMap()[QStringLiteral("value")].toDouble(), 1.0);
        QCOMPARE(flat[1].toMap()[QStringLiteral("frame")].toInt(), 10);
        QCOMPARE(flat[2].toMap()[QStringLiteral("frame")].toInt(), 20);
    }

    void flattenStructuredTrack_noPoints() {
        QVariantMap track = makeStructuredTrack(5.0, QVariantList());
        QVariantList flat = flattenStructuredTrack(track);
        QCOMPARE(flat.size(), 1);
        QCOMPARE(flat[0].toMap()[QStringLiteral("value")].toDouble(), 5.0);
    }

    // --- solveBezierT ---
    void solveBezierT_identity() {
        // Simple linear bezier: (0,0) -> (1,1) with cp at 1/3, 2/3
        double t = solveBezierT(0.5, 1.0 / 3.0, 2.0 / 3.0);
        QVERIFY(std::abs(t - 0.5) < 1e-4);
    }

    void solveBezierT_endpoints() {
        double t0 = solveBezierT(0.0, 0.33, 0.66);
        QVERIFY(std::abs(t0) < 1e-4);
        double t1 = solveBezierT(1.0, 0.33, 0.66);
        QVERIFY(std::abs(t1 - 1.0) < 1e-4);
    }

    void solveBezierT_clamped() {
        // solveBezierT clamps t to [0,1] via std::clamp at the end of iteration.
        // For in-range inputs, result should be in [0,1].
        double t = solveBezierT(0.5, 0.33, 0.66);
        QVERIFY(t >= 0.0 && t <= 1.0);
        t = solveBezierT(0.0, 0.33, 0.66);
        QVERIFY(t >= 0.0 && t <= 1.0);
        t = solveBezierT(1.0, 0.33, 0.66);
        QVERIFY(t >= 0.0 && t <= 1.0);
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

    // --- normalizeTrackForDuration ---
    void normalizeTrackForDuration_clipsBeyondDuration() {
        QVariantList points;
        points.append(makePoint(50, 1.0));
        points.append(makePoint(200, 2.0));
        QVariantMap track = makeStructuredTrack(0.0, points);

        QVariantMap result = normalizeTrackForDuration(QVariant(track), 0.0, 100);
        QVariantList resultPoints = result[QStringLiteral("points")].toList();
        QCOMPARE(resultPoints.size(), 1);
        QCOMPARE(resultPoints[0].toMap()[QStringLiteral("frame")].toInt(), 50);
    }

    void normalizeTrackForDuration_preservesWithinDuration() {
        QVariantList points;
        points.append(makePoint(30, 1.0));
        points.append(makePoint(60, 2.0));
        QVariantMap track = makeStructuredTrack(0.0, points);

        QVariantMap result = normalizeTrackForDuration(QVariant(track), 0.0, 100);
        QVariantList resultPoints = result[QStringLiteral("points")].toList();
        QCOMPARE(resultPoints.size(), 2);
    }

    void normalizeTrackForDuration_flatLegacy() {
        QVariantList flat;
        flat.append(makePoint(0, 0.0));
        flat.append(makePoint(50, 1.0));
        flat.append(makePoint(200, 2.0));

        QVariantMap result = normalizeTrackForDuration(QVariant(flat), 0.0, 100);
        QVariantList resultPoints = result[QStringLiteral("points")].toList();
        // frame=0 goes into start, frame=50 stays, frame=200 is clipped
        QCOMPARE(resultPoints.size(), 1);
        QCOMPARE(resultPoints[0].toMap()[QStringLiteral("frame")].toInt(), 50);
    }

    // --- easingFunctions completeness ---
    void easingFunctions_allPresent() {
        const auto &funcs = easingFunctions();
        QStringList expected = {
            QStringLiteral("linear"),
            QStringLiteral("ease_in_sine"), QStringLiteral("ease_out_sine"),
            QStringLiteral("ease_in_out_sine"), QStringLiteral("ease_out_in_sine"),
            QStringLiteral("ease_in_quad"), QStringLiteral("ease_out_quad"),
            QStringLiteral("ease_in_out_quad"), QStringLiteral("ease_out_in_quad"),
            QStringLiteral("ease_in_cubic"), QStringLiteral("ease_out_cubic"),
            QStringLiteral("ease_in_out_cubic"), QStringLiteral("ease_out_in_cubic"),
            QStringLiteral("ease_in_quart"), QStringLiteral("ease_out_quart"),
            QStringLiteral("ease_in_out_quart"), QStringLiteral("ease_out_in_quart"),
            QStringLiteral("ease_in_quint"), QStringLiteral("ease_out_quint"),
            QStringLiteral("ease_in_out_quint"), QStringLiteral("ease_out_in_quint"),
            QStringLiteral("ease_in_expo"), QStringLiteral("ease_out_expo"),
            QStringLiteral("ease_in_out_expo"), QStringLiteral("ease_out_in_expo"),
            QStringLiteral("ease_in_circ"), QStringLiteral("ease_out_circ"),
            QStringLiteral("ease_in_out_circ"), QStringLiteral("ease_out_in_circ"),
            QStringLiteral("ease_in_back"), QStringLiteral("ease_out_back"),
            QStringLiteral("ease_in_out_back"), QStringLiteral("ease_out_in_back"),
            QStringLiteral("ease_in_elastic"), QStringLiteral("ease_out_elastic"),
            QStringLiteral("ease_in_out_elastic"), QStringLiteral("ease_out_in_elastic"),
            QStringLiteral("ease_out_bounce"), QStringLiteral("ease_in_bounce"),
            QStringLiteral("ease_in_out_bounce"), QStringLiteral("ease_out_in_bounce"),
            QStringLiteral("custom")
        };
        for (const auto &name : expected) {
            QVERIFY2(funcs.contains(name), qPrintable(QStringLiteral("Missing easing: ") + name));
        }
        QCOMPARE(funcs.size(), expected.size());
    }

    void easingFunctions_endpoints() {
        const auto &funcs = easingFunctions();
        std::vector<double> p;
        QVariantMap mp;
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
            auto it = funcs.find(name);
            QVERIFY(it != funcs.end());
            double v0 = it.value()(0.0, p, mp);
            double v1 = it.value()(1.0, p, mp);
            QVERIFY2(std::abs(v0) < 1e-6, qPrintable(name + QStringLiteral(": f(0) = %1").arg(v0)));
            QVERIFY2(std::abs(v1 - 1.0) < 1e-6, qPrintable(name + QStringLiteral(": f(1) = %1").arg(v1)));
        }
    }

    void easingFunctions_midpoints() {
        const auto &funcs = easingFunctions();
        std::vector<double> p;
        QVariantMap mp;
        // Monotonically increasing easings: f(0.5) should be between 0 and 1
        QStringList monotone = {
            QStringLiteral("linear"), QStringLiteral("ease_in_sine"), QStringLiteral("ease_out_sine"),
            QStringLiteral("ease_in_quad"), QStringLiteral("ease_out_quad"),
            QStringLiteral("ease_in_cubic"), QStringLiteral("ease_out_cubic"),
            QStringLiteral("ease_in_expo"), QStringLiteral("ease_out_expo")
        };
        for (const auto &name : monotone) {
            auto it = funcs.find(name);
            double v = it.value()(0.5, p, mp);
            QVERIFY2(v > 0.0 && v < 1.0, qPrintable(name + QStringLiteral(": f(0.5) = %1").arg(v)));
        }
        // linear at 0.5 should be exactly 0.5
        QCOMPARE(funcs[QStringLiteral("linear")](0.5, p, mp), 0.5);
    }
};

QTEST_MAIN(TestKeyframeUtils)
#include "test_keyframe_utils.moc"
