#include "rust_timeline_state.hpp"
#include <QJsonDocument>
#include <QJsonObject>
#include <QtTest>

namespace {

QByteArray compact(const QVariantMap &value) {
    return QJsonDocument(QJsonObject::fromVariantMap(value)).toJson(QJsonDocument::Compact);
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
        const QVariantMap transaction = object(transactionBytes);
        const QByteArray forward = compact(transaction.value(QStringLiteral("forward")).toMap());
        const QByteArray inverse = compact(transaction.value(QStringLiteral("inverse")).toMap());
        QCOMPARE(state.applyPatch(forward), AviQtl::RustCore::TimelineStateStatus::Ok);

        QByteArray changedBytes;
        QCOMPARE(state.snapshot(changedBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        const QVariantMap changedClip =
            object(changedBytes).value(QStringLiteral("clips")).toList().first().toMap();
        QCOMPARE(changedClip.value(QStringLiteral("start")).toInt(), 42);
        QCOMPARE(changedClip.value(QStringLiteral("params")).toMap().value(QStringLiteral("text"))
                     .toString(),
                 QStringLiteral("owned by Rust"));

        QCOMPARE(state.applyPatch(inverse), AviQtl::RustCore::TimelineStateStatus::Ok);
        QByteArray restoredBytes;
        QCOMPARE(state.snapshot(restoredBytes), AviQtl::RustCore::TimelineStateStatus::Ok);
        QCOMPARE(object(restoredBytes), before);
        QCOMPARE(state.applyPatch(inverse), AviQtl::RustCore::TimelineStateStatus::StateConflict);
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
    }
};

QTEST_APPLESS_MAIN(RustTimelineStateTest)
#include "test_rust_timeline_state.moc"
