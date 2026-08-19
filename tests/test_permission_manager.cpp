#include "permission_manager.hpp"
#include "settings_manager.hpp"
#include <QCoreApplication>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSet>
#include <QTest>

using namespace AviQtl::Core;

class TestPermissionManager : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void cleanupTestCase();
    void grantAndCheckPermission();
    void revokePermission();
    void grantAllPermissions();
    void revokeAllPermissions();
    void permissionMetadata_data();
    void permissionMetadata();
    void allPermissionNamesAreComplete();
    void unknownPermissionNamesAreRejected();
    void pluginAuthorization();
    void permissionPersistence();
};

namespace {
const QStringList &testPluginIds() {
    static const QStringList ids = {
        QStringLiteral("test.grant"),
        QStringLiteral("test.revoke"),
        QStringLiteral("test.grantall"),
        QStringLiteral("test.revokeall"),
        QStringLiteral("test.unknown"),
        QStringLiteral("test.auth"),
        QStringLiteral("test.persist"),
    };
    return ids;
}

const QList<QPair<PluginPermission, QString>> &permissionCases() {
    static const QList<QPair<PluginPermission, QString>> cases = {
        {PluginPermission::TransportControl, QStringLiteral("transport.control")},
        {PluginPermission::ClipRead, QStringLiteral("clip.read")},
        {PluginPermission::ClipModify, QStringLiteral("clip.modify")},
        {PluginPermission::EffectModify, QStringLiteral("effect.modify")},
        {PluginPermission::ProjectRead, QStringLiteral("project.read")},
        {PluginPermission::ProjectSave, QStringLiteral("project.save")},
        {PluginPermission::ProjectLoad, QStringLiteral("project.load")},
        {PluginPermission::SceneManage, QStringLiteral("scene.manage")},
        {PluginPermission::SettingsRead, QStringLiteral("settings.read")},
        {PluginPermission::SettingsWrite, QStringLiteral("settings.write")},
        {PluginPermission::ClipboardAccess, QStringLiteral("clipboard.access")},
        {PluginPermission::HistoryControl, QStringLiteral("history.control")},
        {PluginPermission::LogOutput, QStringLiteral("log.output")},
    };
    return cases;
}

void clearTestPermissions() {
    PermissionManager &pm = PermissionManager::instance();
    for (const QString &pluginId : testPluginIds()) {
        pm.revokeAllPermissions(pluginId);
    }
}
} // namespace

void TestPermissionManager::initTestCase() { clearTestPermissions(); }

void TestPermissionManager::cleanupTestCase() { clearTestPermissions(); }

void TestPermissionManager::grantAndCheckPermission() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.grant");

    // Initially no permissions
    QVERIFY(!pm.hasPermission(pluginId, PluginPermission::TransportControl));

    // Grant permission
    pm.grantPermission(pluginId, PluginPermission::TransportControl);
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::TransportControl));

    // Other permissions still not granted
    QVERIFY(!pm.hasPermission(pluginId, PluginPermission::ClipModify));

    // Cleanup
    pm.revokeAllPermissions(pluginId);
}

void TestPermissionManager::revokePermission() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.revoke");

    pm.grantPermission(pluginId, PluginPermission::SettingsRead);
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::SettingsRead));

    pm.revokePermission(pluginId, PluginPermission::SettingsRead);
    QVERIFY(!pm.hasPermission(pluginId, PluginPermission::SettingsRead));

    // Revoking non-existent permission should not crash
    pm.revokePermission(pluginId, PluginPermission::SettingsWrite);
}

void TestPermissionManager::grantAllPermissions() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.grantall");

    pm.grantAllPermissions(pluginId);

    for (const QString &permissionName : PermissionManager::allPermissionNames())
        QVERIFY2(pm.hasPermission(pluginId, permissionName), qPrintable(permissionName));

    pm.revokeAllPermissions(pluginId);
}

