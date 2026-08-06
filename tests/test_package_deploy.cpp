#include "package_manager.hpp"
#include "core/src/package_deployment.hpp"
#include "core/src/package_url_utils.hpp"
#include "settings_manager.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QtCore/private/qzipwriter_p.h>

using namespace AviQtl::Core;
using PackageDeployment = AviQtl::Core::Internal::PackageDeployment;

class TestPackageDeploy : public QObject {
    Q_OBJECT

  private slots:
    void deploysPackageFiles_data();
    void deploysPackageFiles();
    void rejectsUnknownPackageType();
    void rejectsInvalidPackageIds();
    void extractsSafeArchive();
    void rejectsTraversalArchive();
    void rejectsSymlinkArchive();
    void rollsBackWhenStateCommitFails();
    void removesPackageTransactionally();
    void rejectsRemovalWithUnknownType();
    void resolvesRepositoryReferencesSafely();
    void managesRepositoriesAndPersistsThem();
    void syncWithNoEnabledRepositoriesFinishesCleanly();
};

namespace {
struct SettingsSnapshot {
    QVariantMap values;
    QString path;
    bool fileExisted = false;
    QByteArray payload;
};

SettingsSnapshot captureSettings() {
    SettingsSnapshot snapshot;
    snapshot.values = SettingsManager::instance().settings();
    snapshot.path = QCoreApplication::applicationDirPath() + QStringLiteral("/aviqtl_settings.json");
    QFile file(snapshot.path);
    snapshot.fileExisted = file.exists();
    if (snapshot.fileExisted && file.open(QIODevice::ReadOnly)) {
        snapshot.payload = file.readAll();
    }
    return snapshot;
}

void restoreSettings(const SettingsSnapshot &snapshot) {
    SettingsManager::instance().setSettings(snapshot.values);
    if (!snapshot.fileExisted) {
        QFile::remove(snapshot.path);
        return;
    }
    QFile file(snapshot.path);
    if (file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        file.write(snapshot.payload);
    }
}

bool containsTransactionDirectory(const QString &deployDir, const QString &packageId) {
    QDirIterator it(deployDir, QDir::Dirs | QDir::NoDotAndDotDot, QDirIterator::Subdirectories);
    while (it.hasNext()) {
        const QString name = QFileInfo(it.next()).fileName();
        if (name.startsWith(QStringLiteral(".staging_")) || name == QStringLiteral(".backup_") + packageId ||
            name == QStringLiteral(".remove_backup_") + packageId) {
            return true;
        }
    }
    return false;
}
} // namespace

void TestPackageDeploy::deploysPackageFiles_data() {
    QTest::addColumn<QString>("packageType");
    QTest::addColumn<QString>("directoryName");
    QTest::newRow("mod") << QStringLiteral("mod") << QStringLiteral("plugins");
    QTest::newRow("effect") << QStringLiteral("effect") << QStringLiteral("effects");
    QTest::newRow("object") << QStringLiteral("object") << QStringLiteral("objects");
}

void TestPackageDeploy::deploysPackageFiles() {
    QFETCH(QString, packageType);
    QFETCH(QString, directoryName);
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());
    const QString wrappedSource = sourceDir.filePath(QStringLiteral("package"));
    QVERIFY(QDir().mkpath(wrappedSource));

    QFile payload(QDir(wrappedSource).filePath(QStringLiteral("payload.txt")));
    QVERIFY(payload.open(QIODevice::WriteOnly));
    QCOMPARE(payload.write(packageType.toUtf8()), packageType.toUtf8().size());
    payload.close();

    const QString packageId = QStringLiteral("org.aviqtl.test.deploy_") + packageType;
    const QString deployDir = PackageDeployment::deployDirectory(packageType);
    QVERIFY(!deployDir.isEmpty());
    QCOMPARE(QFileInfo(deployDir).fileName(), directoryName);
    const QString packageDir = QDir(deployDir).filePath(packageId);
    QDir(packageDir).removeRecursively();
    QVERIFY(QDir().mkpath(packageDir));
    QFile staleFile(QDir(packageDir).filePath(QStringLiteral("stale.txt")));
    QVERIFY(staleFile.open(QIODevice::WriteOnly));
    staleFile.close();

    struct ScopedCleanup {
        QString dir;
        ~ScopedCleanup() { if (QDir(dir).exists()) QDir(dir).removeRecursively(); }
    } cleanup{packageDir};

    bool stateCommitted = false;
    bool transactionDirectoryWasReachable = false;
    QCOMPARE(PackageDeployment::deployFiles(packageId, sourceDir.path(), packageType, [&] {
                 transactionDirectoryWasReachable = containsTransactionDirectory(deployDir, packageId);
                 stateCommitted = true;
                 return true;
             }),
             PackageDeployment::FileOperationResult::Success);
    QVERIFY(stateCommitted);
    QVERIFY(!transactionDirectoryWasReachable);
    QVERIFY(!QFileInfo::exists(QDir(packageDir).filePath(QStringLiteral("stale.txt"))));
    QFile installedPayload(QDir(packageDir).filePath(QStringLiteral("payload.txt")));
    QVERIFY(installedPayload.open(QIODevice::ReadOnly));
    QCOMPARE(installedPayload.readAll(), packageType.toUtf8());
}

