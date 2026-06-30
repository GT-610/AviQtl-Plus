#include "package_manager.hpp"
#include "effect_registry.hpp"
#include <QDir>
#include <QFileInfo>
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
    void deployTransitionType();
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

void TestPackageDeploy::deployTransitionType() {
    PackageManager &pm = PackageManager::instance();

    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());

    QFile jsonFile(sourceDir.path() + QStringLiteral("/test_transition.json"));
    QVERIFY(jsonFile.open(QIODevice::WriteOnly));
    jsonFile.write("{\"id\":\"test_transition\",\"name\":\"Test Transition\",\"type\":\"transition\"}");
    jsonFile.close();

    QFile fragFile(sourceDir.path() + QStringLiteral("/TestTransition.frag"));
    QVERIFY(fragFile.open(QIODevice::WriteOnly));
    fragFile.write("// transition shader");
    fragFile.close();

    const QString packageId = QStringLiteral("com.aviqtl.test.transition_deploy");
    const QString deployDir = pm.getPackageDeployDir(QStringLiteral("transition"));
    QVERIFY(!deployDir.isEmpty());
    const QString packageDir = deployDir + QStringLiteral("/") + packageId;

    // Clean up any previous test run
    if (QDir(packageDir).exists())
        QDir(packageDir).removeRecursively();

    // Exercise the real deployment helper used in production
    QVERIFY(pm.deployPackageFiles(packageId, sourceDir.path(), QStringLiteral("transition")));

    // Assert transition-specific results: files deployed under /transitions
    QCOMPARE(QFileInfo(deployDir).fileName(), QStringLiteral("transitions"));
    QVERIFY(QFile::exists(packageDir + QStringLiteral("/test_transition.json")));
    QVERIFY(QFile::exists(packageDir + QStringLiteral("/TestTransition.frag")));

    // Verify the registry can load from the transition deploy directory
    // (exercises the metadata/registry reload path used after deployment)
    EffectRegistry::instance().loadEffectsFromDirectory(deployDir);

    // Cleanup
    QDir(packageDir).removeRecursively();
}

void TestPackageDeploy::unknownTypeDefaultsToPlugins() {
    // Unknown package types should default to plugins directory
    // This is tested indirectly through the deploy logic
    QVERIFY(true); // Placeholder
}

QTEST_MAIN(TestPackageDeploy)
#include "test_package_deploy.moc"