void TestPermissionManager::revokeAllPermissions() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.revokeall");

    pm.grantPermission(pluginId, PluginPermission::TransportControl);
    pm.grantPermission(pluginId, PluginPermission::ClipRead);
    QVERIFY(pm.isPluginAuthorized(pluginId));

    pm.revokeAllPermissions(pluginId);
    QVERIFY(!pm.isPluginAuthorized(pluginId));
    QVERIFY(!pm.hasPermission(pluginId, PluginPermission::TransportControl));
    QVERIFY(!pm.hasPermission(pluginId, PluginPermission::ClipRead));
}

void TestPermissionManager::permissionMetadata_data() {
    QTest::addColumn<int>("permissionValue");
    QTest::addColumn<QString>("permissionName");
    for (const auto &[permission, name] : permissionCases())
        QTest::newRow(qPrintable(name)) << static_cast<int>(permission) << name;
}

void TestPermissionManager::permissionMetadata() {
    QFETCH(int, permissionValue);
    QFETCH(QString, permissionName);
    const auto permission = static_cast<PluginPermission>(permissionValue);
    QCOMPARE(PermissionManager::permissionName(permission), permissionName);
    const auto resolved = PermissionManager::permissionFromName(permissionName);
    QVERIFY(resolved.has_value());
    QCOMPARE(*resolved, permission);
}

void TestPermissionManager::allPermissionNamesAreComplete() {
    QSet<QString> expected;
    for (const auto &[permission, name] : permissionCases()) {
        Q_UNUSED(permission);
        expected.insert(name);
    }
    const QStringList actualNames = PermissionManager::allPermissionNames();
    QCOMPARE(actualNames.size(), expected.size());
    QCOMPARE(QSet<QString>(actualNames.cbegin(), actualNames.cend()), expected);
}

void TestPermissionManager::unknownPermissionNamesAreRejected() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.unknown");
    const QString unknown = QStringLiteral("unknown.permission");
    QVERIFY(!PermissionManager::permissionFromName(unknown).has_value());
    QVERIFY(!pm.hasPermission(pluginId, unknown));
    pm.grantPermission(pluginId, unknown);
    QVERIFY(!pm.isPluginAuthorized(pluginId));
}

void TestPermissionManager::pluginAuthorization() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.auth");

    // Not authorized initially
    QVERIFY(!pm.isPluginAuthorized(pluginId));

    // Grant one permission
    pm.grantPermission(pluginId, PluginPermission::LogOutput);
    QVERIFY(pm.isPluginAuthorized(pluginId));

    // Revoke all
    pm.revokeAllPermissions(pluginId);
    QVERIFY(!pm.isPluginAuthorized(pluginId));
}

void TestPermissionManager::permissionPersistence() {
    PermissionManager &pm = PermissionManager::instance();
    const QString pluginId = QStringLiteral("test.persist");
    const QString settingsPath = QCoreApplication::applicationDirPath() + QStringLiteral("/aviqtl_settings.json");

    pm.grantPermission(pluginId, PluginPermission::TransportControl);
    pm.grantPermission(pluginId, PluginPermission::ClipRead);
    QFile persistedFile(settingsPath);
    QVERIFY2(persistedFile.open(QIODevice::ReadOnly), qPrintable(persistedFile.errorString()));
    const QByteArray persistedPayload = persistedFile.readAll();
    persistedFile.close();
    const QJsonObject persistedSettings = QJsonDocument::fromJson(persistedPayload).object();
    const QJsonObject persistedPermissions = persistedSettings.value(QStringLiteral("pluginPermissions")).toObject();
    const QJsonArray persistedPluginPermissions = persistedPermissions.value(pluginId).toArray();
    QCOMPARE(persistedPluginPermissions.size(), 2);

    pm.revokeAllPermissions(pluginId);
    QVERIFY(!pm.isPluginAuthorized(pluginId));

    QVERIFY(persistedFile.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(persistedFile.write(persistedPayload), persistedPayload.size());
    persistedFile.close();
    SettingsManager::instance().load();
    pm.loadPermissions();

    QVERIFY(pm.hasPermission(pluginId, PluginPermission::TransportControl));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ClipRead));
    pm.revokeAllPermissions(pluginId);
}

QTEST_MAIN(TestPermissionManager)
#include "test_permission_manager.moc"
