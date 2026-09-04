#include "rust_timeline_state.hpp"
#include <QJsonDocument>
#include <QJsonObject>
#include <QtTest>
#include <utility>

namespace {

QByteArray compact(const QVariantMap &value) {
    return QJsonDocument(QJsonObject::fromVariantMap(value)).toJson(QJsonDocument::Compact);
}

QByteArray compact(const QVariantList &value) {
    return QJsonDocument::fromVariant(value).toJson(QJsonDocument::Compact);
}

QVariantMap object(const QByteArray &value) {
    return QJsonDocument::fromJson(value).object().toVariantMap();
}

QVariantMap projectDocument() {
    return {
        {QStringLiteral("version"), 3},
        {QStringLiteral("settings"),
         QVariantMap{{QStringLiteral("width"), 1920},
                     {QStringLiteral("height"), 1080},
                     {QStringLiteral("fps"), 60.0},
                     {QStringLiteral("sampleRate"), 48000}}},
        {QStringLiteral("scenes"),
         QVariantList{QVariantMap{{QStringLiteral("id"), 0},
                                  {QStringLiteral("name"), QStringLiteral("Root")},
                                  {QStringLiteral("width"), 1920},
                                  {QStringLiteral("height"), 1080},
                                  {QStringLiteral("fps"), 60.0},
                                  {QStringLiteral("start"), 0},
                                  {QStringLiteral("duration"), 300},
                                  {QStringLiteral("nestedDuration"), 0},
                                  {QStringLiteral("lockedLayers"), QVariantList{}},
                                  {QStringLiteral("hiddenLayers"), QVariantList{}},
                                  {QStringLiteral("gridMode"), QStringLiteral("Auto")},
                                  {QStringLiteral("gridBpm"), 120.0},
                                  {QStringLiteral("gridOffset"), 0.0},
                                  {QStringLiteral("gridInterval"), 10},
                                  {QStringLiteral("gridSubdivision"), 4},
                                  {QStringLiteral("enableSnap"), true},
                                  {QStringLiteral("magneticSnapRange"), 10}}}},
        {QStringLiteral("clips"),
         QVariantList{QVariantMap{{QStringLiteral("id"), 1},
                                  {QStringLiteral("sceneId"), 0},
                                  {QStringLiteral("type"), QStringLiteral("text")},
                                  {QStringLiteral("start"), 0},
                                  {QStringLiteral("duration"), 30},
                                  {QStringLiteral("layer"), 0},
                                  {QStringLiteral("clipByUpperObject"), false},
                                  {QStringLiteral("params"), QVariantMap{}},
                                  {QStringLiteral("audioPlugins"), QVariantList{}},
                                  {QStringLiteral("effects"), QVariantList{}}}}},
    };
}

} // namespace

class RustTimelineStateTest : public QObject {
    Q_OBJECT

