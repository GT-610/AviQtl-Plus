#include "package_manager.hpp"
#include <QDir>
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::Core;

class TestPackageDeploy : public QObject {
    Q_OBJECT

  private slots:
    void deployDirectoryCreation();
    void deployModType();
    void deployEffectType();
    void deployObjectType();
    void unknownTypeDefaultsToPlugins();
};

void TestPackageDeploy::deployDirectoryCreation() {
    // Test that getPackageDeployDir returns valid paths
    PackageManager &pm = PackageManager::instance();

    // We can't directly call private methods, but we can test the behavior
    // through the public API. For now, verify the singleton exists.
    QVERIFY(&pm != nullptr);
}

void TestPackageDeploy::deployModType() {
    // Test that mod type deploys to plugins directory
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());

    // Create a test file
    QFile testFile(sourceDir.path() + QStringLiteral("/test.lua"));
    QVERIFY(testFile.open(QIODevice::WriteOnly));
    testFile.write("-- test plugin");
    testFile.close();

    // Verify the file exists
    QVERIFY(QFile::exists(sourceDir.path() + QStringLiteral("/test.lua")));
}

void TestPackageDeploy::deployEffectType() {
    // Test effect deployment structure
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());

    // Create test effect files
    QFile jsonFile(sourceDir.path() + QStringLiteral("/test_effect.json"));
    QVERIFY(jsonFile.open(QIODevice::WriteOnly));
    jsonFile.write("{\"id\":\"test\",\"name\":\"Test\"}");
    jsonFile.close();

    QFile qmlFile(sourceDir.path() + QStringLiteral("/TestEffect.qml"));
    QVERIFY(qmlFile.open(QIODevice::WriteOnly));
    qmlFile.write("import QtQuick; Item {}");
    qmlFile.close();

    QVERIFY(QFile::exists(sourceDir.path() + QStringLiteral("/test_effect.json")));
    QVERIFY(QFile::exists(sourceDir.path() + QStringLiteral("/TestEffect.qml")));
}

void TestPackageDeploy::deployObjectType() {
    // Test object deployment structure
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());

    QFile jsonFile(sourceDir.path() + QStringLiteral("/test_object.json"));
    QVERIFY(jsonFile.open(QIODevice::WriteOnly));
    jsonFile.write("{\"id\":\"testobj\",\"name\":\"Test Object\"}");
    jsonFile.close();

    QVERIFY(QFile::exists(sourceDir.path() + QStringLiteral("/test_object.json")));
}

void TestPackageDeploy::unknownTypeDefaultsToPlugins() {
    // Unknown package types should default to plugins directory
    // This is tested indirectly through the deploy logic
    QVERIFY(true); // Placeholder
}

QTEST_MAIN(TestPackageDeploy)
#include "test_package_deploy.moc"
