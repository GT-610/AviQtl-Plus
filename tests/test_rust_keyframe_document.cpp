#include "rust_core_abi.hpp"
#include "rust_keyframe_document.hpp"
#include <QJsonDocument>
#include <QTest>
#include <array>

using namespace AviQtl::Core::RustKeyframeDocument;

class TestRustKeyframeDocument : public QObject {
    Q_OBJECT

  private slots:
    void normalizesLegacyTracksAndPreservesScalarTypes() {
        const QVariantList legacy{
            QVariantMap{{QStringLiteral("frame"), 20},
                        {QStringLiteral("value"), 20},
                        {QStringLiteral("interp"), QStringLiteral("none")}},
            QVariantMap{{QStringLiteral("frame"), 0},
                        {QStringLiteral("value"), 0},
                        {QStringLiteral("interp"), QStringLiteral("linear")}},
            QVariantMap{{QStringLiteral("frame"), 10},
                        {QStringLiteral("value"), 10},
                        {QStringLiteral("interp"), QStringLiteral("linear")}},
        };
        const auto result = inspect(legacy, 5, 21);
        QVERIFY(result.has_value());
        QCOMPARE(result->flat.size(), 3);
        QCOMPARE(result->flat.at(0).toMap().value(QStringLiteral("frame")).toInt(), 0);
        QCOMPARE(result->flat.at(2).toMap().value(QStringLiteral("frame")).toInt(), 20);
        QCOMPARE(result->flat.at(0).toMap().value(QStringLiteral("value")).typeId(),
                 QMetaType::Int);
        QCOMPARE(result->flat.at(2).toMap().value(QStringLiteral("value")).typeId(),
                 QMetaType::Int);
    }

    void mutationsUseOneCanonicalTrackModel() {
        const QVariantMap start{
            {QStringLiteral("frame"), 0},
            {QStringLiteral("value"), 0.0},
            {QStringLiteral("interp"), QStringLiteral("linear")},
        };
        const QVariantMap track{
            {QStringLiteral("start"), start},
            {QStringLiteral("points"),
             QVariantList{QVariantMap{{QStringLiteral("frame"), 10},
                                      {QStringLiteral("value"), 100.0},
                                      {QStringLiteral("interp"), QStringLiteral("none")}}}},
        };
        const QVariantList customPoints{0.33, 0.0, 0.66, 1.0, 1.0, 1.0};
        const QVariantMap modeParams{{QStringLiteral("stepFrames"), 3}};
        const auto inserted = set(track, 0.0, 20, 5, 50.0,
                                  {{QStringLiteral("interp"), QStringLiteral("custom")},
                                   {QStringLiteral("points"), customPoints},
                                   {QStringLiteral("modeParams"), modeParams}});
        QVERIFY(inserted && inserted->accepted && inserted->changed);
        QCOMPARE(inserted->flat.size(), 3);
        QCOMPARE(inserted->flat.at(1).toMap().value(QStringLiteral("points")).toList(),
                 customPoints);

        const auto moved = move(inserted->track, 0.0, 20, 5, 8);
        QVERIFY(moved && moved->accepted && moved->changed);
        QCOMPARE(moved->flat.at(1).toMap().value(QStringLiteral("frame")).toInt(), 8);

        const auto removed = remove(moved->track, 0.0, 20, 8);
        QVERIFY(removed && removed->accepted && removed->changed);
        QCOMPARE(removed->flat.size(), 2);
    }

    void syncAndSplitRemainDeterministic() {
        const QVariantMap track{
            {QStringLiteral("start"),
             QVariantMap{{QStringLiteral("frame"), 0},
                         {QStringLiteral("value"), 0.0},
                         {QStringLiteral("interp"), QStringLiteral("linear")}}},
            {QStringLiteral("points"),
             QVariantList{
                 QVariantMap{{QStringLiteral("frame"), 10},
                             {QStringLiteral("value"), 100.0},
                             {QStringLiteral("interp"), QStringLiteral("linear")}},
                 QVariantMap{{QStringLiteral("frame"), 20},
                             {QStringLiteral("value"), 200.0},
                             {QStringLiteral("interp"), QStringLiteral("none")}},
             }},
        };
        const auto synced = sync(track, 0.0, 20, 30);
        QVERIFY(synced && synced->accepted);
        QCOMPARE(synced->flat.last().toMap().value(QStringLiteral("frame")).toInt(), 30);

        const auto splitResult = split(track, 0.0, 10, 21);
        QVERIFY(splitResult && splitResult->accepted && splitResult->secondaryTrack);
        QCOMPARE(splitResult->secondaryTrack->value(QStringLiteral("start"))
                     .toMap()
                     .value(QStringLiteral("value"))
                     .toDouble(),
                 100.0);
    }

    void undersizedBufferDoesNotReceivePartialJson() {
        const QByteArray request = QJsonDocument::fromVariant(QVariantMap{
            {QStringLiteral("operation"), QStringLiteral("normalize")},
            {QStringLiteral("track"), QVariantList{}},
            {QStringLiteral("fallback"), 1.0},
            {QStringLiteral("duration"), 10},
        }).toJson(QJsonDocument::Compact);
        std::size_t required = 0;
        QCOMPARE(aviqtl_keyframe_document_apply_json(
                     reinterpret_cast<const std::uint8_t *>(request.constData()),
                     static_cast<std::size_t>(request.size()), nullptr, 0, &required),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL});
        QVERIFY(required > 1);

        QByteArray output(static_cast<qsizetype>(required - 1), '\x5a');
        const QByteArray original = output;
        std::size_t reported = 0;
        QCOMPARE(aviqtl_keyframe_document_apply_json(
                     reinterpret_cast<const std::uint8_t *>(request.constData()),
                     static_cast<std::size_t>(request.size()),
                     reinterpret_cast<std::uint8_t *>(output.data()),
                     static_cast<std::size_t>(output.size()), &reported),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL});
        QCOMPARE(reported, required);
        QCOMPARE(output, original);
    }
};

QTEST_MAIN(TestRustKeyframeDocument)
#include "test_rust_keyframe_document.moc"