  private slots:
    void reversibleClipTransaction() {
        AviQtl::RustCore::TimelineState state;
        QCOMPARE(state.reset(compact(projectDocument()), 2, 1),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.nextClipId(), 2);

        QByteArray beforeBytes;
        QCOMPARE(state.snapshot(beforeBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        const QVariantMap before = object(beforeBytes);
        QVariantMap clip = before.value(QStringLiteral("clips")).toList().first().toMap();
        clip.insert(QStringLiteral("start"), 42);
        clip.insert(QStringLiteral("params"),
                    QVariantMap{{QStringLiteral("text"), QStringLiteral("owned by Rust")}});
        const QVariantMap request{
            {QStringLiteral("operation"), QStringLiteral("replace_clip")},
            {QStringLiteral("clip_id"), 1},
            {QStringLiteral("clip"), clip},
        };

        QByteArray transactionBytes;
        QCOMPARE(state.plan(compact(request), transactionBytes),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(transactionBytes, true),
                 AviQtl::RustCore::TimelineStateStatus::Ok);

        QByteArray changedBytes;
        QCOMPARE(state.snapshot(changedBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        const QVariantMap changedClip =
            object(changedBytes).value(QStringLiteral("clips")).toList().first().toMap();
        QCOMPARE(changedClip.value(QStringLiteral("start")).toInt(), 42);
        QCOMPARE(changedClip.value(QStringLiteral("params")).toMap().value(QStringLiteral("text"))
                     .toString(),
                 QStringLiteral("owned by Rust"));

        QCOMPARE(state.applyTransaction(transactionBytes, false),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QByteArray restoredBytes;
        QCOMPARE(state.snapshot(restoredBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(object(restoredBytes), before);
        QCOMPARE(state.applyTransaction(transactionBytes, false),
                 AviQtl::RustCore::TimelineStateStatus::StateConflict);
    }

    void batchPlanningAndCombinationStayOpaque() {
        AviQtl::RustCore::TimelineState state;
        QCOMPARE(state.reset(compact(projectDocument()), 2, 1),
                 AviQtl::RustCore::TimelineStateStatus::Ok);

        const QVariantMap geometryRequest{
            {QStringLiteral("operation"), QStringLiteral("update_clip_geometry")},
            {QStringLiteral("clip_id"), 1},
            {QStringLiteral("layer"), 2},
            {QStringLiteral("start"), 14},
            {QStringLiteral("duration"), 20},
        };
        const QVariantMap compositingRequest{
            {QStringLiteral("operation"), QStringLiteral("set_clip_by_upper_object")},
            {QStringLiteral("clip_id"), 1},
            {QStringLiteral("enabled"), true},
        };

        QByteArray batch;
        QCOMPARE(state.planBatch(compact(QVariantList{geometryRequest, compositingRequest}), batch),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(batch, true),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QByteArray changedBytes;
        QCOMPARE(state.snapshot(changedBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        const QVariantMap changedClip =
            object(changedBytes).value(QStringLiteral("clips")).toList().first().toMap();
        QCOMPARE(changedClip.value(QStringLiteral("layer")).toInt(), 2);
        QCOMPARE(changedClip.value(QStringLiteral("start")).toInt(), 14);
        QVERIFY(changedClip.value(QStringLiteral("clipByUpperObject")).toBool());
        QCOMPARE(state.applyTransaction(batch, false),
                 AviQtl::RustCore::TimelineStateStatus::Ok);

        QByteArray first;
        QCOMPARE(state.plan(compact(geometryRequest), first),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(first, true),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QByteArray second;
        QCOMPARE(state.plan(compact(compositingRequest), second),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(second, true),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QByteArray combined;
        QCOMPARE(state.combineTransactions(first, second, combined),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(second, false),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(first, false),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(combined, true),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.applyTransaction(combined, false),
                 AviQtl::RustCore::TimelineStateStatus::Ok);

        QByteArray restoredBytes;
        QCOMPARE(state.snapshot(restoredBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(object(restoredBytes), projectDocument());
    }

    void reservesIdsInsideTheState() {
        AviQtl::RustCore::TimelineState state;
        QCOMPARE(state.reset(compact(projectDocument()), 1, 1),
                 AviQtl::RustCore::TimelineStateStatus::Ok);
        std::vector<std::int32_t> ids;
        QCOMPARE(state.reserveClipIds(3, ids), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(ids, std::vector<std::int32_t>({2, 3, 4}));
        QCOMPARE(state.nextClipId(), 5);
        QCOMPARE(state.setNextClipHint(12), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(state.nextClipId(), 12);

        QCOMPARE(state.reserveSceneIds(2, ids), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(ids, std::vector<std::int32_t>({1, 2}));
        QCOMPARE(state.nextSceneId(), 3);
    }

    void defaultConstructedStateRejectsHandleOperations() {
        AviQtl::RustCore::TimelineState state;
        QByteArray output;
        std::vector<std::int32_t> ids;
        QCOMPARE(state.snapshot(output), AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.plan(QByteArrayLiteral("{}"), output),
                 AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.planBatch(QByteArrayLiteral("[]"), output),
                 AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.applyTransaction(QByteArrayLiteral("{}"), true),
                 AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.reserveClipIds(1, ids),
                 AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.reserveSceneIds(1, ids),
                 AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.setNextClipHint(4),
                 AviQtl::RustCore::TimelineStateStatus::InvalidArgument);
        QCOMPARE(state.nextClipId(), -1);
        QCOMPARE(state.nextSceneId(), -1);
    }

    void moveTransfersStateOwnership() {
        AviQtl::RustCore::TimelineState source;
        QCOMPARE(source.reset(compact(projectDocument()), 2, 1),
                 AviQtl::RustCore::TimelineStateStatus::Ok);

        AviQtl::RustCore::TimelineState constructed(std::move(source));
        QVERIFY(!source.isValid());
        QVERIFY(constructed.isValid());
        QCOMPARE(constructed.nextClipId(), 2);

        AviQtl::RustCore::TimelineState assigned;
        assigned = std::move(constructed);
        QVERIFY(!constructed.isValid());
        QVERIFY(assigned.isValid());
        QByteArray snapshot;
        QCOMPARE(assigned.snapshot(snapshot), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(object(snapshot), projectDocument());
    }
};

QTEST_APPLESS_MAIN(RustTimelineStateTest)
#include "test_rust_timeline_state.moc"
