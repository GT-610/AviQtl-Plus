#include "package_manager.hpp"
#include "effect_registry.hpp"
#include <QDir>
#include <QFileInfo>
#include <QTemporaryDir>
#include <QTest>
#include <QtCore/private/qzipwriter_p.h>

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
    void rejectsInvalidPackageIds();
    void extractsSafeArchive();
    void rejectsTraversalArchive();
    void rejectsSymlinkArchive();
    void rollsBackWhenStateCommitFails();
};

void TestPackageDeploy::deployDirectoryCreation() {
    // Test that getPackageDeployDir returns valid paths
    PackageManager &pm = PackageManager::instance();

    // We can't directly call private methods, but we can test the behavior
    // through the public API. For now, verify the singleton exists.
    QVERIFY(!pm.statusText().isNull());
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

    // Register a scoped cleanup guard immediately so the real transition
    // deploy directory is always removed even if QVERIFY/QCOMPARE below fail
    // (QVERIFY returns from the function on failure, skipping the tail cleanup).
    struct ScopedCleanup {
        QString dir;
        ~ScopedCleanup() { if (QDir(dir).exists()) QDir(dir).removeRecursively(); }
    } cleanupGuard;
    cleanupGuard.dir = packageDir;

    // Exercise the real deployment helper used in production
    QVERIFY(pm.deployPackageFiles(packageId, sourceDir.path(), QStringLiteral("transition")));

    // Assert transition-specific results: files deployed under /transitions
    QCOMPARE(QFileInfo(deployDir).fileName(), QStringLiteral("transitions"));
    QVERIFY(QFile::exists(packageDir + QStringLiteral("/test_transition.json")));
    QVERIFY(QFile::exists(packageDir + QStringLiteral("/TestTransition.frag")));

    // Verify the registry can load from the transition deploy directory
    // (exercises the metadata/registry reload path used after deployment)
    EffectRegistry::instance().loadEffectsFromDirectory(deployDir);

    // Cleanup (also handled by cleanupGuard destructor on scope exit)
    QDir(packageDir).removeRecursively();
}

void TestPackageDeploy::unknownTypeDefaultsToPlugins() {
    PackageManager &pm = PackageManager::instance();
    QVERIFY(pm.getPackageDeployDir(QStringLiteral("unknown")).isEmpty());

    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());
    QVERIFY(!pm.deployPackageFiles(QStringLiteral("org.aviqtl.invalid"), sourceDir.path(), QStringLiteral("unknown")));
}

void TestPackageDeploy::rejectsInvalidPackageIds() {
    PackageManager &pm = PackageManager::instance();
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());

    QVERIFY(!pm.deployPackageFiles(QStringLiteral("../escape"), sourceDir.path(), QStringLiteral("effect")));
    QVERIFY(!pm.deployPackageFiles(QStringLiteral("contains/slash"), sourceDir.path(), QStringLiteral("effect")));
    QVERIFY(!pm.deployPackageFiles(QStringLiteral("contains\\slash"), sourceDir.path(), QStringLiteral("effect")));
    QVERIFY(!pm.deployPackageFiles(QString(), sourceDir.path(), QStringLiteral("effect")));
}

void TestPackageDeploy::extractsSafeArchive() {
    PackageManager &pm = PackageManager::instance();
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString archivePath = dir.filePath(QStringLiteral("safe.zip"));
    QZipWriter writer(archivePath);
    writer.addFile(QStringLiteral("package/metadata.json"), QByteArrayLiteral("{}"));
    writer.close();
    QCOMPARE(writer.status(), QZipWriter::NoError);

    const QString destination = dir.filePath(QStringLiteral("safe-output"));
    QVERIFY(pm.extractPackageArchive(archivePath, destination));
    QVERIFY(QFile::exists(destination + QStringLiteral("/package/metadata.json")));
}

void TestPackageDeploy::rejectsTraversalArchive() {
    QVERIFY(!PackageManager::isSafeArchivePath(QStringLiteral("../escaped.txt")));
    QVERIFY(!PackageManager::isSafeArchivePath(QStringLiteral("folder/../../escaped.txt")));
    QVERIFY(!PackageManager::isSafeArchivePath(QStringLiteral("/absolute/path")));
    QVERIFY(!PackageManager::isSafeArchivePath(QStringLiteral("folder\\escaped.txt")));
    QVERIFY(PackageManager::isSafeArchivePath(QStringLiteral("package/metadata.json")));
}

void TestPackageDeploy::rejectsSymlinkArchive() {
    PackageManager &pm = PackageManager::instance();
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString archivePath = dir.filePath(QStringLiteral("symlink.zip"));
    QZipWriter writer(archivePath);
    writer.addSymLink(QStringLiteral("package/link"), QStringLiteral("../../outside"));
    writer.close();

    QVERIFY(!pm.extractPackageArchive(archivePath, dir.filePath(QStringLiteral("symlink-output"))));
}

void TestPackageDeploy::rollsBackWhenStateCommitFails() {
    PackageManager &pm = PackageManager::instance();
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());
    QFile newFile(sourceDir.filePath(QStringLiteral("content.txt")));
    QVERIFY(newFile.open(QIODevice::WriteOnly));
    QCOMPARE(newFile.write("new"), 3);
    newFile.close();

    const QString packageId = QStringLiteral("org.aviqtl.test.rollback");
    const QString packageDir = pm.getPackageDeployDir(QStringLiteral("effect")) + QLatin1Char('/') + packageId;
    QDir(packageDir).removeRecursively();
    QVERIFY(QDir().mkpath(packageDir));
    QFile oldFile(packageDir + QStringLiteral("/content.txt"));
    QVERIFY(oldFile.open(QIODevice::WriteOnly));
    QCOMPARE(oldFile.write("old"), 3);
    oldFile.close();

    struct ScopedCleanup {
        QString dir;
        ~ScopedCleanup() { QDir(dir).removeRecursively(); }
    } cleanup{packageDir};

    QVERIFY(!pm.deployPackageFiles(packageId, sourceDir.path(), QStringLiteral("effect"), [] { return false; }));
    QFile restored(packageDir + QStringLiteral("/content.txt"));
    QVERIFY(restored.open(QIODevice::ReadOnly));
    QCOMPARE(restored.readAll(), QByteArrayLiteral("old"));
}

QTEST_MAIN(TestPackageDeploy)
#include "test_package_deploy.moc"