void TestPackageDeploy::rejectsUnknownPackageType() {
    QVERIFY(PackageDeployment::deployDirectory(QStringLiteral("unknown")).isEmpty());

    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());
    QCOMPARE(PackageDeployment::deployFiles(QStringLiteral("org.aviqtl.invalid"), sourceDir.path(), QStringLiteral("unknown")),
             PackageDeployment::FileOperationResult::Failed);
}

void TestPackageDeploy::rejectsInvalidPackageIds() {
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());

    QCOMPARE(PackageDeployment::deployFiles(QStringLiteral("../escape"), sourceDir.path(), QStringLiteral("effect")), PackageDeployment::FileOperationResult::Failed);
    QCOMPARE(PackageDeployment::deployFiles(QStringLiteral("contains/slash"), sourceDir.path(), QStringLiteral("effect")), PackageDeployment::FileOperationResult::Failed);
    QCOMPARE(PackageDeployment::deployFiles(QStringLiteral("contains\\slash"), sourceDir.path(), QStringLiteral("effect")), PackageDeployment::FileOperationResult::Failed);
    QCOMPARE(PackageDeployment::deployFiles(QString(), sourceDir.path(), QStringLiteral("effect")), PackageDeployment::FileOperationResult::Failed);
}

void TestPackageDeploy::extractsSafeArchive() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString archivePath = dir.filePath(QStringLiteral("safe.zip"));
    QZipWriter writer(archivePath);
    writer.addFile(QStringLiteral("package/metadata.json"), QByteArrayLiteral("{}"));
    writer.close();
    QCOMPARE(writer.status(), QZipWriter::NoError);

    const QString destination = dir.filePath(QStringLiteral("safe-output"));
    QVERIFY(PackageDeployment::extractArchive(archivePath, destination));
    QVERIFY(QFile::exists(destination + QStringLiteral("/package/metadata.json")));
}

void TestPackageDeploy::rejectsTraversalArchive() {
    QVERIFY(!PackageDeployment::isSafeArchivePath(QStringLiteral("../escaped.txt")));
    QVERIFY(!PackageDeployment::isSafeArchivePath(QStringLiteral("folder/../../escaped.txt")));
    QVERIFY(!PackageDeployment::isSafeArchivePath(QStringLiteral("/absolute/path")));
    QVERIFY(!PackageDeployment::isSafeArchivePath(QStringLiteral("folder\\escaped.txt")));
    QVERIFY(PackageDeployment::isSafeArchivePath(QStringLiteral("package/metadata.json")));
}

void TestPackageDeploy::rejectsSymlinkArchive() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString archivePath = dir.filePath(QStringLiteral("symlink.zip"));
    QZipWriter writer(archivePath);
    writer.addSymLink(QStringLiteral("package/link"), QStringLiteral("../../outside"));
    writer.close();
    QCOMPARE(writer.status(), QZipWriter::NoError);

    QVERIFY(!PackageDeployment::extractArchive(archivePath, dir.filePath(QStringLiteral("symlink-output"))));
}

void TestPackageDeploy::rollsBackWhenStateCommitFails() {
    QTemporaryDir sourceDir;
    QVERIFY(sourceDir.isValid());
    QFile newFile(sourceDir.filePath(QStringLiteral("content.txt")));
    QVERIFY(newFile.open(QIODevice::WriteOnly));
    QCOMPARE(newFile.write("new"), 3);
    newFile.close();

    const QString packageId = QStringLiteral("org.aviqtl.test.rollback");
    const QString packageDir = QDir(PackageDeployment::deployDirectory(QStringLiteral("effect"))).filePath(packageId);
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

    QCOMPARE(PackageDeployment::deployFiles(packageId, sourceDir.path(), QStringLiteral("effect"), [] { return false; }),
             PackageDeployment::FileOperationResult::StateCommitFailed);
    QFile restored(packageDir + QStringLiteral("/content.txt"));
    QVERIFY(restored.open(QIODevice::ReadOnly));
    QCOMPARE(restored.readAll(), QByteArrayLiteral("old"));
}

