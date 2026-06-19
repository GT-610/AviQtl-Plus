#include "permission_manager.hpp"
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::Core;

class TestPermissionManager : public QObject {
    Q_OBJECT

  private slots:
    void singletonInstance();
    void grantAndCheckPermission();
    void revokePermission();
    void grantAllPermissions();
    void revokeAllPermissions();
    void permissionNames();
    void permissionFromName();
    void allPermissionNames();
    void permissionDescription();
    void pluginAuthorization();
    void permissionPersistence();
};

void TestPermissionManager::singletonInstance() {
    PermissionManager &p1 = PermissionManager::instance();
    PermissionManager &p2 = PermissionManager::instance();
    QCOMPARE(&p1, &p2);
}

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

    QVERIFY(pm.hasPermission(pluginId, PluginPermission::TransportControl));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ClipRead));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ClipModify));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::EffectRead));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::EffectModify));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ProjectRead));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ProjectSave));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ProjectLoad));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::SceneManage));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::SettingsRead));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::SettingsWrite));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ClipboardAccess));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::LogOutput));

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

void TestPermissionManager::permissionNames() {
    QCOMPARE(PermissionManager::permissionName(PluginPermission::TransportControl), QStringLiteral("transport.control"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::ClipRead), QStringLiteral("clip.read"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::ClipModify), QStringLiteral("clip.modify"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::EffectRead), QStringLiteral("effect.read"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::EffectModify), QStringLiteral("effect.modify"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::ProjectRead), QStringLiteral("project.read"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::ProjectSave), QStringLiteral("project.save"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::ProjectLoad), QStringLiteral("project.load"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::SceneManage), QStringLiteral("scene.manage"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::SettingsRead), QStringLiteral("settings.read"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::SettingsWrite), QStringLiteral("settings.write"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::ClipboardAccess), QStringLiteral("clipboard.access"));
    QCOMPARE(PermissionManager::permissionName(PluginPermission::LogOutput), QStringLiteral("log.output"));
}

void TestPermissionManager::permissionFromName() {
    QCOMPARE(PermissionManager::permissionFromName(QStringLiteral("transport.control")), PluginPermission::TransportControl);
    QCOMPARE(PermissionManager::permissionFromName(QStringLiteral("clip.read")), PluginPermission::ClipRead);
    QCOMPARE(PermissionManager::permissionFromName(QStringLiteral("clip.modify")), PluginPermission::ClipModify);
    QCOMPARE(PermissionManager::permissionFromName(QStringLiteral("settings.read")), PluginPermission::SettingsRead);
    QCOMPARE(PermissionManager::permissionFromName(QStringLiteral("log.output")), PluginPermission::LogOutput);
}

void TestPermissionManager::allPermissionNames() {
    QStringList names = PermissionManager::allPermissionNames();
    QCOMPARE(names.size(), 13);
    QVERIFY(names.contains(QStringLiteral("transport.control")));
    QVERIFY(names.contains(QStringLiteral("clip.read")));
    QVERIFY(names.contains(QStringLiteral("clip.modify")));
    QVERIFY(names.contains(QStringLiteral("log.output")));
}

void TestPermissionManager::permissionDescription() {
    // Just verify they return non-empty strings
    QVERIFY(!PermissionManager::permissionDescription(PluginPermission::TransportControl).isEmpty());
    QVERIFY(!PermissionManager::permissionDescription(PluginPermission::ClipRead).isEmpty());
    QVERIFY(!PermissionManager::permissionDescription(PluginPermission::LogOutput).isEmpty());
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

    // Grant permissions and save
    pm.grantPermission(pluginId, PluginPermission::TransportControl);
    pm.grantPermission(pluginId, PluginPermission::ClipRead);
    pm.savePermissions();

    // Verify permissions were saved and can be retrieved
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::TransportControl));
    QVERIFY(pm.hasPermission(pluginId, PluginPermission::ClipRead));

    // Cleanup
    pm.revokeAllPermissions(pluginId);
}

QTEST_MAIN(TestPermissionManager)
#include "test_permission_manager.moc"
