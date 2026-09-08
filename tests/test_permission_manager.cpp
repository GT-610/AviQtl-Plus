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
    void permissionPersistence();
};

namespace {
const QStringList &testPluginIds() {
    static const QStringList ids = {
        QStringLiteral("test.grant"),
        QStringLiteral("test.persist"),
    };
    return ids;
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
