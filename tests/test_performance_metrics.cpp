#include "performance_metrics.hpp"

#include <QJsonObject>
#include <QTest>

using namespace AviQtl::Core;

class TestPerformanceMetrics : public QObject {
    Q_OBJECT

  private slots:
    void init() { PerformanceMetrics::instance().reset(); }

    void resetAndSnapshot() {
        auto &metrics = PerformanceMetrics::instance();
        metrics.add(PerformanceCounter::BakeCalls);
        metrics.add(PerformanceCounter::BakeClipsVisited, 7);

        const PerformanceSnapshot populated = metrics.snapshot();
        QCOMPARE(populated.value(PerformanceCounter::BakeCalls), quint64{1});
        QCOMPARE(populated.value(PerformanceCounter::BakeClipsVisited), quint64{7});

        metrics.reset();
        const PerformanceSnapshot cleared = metrics.snapshot();
        QCOMPARE(cleared.value(PerformanceCounter::BakeCalls), quint64{0});
        QCOMPARE(cleared.value(PerformanceCounter::BakeClipsVisited), quint64{0});
    }

    void reportIncludesScenarioContextAndCounters() {
        auto &metrics = PerformanceMetrics::instance();
        metrics.add(PerformanceCounter::DecodeRequests, 3);
        metrics.add(PerformanceCounter::DecodeRequestsCoalesced, 2);

        const QJsonObject context{{QStringLiteral("frames"), 120}, {QStringLiteral("objects"), 48}};
        const QJsonObject report = metrics.report(QStringLiteral("timeline-scrub"), context);

        QCOMPARE(report.value(QStringLiteral("scenario")).toString(), QStringLiteral("timeline-scrub"));
        QCOMPARE(report.value(QStringLiteral("context")).toObject(), context);
        const QJsonObject counters = report.value(QStringLiteral("metrics")).toObject();
        QCOMPARE(counters.value(QStringLiteral("decode_requests")).toInteger(), qint64{3});
        QCOMPARE(counters.value(QStringLiteral("decode_requests_coalesced")).toInteger(), qint64{2});
        QVERIFY(counters.contains(QStringLiteral("rhi_pipeline_rebuilds")));
        QVERIFY(counters.contains(QStringLiteral("export_frame_wait_nanoseconds")));
    }
};

QTEST_MAIN(TestPerformanceMetrics)
#include "test_performance_metrics.moc"