void TestPackageDeploy::removesPackageTransactionally() {
    const QString packageId = QStringLiteral("org.aviqtl.test.remove_rollback");
    const QString packageDir = QDir(PackageDeployment::deployDirectory(QStringLiteral("effect"))).filePath(packageId);
    QDir(packageDir).removeRecursively();
    QVERIFY(QDir().mkpath(packageDir));
    QFile file(packageDir + QStringLiteral("/content.txt"));
    QVERIFY(file.open(QIODevice::WriteOnly));
    file.write("old");
    file.close();

    const QString deployDir = PackageDeployment::deployDirectory(QStringLiteral("effect"));
    bool transactionDirectoryWasReachable = false;
    QCOMPARE(PackageDeployment::removeFiles(packageId, QStringLiteral("effect"), [&] {
                 transactionDirectoryWasReachable = containsTransactionDirectory(deployDir, packageId);
                 return false;
             }),
             PackageDeployment::FileOperationResult::StateCommitFailed);
    QVERIFY(!transactionDirectoryWasReachable);
    QVERIFY(QFile::exists(packageDir + QStringLiteral("/content.txt")));
    QDir(packageDir).removeRecursively();
}

void TestPackageDeploy::rejectsRemovalWithUnknownType() {
    PackageManager &pm = PackageManager::instance();
    const QString installedPath = QCoreApplication::applicationDirPath() + QStringLiteral("/repos/installed.json");
    QDir().mkpath(QFileInfo(installedPath).absolutePath());
    QFile originalFile(installedPath);
    const bool hadOriginal = originalFile.exists();
    QByteArray originalData;
    if (hadOriginal) {
        QVERIFY(originalFile.open(QIODevice::ReadOnly));
        originalData = originalFile.readAll();
        originalFile.close();
    }
    struct RestoreFile {
        QString path;
        bool existed;
        QByteArray data;
        ~RestoreFile() {
            if (!existed) {
                QFile::remove(path);
                return;
            }
            QFile file(path);
            if (file.open(QIODevice::WriteOnly | QIODevice::Truncate))
                file.write(data);
        }
    } restore{installedPath, hadOriginal, originalData};

    const QString packageId = QStringLiteral("org.aviqtl.test.unknown");
    QJsonObject installed;
    installed.insert(packageId, QJsonObject{{QStringLiteral("type"), QStringLiteral("unknown")}});
    QFile installedFile(installedPath);
    QVERIFY(installedFile.open(QIODevice::WriteOnly | QIODevice::Truncate));
    installedFile.write(QJsonDocument(installed).toJson());
    installedFile.close();

    QSignalSpy errorSpy(&pm, &PackageManager::errorOccurred);
    QSignalSpy removedSpy(&pm, &PackageManager::packageRemoved);
    pm.removePackage(packageId);
    QCOMPARE(errorSpy.count(), 1);
    QCOMPARE(removedSpy.count(), 0);

    QFile unchanged(installedPath);
    QVERIFY(unchanged.open(QIODevice::ReadOnly));
    QVERIFY(QJsonDocument::fromJson(unchanged.readAll()).object().contains(packageId));
}

void TestPackageDeploy::resolvesRepositoryReferencesSafely() {
    using namespace AviQtl::Core::Internal;

    const QUrl repository(QStringLiteral("https://packages.example.com/repos/stable/repo.json?channel=beta"));
    QCOMPARE(resolveRepositoryReference(repository, QStringLiteral("catalog.json")),
             QUrl(QStringLiteral("https://packages.example.com/repos/stable/catalog.json")));
    QCOMPARE(resolveRepositoryReference(repository, QStringLiteral("../catalogs/main%20list.json?arch=arm64")).toString(QUrl::FullyEncoded),
             QStringLiteral("https://packages.example.com/repos/catalogs/main%20list.json?arch=arm64"));
    QCOMPARE(resolveRepositoryReference(repository, QStringLiteral("//cdn.example.com/catalog.json")),
             QUrl(QStringLiteral("https://cdn.example.com/catalog.json")));
    QCOMPARE(resolveRepositoryReference(repository, QStringLiteral("https://mirror.example.com/catalog.json")),
             QUrl(QStringLiteral("https://mirror.example.com/catalog.json")));

    QVERIFY(isSecureNetworkUrl(resolveRepositoryReference(repository, QStringLiteral("catalog.json"))));
    QVERIFY(!isSecureNetworkUrl(resolveRepositoryReference(repository, QStringLiteral("http://example.com/catalog.json"))));
    QVERIFY(!isSecureNetworkUrl(QUrl(QStringLiteral("https:///missing-host.json"))));
}

