#include "rust_effect_document.hpp"
#include <QTest>

using AviQtl::RustCore::Effect::CatalogState;
using AviQtl::RustCore::Effect::Status;

class TestRustEffectState : public QObject {
    Q_OBJECT

  private slots:
    void ownsCatalogOrderingAndReplacement();
    void rejectsInvalidMetadata();
};

void TestRustEffectState::ownsCatalogOrderingAndReplacement() {
    CatalogState state;
    QVERIFY(state.isValid());

    QVariantMap alpha = {
        {QStringLiteral("id"), QStringLiteral("effect.alpha")},
        {QStringLiteral("name"), QStringLiteral("Alpha")},
        {QStringLiteral("defaultParams"), QVariantMap{{QStringLiteral("amount"), 0.5}}},
    };
    const QVariantMap beta = {
        {QStringLiteral("id"), QStringLiteral("effect.beta")},
        {QStringLiteral("name"), QStringLiteral("Beta")},
    };
    QCOMPARE(state.registerMetadata(alpha), Status::Ok);
    QCOMPARE(state.registerMetadata(beta), Status::Ok);

    alpha.insert(QStringLiteral("name"), QStringLiteral("Alpha 2"));
    QCOMPARE(state.registerMetadata(alpha), Status::Ok);

    QVariantList catalog;
    QCOMPARE(state.snapshot(catalog), Status::Ok);
    QCOMPARE(catalog.size(), 2);
    QCOMPARE(catalog.at(0).toMap().value(QStringLiteral("id")).toString(), QStringLiteral("effect.alpha"));
    QCOMPARE(catalog.at(0).toMap().value(QStringLiteral("name")).toString(), QStringLiteral("Alpha 2"));
    QCOMPARE(catalog.at(0).toMap().value(QStringLiteral("defaultParams")).toMap().value(QStringLiteral("amount")).toDouble(), 0.5);
    QCOMPARE(catalog.at(1).toMap().value(QStringLiteral("id")).toString(), QStringLiteral("effect.beta"));

    QVariantMap found;
    QCOMPARE(state.find(QStringLiteral("effect.beta"), found), Status::Ok);
    QCOMPARE(found.value(QStringLiteral("name")).toString(), QStringLiteral("Beta"));
    QCOMPARE(state.find(QStringLiteral("effect.missing"), found), Status::Ok);
    QVERIFY(found.isEmpty());

    QCOMPARE(state.removeIds({QStringLiteral("effect.alpha")}), Status::Ok);
    QCOMPARE(state.snapshot(catalog), Status::Ok);
    QCOMPARE(catalog.size(), 1);
    QCOMPARE(catalog.at(0).toMap().value(QStringLiteral("id")).toString(), QStringLiteral("effect.beta"));
}

void TestRustEffectState::rejectsInvalidMetadata() {
    CatalogState state;
    QCOMPARE(state.registerMetadata({}), Status::InvalidArgument);
    QCOMPARE(state.registerMetadata({{QStringLiteral("id"), QString()}}), Status::InvalidArgument);
}

QTEST_MAIN(TestRustEffectState)
#include "test_rust_effect_state.moc"
