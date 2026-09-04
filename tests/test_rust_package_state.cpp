#include "rust_package_document.hpp"
#include <QTest>

using AviQtl::RustCore::Package::CatalogState;
using AviQtl::RustCore::Package::Status;

class TestRustPackageState : public QObject {
    Q_OBJECT

  private slots:
    void ownsCatalogMergeQueriesAndInstalledState();
};

void TestRustPackageState::ownsCatalogMergeQueriesAndInstalledState() {
    CatalogState state;
    QVERIFY(state.isValid());

    const QVariantList repositories = {
        QVariantMap{{QStringLiteral("url"), QStringLiteral("https://primary")},
                    {QStringLiteral("priority"), 1}},
        QVariantMap{{QStringLiteral("url"), QStringLiteral("https://secondary")},
                    {QStringLiteral("priority"), 10}},
    };
    const QVariantMap installed = {
        {QStringLiteral("org.aviqtl.effect"),
         QVariantMap{{QStringLiteral("version"), QStringLiteral("1.0.0")}}},
    };
    const QVariantList secondaryPackages = {
        QVariantMap{
            {QStringLiteral("id"), QStringLiteral("org.aviqtl.effect")},
            {QStringLiteral("type"), QStringLiteral("effect")},
            {QStringLiteral("version"), QStringLiteral("1.5.0")},
            {QStringLiteral("display_name"),
             QVariantMap{{QStringLiteral("en"), QStringLiteral("Secondary Effect")}}},
            {QStringLiteral("metadata_url"), QStringLiteral("https://secondary/effect.json")},
        },
    };
    QCOMPARE(state.merge(secondaryPackages, repositories.at(1).toMap(), repositories,
                         installed, QStringLiteral("en"), QStringLiteral("0.5.9")),
             Status::Ok);

    const QVariantList primaryPackages = {
        QVariantMap{
            {QStringLiteral("id"), QStringLiteral("org.aviqtl.effect")},
            {QStringLiteral("type"), QStringLiteral("effect")},
            {QStringLiteral("version"), QStringLiteral("2.0.0")},
            {QStringLiteral("display_name"),
             QVariantMap{{QStringLiteral("en"), QStringLiteral("Primary Effect")}}},
            {QStringLiteral("metadata_url"), QStringLiteral("https://primary/effect.json")},
        },
        QVariantMap{
            {QStringLiteral("id"), QStringLiteral("org.aviqtl.object")},
            {QStringLiteral("type"), QStringLiteral("object")},
            {QStringLiteral("version"), QStringLiteral("1.0.0")},
        },
    };
    QCOMPARE(state.merge(primaryPackages, repositories.at(0).toMap(), repositories, installed,
                         QStringLiteral("en"), QStringLiteral("0.5.9")),
             Status::Ok);

    QVariantList snapshot;
    QCOMPARE(state.snapshot(snapshot), Status::Ok);
    QCOMPARE(snapshot.size(), 2);
    QCOMPARE(snapshot.at(0).toMap().value(QStringLiteral("latest_version")).toString(),
             QStringLiteral("2.0.0"));
    QCOMPARE(snapshot.at(0).toMap().value(QStringLiteral("display_name")).toString(),
             QStringLiteral("Primary Effect"));
    QVERIFY(state.hasUpdates());

    QVariantMap package;
    QCOMPARE(state.find(QStringLiteral("org.aviqtl.effect"), QStringLiteral("https://secondary"),
                        package),
             Status::Ok);
    QCOMPARE(package.value(QStringLiteral("id")).toString(),
             QStringLiteral("org.aviqtl.effect"));

    QVariantList effects;
    QCOMPARE(state.filter(QStringLiteral("effect"), effects), Status::Ok);
    QCOMPARE(effects.size(), 1);

    QStringList upgrades;
    QCOMPARE(state.upgradeIds(upgrades), Status::Ok);
    QCOMPARE(upgrades, QStringList{QStringLiteral("org.aviqtl.effect")});

    bool changed = false;
    QCOMPARE(state.setInstalled(QStringLiteral("org.aviqtl.effect"),
                                QStringLiteral("2.0.0"), changed),
             Status::Ok);
    QVERIFY(changed);
    QVERIFY(!state.hasUpdates());
    QCOMPARE(state.setInstalled(QStringLiteral("org.aviqtl.effect"),
                                QStringLiteral("2.0.0"), changed),
             Status::Ok);
    QVERIFY(!changed);

    QCOMPARE(state.clear(), Status::Ok);
    QCOMPARE(state.snapshot(snapshot), Status::Ok);
    QVERIFY(snapshot.isEmpty());
}

QTEST_MAIN(TestRustPackageState)
#include "test_rust_package_state.moc"