void TestPackageDeploy::managesRepositoriesAndPersistsThem() {
    SettingsManager &settings = SettingsManager::instance();
    PackageManager &manager = PackageManager::instance();
    QCoreApplication::processEvents();
    const SettingsSnapshot snapshot = captureSettings();
    const auto restore = qScopeGuard([&snapshot]() { restoreSettings(snapshot); });

    settings.setValue(QStringLiteral("packageRepositories"), QVariantList{});
    QSignalSpy changedSpy(&manager, &PackageManager::repositoriesChanged);
    QSignalSpy errorSpy(&manager, &PackageManager::errorOccurred);

    manager.addRepository(QStringLiteral("http://packages.example.com/repo.json"));
    QCOMPARE(errorSpy.count(), 1);
    QCOMPARE(changedSpy.count(), 0);
    QVERIFY(manager.repositories().isEmpty());

    const QString primaryUrl = QStringLiteral("https://packages.example.com/stable/repo.json");
    const QString secondaryUrl = QStringLiteral("https://mirror.example.com/nightly");
    manager.addRepository(primaryUrl, true, 20);
    manager.addRepository(secondaryUrl, false, 5);
    QCOMPARE(changedSpy.count(), 2);
    QCOMPARE(manager.repositories().size(), 2);

    manager.addRepository(primaryUrl, false, 1);
    QCOMPARE(changedSpy.count(), 2);

    manager.setRepositoryEnabled(secondaryUrl, true);
    manager.setRepositoryEnabled(secondaryUrl, true);
    manager.setRepositoryPriority(primaryUrl, 1);
    manager.setRepositoryPriority(primaryUrl, 1);
    QCOMPARE(changedSpy.count(), 4);

    const QVariantList expectedRepositories = manager.repositories();
    QCOMPARE(expectedRepositories.at(0).toMap().value(QStringLiteral("url")).toString(), primaryUrl);
    QCOMPARE(expectedRepositories.at(0).toMap().value(QStringLiteral("priority")).toInt(), 1);
    QVERIFY(expectedRepositories.at(1).toMap().value(QStringLiteral("enabled")).toBool());

    QFile persistedFile(snapshot.path);
    QVERIFY2(persistedFile.open(QIODevice::ReadOnly), qPrintable(persistedFile.errorString()));
    const QByteArray persistedPayload = persistedFile.readAll();
    persistedFile.close();
    const QVariantList persistedRepositories = QJsonDocument::fromJson(persistedPayload)
                                                   .object()
                                                   .value(QStringLiteral("packageRepositories"))
                                                   .toArray()
                                                   .toVariantList();
    QCOMPARE(persistedRepositories, QJsonArray::fromVariantList(expectedRepositories).toVariantList());

    settings.setValue(QStringLiteral("packageRepositories"), QVariantList{});
    QVERIFY(persistedFile.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(persistedFile.write(persistedPayload), persistedPayload.size());
    persistedFile.close();
    settings.load();
    QCOMPARE(manager.repositories(), expectedRepositories);

    manager.removeRepository(QStringLiteral("https://missing.example.com/repo.json"));
    QCOMPARE(changedSpy.count(), 4);
    manager.removeRepository(primaryUrl);
    QCOMPARE(changedSpy.count(), 5);
    QCOMPARE(manager.repositories().size(), 1);
    QCOMPARE(manager.repositories().first().toMap().value(QStringLiteral("url")).toString(), secondaryUrl);
}

void TestPackageDeploy::syncWithNoEnabledRepositoriesFinishesCleanly() {
    SettingsManager &settings = SettingsManager::instance();
    PackageManager &manager = PackageManager::instance();
    QCoreApplication::processEvents();
    const SettingsSnapshot snapshot = captureSettings();
    const auto restore = qScopeGuard([&snapshot]() { restoreSettings(snapshot); });

    settings.setValue(QStringLiteral("packageRepositories"), QVariantList{QVariantMap{
        {QStringLiteral("url"), QStringLiteral("https://packages.example.com/repo.json")},
        {QStringLiteral("name"), QStringLiteral("Disabled")},
        {QStringLiteral("enabled"), false},
        {QStringLiteral("priority"), 10},
    }});

    QSignalSpy busySpy(&manager, &PackageManager::isBusyChanged);
    QSignalSpy refreshedSpy(&manager, &PackageManager::repositoryRefreshed);
    manager.refreshRepositories();

    QCOMPARE(busySpy.count(), 2);
    QCOMPARE(refreshedSpy.count(), 1);
    QVERIFY(!manager.isBusy());
    QCOMPARE(manager.progress(), 1.0);
    QCOMPARE(manager.statusText(), QStringLiteral("Idle"));
    QVERIFY(manager.packageList().isEmpty());
    QVERIFY(!manager.hasUpdatesAvailable());
}

QTEST_MAIN(TestPackageDeploy)
#include "test_package_deploy.moc"
