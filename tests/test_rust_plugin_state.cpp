#include "rust_plugin_document.hpp"
#include <QTest>

using AviQtl::RustCore::Plugin::CatalogState;
using AviQtl::RustCore::Plugin::Status;

class TestRustPluginState : public QObject {
    Q_OBJECT

  private slots:
    void ownsCatalogOrderingReplacementLookupAndClear();
    void rejectsInvalidAndConflictingIdentity();
};

namespace {
QVariantMap plugin(const QString &id, const QString &path, int amount) {
    return {
        {QStringLiteral("manifest"),
         QVariantMap{{QStringLiteral("id"), id},
                     {QStringLiteral("name"), id + QStringLiteral(" name")}}},
        {QStringLiteral("filePath"), path},
        {QStringLiteral("paramValues"), QVariantMap{{QStringLiteral("amount"), amount}}},
    };
}
} // namespace

void TestRustPluginState::ownsCatalogOrderingReplacementLookupAndClear() {
    CatalogState state;
    QVERIFY(state.isValid());

    const QString alphaPath = QStringLiteral("/plugins/alpha/main.lua");
    QCOMPARE(state.store(plugin(QStringLiteral("plugin.alpha"), alphaPath, 1)), Status::Ok);
    QCOMPARE(state.store(plugin(QStringLiteral("plugin.beta"),
                                QStringLiteral("/plugins/beta/main.lua"), 3)),
             Status::Ok);
    QCOMPARE(state.store(plugin(QStringLiteral("plugin.alpha"), alphaPath, 2)), Status::Ok);

    QVariantList plugins;
    QCOMPARE(state.snapshot(plugins), Status::Ok);
    QCOMPARE(plugins.size(), 2);
    QCOMPARE(plugins.at(0).toMap().value(QStringLiteral("manifest")).toMap().value(QStringLiteral("id")).toString(),
             QStringLiteral("plugin.alpha"));
    QCOMPARE(plugins.at(0).toMap().value(QStringLiteral("paramValues")).toMap().value(QStringLiteral("amount")).toInt(),
             2);
    QCOMPARE(plugins.at(1).toMap().value(QStringLiteral("manifest")).toMap().value(QStringLiteral("id")).toString(),
             QStringLiteral("plugin.beta"));

    QVariantMap found;
    QCOMPARE(state.find(QStringLiteral("plugin.beta"), found), Status::Ok);
    QCOMPARE(found.value(QStringLiteral("filePath")).toString(),
             QStringLiteral("/plugins/beta/main.lua"));
    QCOMPARE(state.find(QStringLiteral("missing"), found), Status::Ok);
    QVERIFY(found.isEmpty());

    QCOMPARE(state.clear(), Status::Ok);
    QCOMPARE(state.snapshot(plugins), Status::Ok);
    QVERIFY(plugins.isEmpty());
}

void TestRustPluginState::rejectsInvalidAndConflictingIdentity() {
    CatalogState state;
    QCOMPARE(state.store({}), Status::InvalidArgument);
    QCOMPARE(state.store(plugin(QStringLiteral("plugin.alpha"),
                                QStringLiteral("/plugins/alpha/main.lua"), 1)),
             Status::Ok);
    QCOMPARE(state.store(plugin(QStringLiteral("plugin.alpha"),
                                QStringLiteral("/plugins/other/main.lua"), 2)),
             Status::InvalidArgument);
    QCOMPARE(state.store(plugin(QStringLiteral("plugin.other"),
                                QStringLiteral("/plugins/alpha/main.lua"), 2)),
             Status::InvalidArgument);
}

QTEST_MAIN(TestRustPluginState)
#include "test_rust_plugin_state.moc"
