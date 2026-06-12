#include <QTest>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include "engine/timeline/keyframe_evaluator.hpp"

using namespace AviQtl::Engine::Timeline;

class TestKeyframeEvaluator : public QObject {
    Q_OBJECT

private slots:
    void isStructuredTrack_data() {
        QTest::addColumn<QVariant>("input");
        QTest::addColumn<bool>("expected");

        QVariantMap structured;
        structured[QStringLiteral("start")] = QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 1.0}};
        structured[QStringLiteral("points")] = QVariantList();
        QTest::newRow("structured") << QVariant(structured) << true;

        QVariantList flat;
        flat.append(QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 1.0}});
        QTest::newRow("flat list") << QVariant(flat) << false;

        QTest::newRow("empty") << QVariant() << false;
        QTest::newRow("int") << QVariant(42) << false;
    }

    void isStructuredTrack() {
        QFETCH(QVariant, input);
        QFETCH(bool, expected);
        QCOMPARE(AviQtl::Engine::Timeline::isStructuredTrack(input), expected);
    }

    void evaluateTrack_empty_returnsFallback() {
        QVariantList track;
        QCOMPARE(evaluateTrack(track, 0, 42.0), 42.0);
    }

    void evaluateTrack_singlePoint() {
        QVariantList track;
        track.append(QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), 5.0}, {QStringLiteral("interp"), QStringLiteral("none")}});
        QCOMPARE(evaluateTrack(track, 0, 0.0).toDouble(), 5.0);
        QCOMPARE(evaluateTrack(track, 10, 0.0).toDouble(), 5.0);
        QCOMPARE(evaluateTrack(track, 20, 0.0).toDouble(), 5.0);
    }

    void evaluateTrack_twoPoints_noneInterp() {
        QVariantList track;
        track.append(QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 10.0}, {QStringLiteral("interp"), QStringLiteral("none")}});
        track.append(QVariantMap{{QStringLiteral("frame"), 100}, {QStringLiteral("value"), 20.0}, {QStringLiteral("interp"), QStringLiteral("none")}});
        QCOMPARE(evaluateTrack(track, 0, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 50, 0.0).toDouble(), 10.0);
        QCOMPARE(evaluateTrack(track, 100, 0.0).toDouble(), 20.0);
    }

    void evaluateTrack_twoPoints_linear() {
        QVariantList track;
        track.append(QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 0.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        track.append(QVariantMap{{QStringLiteral("frame"), 100}, {QStringLiteral("value"), 100.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        QCOMPARE(evaluateTrack(track, 50, 0.0).toDouble(), 50.0);
        QCOMPARE(evaluateTrack(track, 25, 0.0).toDouble(), 25.0);
    }

    void evaluateTrack_beforeFirstFrame() {
        QVariantList track;
        track.append(QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), 5.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        track.append(QVariantMap{{QStringLiteral("frame"), 20}, {QStringLiteral("value"), 15.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        QCOMPARE(evaluateTrack(track, 0, 0.0).toDouble(), 5.0);
    }

    void evaluateTrack_afterLastFrame() {
        QVariantList track;
        track.append(QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 5.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        track.append(QVariantMap{{QStringLiteral("frame"), 10}, {QStringLiteral("value"), 15.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        QCOMPARE(evaluateTrack(track, 50, 0.0).toDouble(), 15.0);
    }

    void evaluateTrack_easingFunctionsExist() {
        const auto &funcs = easingFunctions();
        QVERIFY(funcs.contains(QStringLiteral("linear")));
        QVERIFY(funcs.contains(QStringLiteral("ease_in_sine")));
        QVERIFY(funcs.contains(QStringLiteral("ease_out_bounce")));
        QVERIFY(funcs.contains(QStringLiteral("ease_in_elastic")));
        QVERIFY(funcs.contains(QStringLiteral("custom")));
    }

    void evaluateTrack_easingLinear() {
        const auto &funcs = easingFunctions();
        auto it = funcs.find(QStringLiteral("linear"));
        QVERIFY(it != funcs.end());
        std::vector<double> params;
        QVariantMap modeParams;
        QCOMPARE(it.value()(0.0, params, modeParams), 0.0);
        QCOMPARE(it.value()(0.5, params, modeParams), 0.5);
        QCOMPARE(it.value()(1.0, params, modeParams), 1.0);
    }

    void evaluateParam_noTrack() {
        QVariantMap params;
        params[QStringLiteral("x")] = 42.0;
        QVariantMap tracks;
        QCOMPARE(evaluateParam(params, tracks, QStringLiteral("x"), 0, 30.0, 100), 42.0);
    }

    void evaluateParam_withTrack() {
        QVariantMap params;
        params[QStringLiteral("x")] = 0.0;

        QVariantMap start;
        start[QStringLiteral("frame")] = 0;
        start[QStringLiteral("value")] = 0.0;
        start[QStringLiteral("interp")] = QStringLiteral("linear");
        QVariantList points;
        points.append(QVariantMap{{QStringLiteral("frame"), 100}, {QStringLiteral("value"), 100.0}, {QStringLiteral("interp"), QStringLiteral("linear")}});
        QVariantMap track;
        track[QStringLiteral("start")] = start;
        track[QStringLiteral("points")] = points;

        QVariantMap tracks;
        tracks[QStringLiteral("x")] = track;

        QCOMPARE(evaluateParam(params, tracks, QStringLiteral("x"), 0, 30.0, 200).toDouble(), 0.0);
        QCOMPARE(evaluateParam(params, tracks, QStringLiteral("x"), 50, 30.0, 200).toDouble(), 50.0);
        QCOMPARE(evaluateParam(params, tracks, QStringLiteral("x"), 100, 30.0, 200).toDouble(), 100.0);
    }

    void inferredDurationForTrack_structured() {
        QVariantMap start;
        start[QStringLiteral("frame")] = 0;
        start[QStringLiteral("value")] = 0.0;
        QVariantList points;
        points.append(QVariantMap{{QStringLiteral("frame"), 50}, {QStringLiteral("value"), 1.0}});
        points.append(QVariantMap{{QStringLiteral("frame"), 120}, {QStringLiteral("value"), 2.0}});
        QVariantMap track;
        track[QStringLiteral("start")] = start;
        track[QStringLiteral("points")] = points;
        QCOMPARE(inferredDurationForTrack(QVariant(track)), 121);
    }

    void inferredDurationForTrack_flat() {
        QVariantList flat;
        flat.append(QVariantMap{{QStringLiteral("frame"), 0}, {QStringLiteral("value"), 1.0}});
        flat.append(QVariantMap{{QStringLiteral("frame"), 30}, {QStringLiteral("value"), 2.0}});
        QCOMPARE(inferredDurationForTrack(QVariant(flat)), 31);
    }

    void normalizeTrackForDuration_clipsPointsBeyondDuration() {
        QVariantMap start;
        start[QStringLiteral("frame")] = 0;
        start[QStringLiteral("value")] = 0.0;
        QVariantList points;
        points.append(QVariantMap{{QStringLiteral("frame"), 50}, {QStringLiteral("value"), 1.0}});
        points.append(QVariantMap{{QStringLiteral("frame"), 200}, {QStringLiteral("value"), 2.0}});
        QVariantMap track;
        track[QStringLiteral("start")] = start;
        track[QStringLiteral("points")] = points;

        QVariantMap result = normalizeTrackForDuration(QVariant(track), 0.0, 100);
        QVariantList resultPoints = result[QStringLiteral("points")].toList();
        QCOMPARE(resultPoints.size(), 1);
        QCOMPARE(resultPoints[0].toMap()[QStringLiteral("frame")].toInt(), 50);
    }
};

QTEST_MAIN(TestKeyframeEvaluator)
#include "test_keyframe_evaluator.moc"
