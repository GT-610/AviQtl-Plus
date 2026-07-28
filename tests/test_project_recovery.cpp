#include "project_recovery_manager.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "workspace.hpp"
#include <QDir>
#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QScopeGuard>
#include <QSemaphore>
#include <QSignalSpy>
#include <QTimer>
#include <QTemporaryDir>
#include <QTest>
#include <QUuid>
#include <QUndoCommand>
#include <QUndoStack>
#include <QUrl>
#include <algorithm>
#include <atomic>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestProjectRecovery : public QObject {
    Q_OBJECT

  private slots:
    void initTestCase();
    void init();
    void cleanupTestCase();

    void dirtyProjectCreatesIndependentSnapshot();
    void projectSettingsAreRecoverableChanges();
    void snapshotDoesNotAffectFormalSaveState();
    void cleanAndDisabledProjectsDoNotCreateSnapshots();
    void untitledProjectsUseDistinctSnapshots();
    void formalSaveRemovesSnapshot();
    void recoveredProjectRemainsUnsaved();
    void corruptMetadataDoesNotHideValidRecoveries();
    void corruptSnapshotDoesNotPreventOtherRecovery();
    void closingProjectDiscardsSnapshot();
    void invalidIdentifiersAreRejected();
    void successiveWritesReplaceSnapshotGeneration();
    void legacySnapshotMetadataRemainsReadable();
    void failedMetadataOpenPreservesPreviousSnapshot();
    void staleRecoveriesAreRemoved();
    void staleCorruptMetadataIsRemoved();
    void staleOrphanedSnapshotsAreRemoved();
    void timerBackupCompletesAsynchronously();
    void timerBackupQueuesLatestSnapshot();
    void synchronousBackupWaitsForAsyncWrite();

  private:
    void markDirty(TimelineController &controller);
    QTemporaryDir m_directory;
    QVariant m_originalAutoBackup;
    QVariant m_originalBackupInterval;
};

void TestProjectRecovery::initTestCase() {
    QVERIFY(m_directory.isValid());
    ProjectRecoveryManager::setRecoveryRootForTests(m_directory.path());
    m_originalAutoBackup = SettingsManager::instance().value(QStringLiteral("enableAutoBackup"), true);
    m_originalBackupInterval = SettingsManager::instance().value(QStringLiteral("backupInterval"), 5);
}

void TestProjectRecovery::init() {
    ProjectRecoveryManager::setWriteBarrierForTests({});
    QDir root(m_directory.path());
    for (const QString &file : root.entryList(QDir::Files))
        QVERIFY(root.remove(file));
    SettingsManager::instance().setValue(QStringLiteral("enableAutoBackup"), true);
    SettingsManager::instance().setValue(QStringLiteral("backupInterval"), 5);
}

void TestProjectRecovery::cleanupTestCase() {
    ProjectRecoveryManager::setWriteBarrierForTests({});
    SettingsManager::instance().setValue(QStringLiteral("enableAutoBackup"), m_originalAutoBackup);
    SettingsManager::instance().setValue(QStringLiteral("backupInterval"), m_originalBackupInterval);
    ProjectRecoveryManager::setRecoveryRootForTests(QString());
}

void TestProjectRecovery::markDirty(TimelineController &controller) { controller.timeline()->undoStack()->push(new QUndoCommand(QStringLiteral("test edit"))); }

void TestProjectRecovery::dirtyProjectCreatesIndependentSnapshot() {
    TimelineController controller;
    controller.setProperty("untitledName", QStringLiteral("Draft"));
    markDirty(controller);

    QVERIFY(controller.writeRecoveryNow());
    QVERIFY(controller.hasUnsavedChanges());
    QVERIFY(controller.currentProjectUrl().isEmpty());

    const auto entries = ProjectRecoveryManager::entries();
    QCOMPARE(entries.size(), 1);
    QVERIFY(entries.first().valid);
    QCOMPARE(entries.first().displayName, QStringLiteral("Draft"));
    QVERIFY(QFileInfo::exists(entries.first().snapshotPath));
}

void TestProjectRecovery::projectSettingsAreRecoverableChanges() {
    TimelineController controller;
    QVERIFY(!controller.hasUnsavedChanges());
    controller.project()->setFps(24.0);
    QVERIFY(controller.hasUnsavedChanges());
    QVERIFY(controller.writeRecoveryNow());
    QCOMPARE(ProjectRecoveryManager::entries().size(), 1);
}

void TestProjectRecovery::snapshotDoesNotAffectFormalSaveState() {
    TimelineController controller;
    const QString projectUrl = QUrl::fromLocalFile(m_directory.filePath(QStringLiteral("formal.aviqtl"))).toString();
    markDirty(controller);
    QVERIFY(controller.saveProject(projectUrl));
    QVERIFY(!controller.hasUnsavedChanges());

    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());
    QCOMPARE(controller.currentProjectUrl(), projectUrl);
    QVERIFY(controller.hasUnsavedChanges());
}

void TestProjectRecovery::cleanAndDisabledProjectsDoNotCreateSnapshots() {
    TimelineController clean;
    QVERIFY(!clean.writeRecoveryNow());
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());

    TimelineController disabled;
    markDirty(disabled);
    SettingsManager::instance().setValue(QStringLiteral("enableAutoBackup"), false);
    QVERIFY(!disabled.writeRecoveryNow());
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::untitledProjectsUseDistinctSnapshots() {
    TimelineController first;
    TimelineController second;
    markDirty(first);
    markDirty(second);
    QVERIFY(first.writeRecoveryNow());
    QVERIFY(second.writeRecoveryNow());
    QVERIFY(first.recoveryId() != second.recoveryId());
    QCOMPARE(ProjectRecoveryManager::entries().size(), 2);
}

void TestProjectRecovery::formalSaveRemovesSnapshot() {
    TimelineController controller;
    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());

    const QString projectPath = m_directory.filePath(QStringLiteral("saved-project.aviqtl"));
    QVERIFY(controller.saveProject(QUrl::fromLocalFile(projectPath).toString()));
    QVERIFY(!controller.hasUnsavedChanges());
    QCOMPARE(controller.currentProjectUrl(), QUrl::fromLocalFile(projectPath).toString());
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::recoveredProjectRemainsUnsaved() {
    TimelineController source;
    source.project()->setWidth(1234);
    markDirty(source);
    const QString originalUrl = QUrl::fromLocalFile(m_directory.filePath(QStringLiteral("original.aviqtl"))).toString();
    QString error;
    QVERIFY(ProjectRecoveryManager::write(source.recoveryId(), originalUrl, QStringLiteral("Original"), source.timeline(), source.project(), &error));
    const ProjectRecoveryEntry entry = ProjectRecoveryManager::entries().first();

    TimelineController recovered;
    QVERIFY(recovered.loadRecovery(entry.snapshotPath, entry.id, entry.originalProjectUrl));
    QCOMPARE(recovered.project()->width(), 1234);
    QVERIFY(recovered.currentProjectUrl().isEmpty());
    QCOMPARE(recovered.recoveryOriginalProjectUrl(), originalUrl);
    QVERIFY(recovered.hasUnsavedChanges());
    QCOMPARE(ProjectRecoveryManager::entries().size(), 1);
}

void TestProjectRecovery::corruptMetadataDoesNotHideValidRecoveries() {
    TimelineController controller;
    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());

    QFile corrupt(m_directory.filePath(QStringLiteral("corrupt.json")));
    QVERIFY(corrupt.open(QIODevice::WriteOnly));
    QCOMPARE(corrupt.write("{not json"), qint64(9));
    corrupt.close();

    const auto entries = ProjectRecoveryManager::entries();
    QCOMPARE(entries.size(), 2);
    QCOMPARE(std::count_if(entries.cbegin(), entries.cend(), [](const ProjectRecoveryEntry &entry) { return entry.valid; }), 1);
    QCOMPARE(std::count_if(entries.cbegin(), entries.cend(), [](const ProjectRecoveryEntry &entry) { return !entry.valid && !entry.error.isEmpty(); }), 1);
}

void TestProjectRecovery::corruptSnapshotDoesNotPreventOtherRecovery() {
    TimelineController corruptSource;
    markDirty(corruptSource);
    QVERIFY(corruptSource.writeRecoveryNow());
    const QString corruptId = corruptSource.recoveryId();
    const ProjectRecoveryEntry corruptEntry = ProjectRecoveryManager::entries().first();
    QFile corruptSnapshot(corruptEntry.snapshotPath);
    QVERIFY(corruptSnapshot.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QVERIFY(corruptSnapshot.write("not a project") > 0);
    corruptSnapshot.close();

    TimelineController validSource;
    validSource.project()->setHeight(777);
    markDirty(validSource);
    QVERIFY(validSource.writeRecoveryNow());

    Workspace workspace;
    QVERIFY(!workspace.recoverProject(corruptId));
    QVERIFY(workspace.tabs().isEmpty());
    QVERIFY(workspace.recoverProject(validSource.recoveryId()));
    QCOMPARE(workspace.tabs().size(), 1);
    QCOMPARE(workspace.currentTimeline()->project()->height(), 777);
    QVERIFY(workspace.currentTimeline()->hasUnsavedChanges());
}

void TestProjectRecovery::closingProjectDiscardsSnapshot() {
    Workspace workspace;
    workspace.newProject();
    TimelineController *controller = workspace.currentTimeline();
    QVERIFY(controller != nullptr);
    markDirty(*controller);
    QVERIFY(controller->writeRecoveryNow());
    QCOMPARE(ProjectRecoveryManager::entries().size(), 1);

    workspace.closeProject(0);
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::invalidIdentifiersAreRejected() {
    TimelineController controller;
    QString error;
    QVERIFY(!ProjectRecoveryManager::write(QStringLiteral("../outside"), QString(), QStringLiteral("Invalid"), controller.timeline(), controller.project(), &error));
    QVERIFY(!error.isEmpty());
    QVERIFY(!ProjectRecoveryManager::remove(QStringLiteral("../outside")));
    QVERIFY(!QFileInfo::exists(QDir(m_directory.path()).filePath(QStringLiteral("../outside.json"))));
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::successiveWritesReplaceSnapshotGeneration() {
    TimelineController controller;
    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());
    const QString firstSnapshot = ProjectRecoveryManager::entries().first().snapshotPath;
    QVERIFY(QFileInfo::exists(firstSnapshot));

    controller.project()->setWidth(1440);
    QVERIFY(controller.writeRecoveryNow());
    const ProjectRecoveryEntry secondEntry = ProjectRecoveryManager::entries().first();
    QVERIFY(secondEntry.valid);
    QVERIFY(secondEntry.snapshotPath != firstSnapshot);
    QVERIFY(QFileInfo::exists(secondEntry.snapshotPath));
    QVERIFY(!QFileInfo::exists(firstSnapshot));
    QCOMPARE(QDir(m_directory.path()).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);
}

void TestProjectRecovery::legacySnapshotMetadataRemainsReadable() {
    TimelineController controller;
    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());
    const ProjectRecoveryEntry generatedEntry = ProjectRecoveryManager::entries().first();
    const QString legacyPath = m_directory.filePath(generatedEntry.id + QStringLiteral(".aviqtl"));
    QVERIFY(QFile::rename(generatedEntry.snapshotPath, legacyPath));

    QFile metadata(m_directory.filePath(generatedEntry.id + QStringLiteral(".json")));
    QVERIFY(metadata.open(QIODevice::ReadOnly));
    QJsonObject object = QJsonDocument::fromJson(metadata.readAll()).object();
    metadata.close();
    object.remove(QStringLiteral("snapshotFile"));
    const QByteArray metadataDocument = QJsonDocument(object).toJson(QJsonDocument::Compact);
    QVERIFY(metadata.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(metadata.write(metadataDocument), metadataDocument.size());
    metadata.close();

    const ProjectRecoveryEntry legacyEntry = ProjectRecoveryManager::entries().first();
    QVERIFY(legacyEntry.valid);
    QCOMPARE(legacyEntry.snapshotPath, legacyPath);
}

void TestProjectRecovery::failedMetadataOpenPreservesPreviousSnapshot() {
    TimelineController controller;
    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());
    const ProjectRecoveryEntry entry = ProjectRecoveryManager::entries().first();
    const QString metadataPath = m_directory.filePath(entry.id + QStringLiteral(".json"));
    QVERIFY(QFile::remove(metadataPath));
    QVERIFY(QDir().mkpath(metadataPath));

    QString error;
    QVERIFY(!ProjectRecoveryManager::write(entry.id, QString(), QStringLiteral("Retry"), controller.timeline(), controller.project(), &error));
    QVERIFY(!error.isEmpty());
    QVERIFY(QFileInfo::exists(entry.snapshotPath));
    QCOMPARE(QDir(m_directory.path()).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);
    QVERIFY(QDir(metadataPath).removeRecursively());
}

void TestProjectRecovery::staleRecoveriesAreRemoved() {
    TimelineController controller;
    markDirty(controller);
    QVERIFY(controller.writeRecoveryNow());
    const ProjectRecoveryEntry entry = ProjectRecoveryManager::entries().first();

    QFile metadata(m_directory.filePath(entry.id + QStringLiteral(".json")));
    QVERIFY(metadata.open(QIODevice::ReadOnly));
    QJsonObject object = QJsonDocument::fromJson(metadata.readAll()).object();
    metadata.close();
    object.insert(QStringLiteral("savedAt"), QDateTime::currentDateTimeUtc().addDays(-31).toString(Qt::ISODateWithMs));
    QVERIFY(metadata.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QVERIFY(metadata.write(QJsonDocument(object).toJson(QJsonDocument::Compact)) > 0);
    metadata.close();

    ProjectRecoveryManager::cleanupStale(30);
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
    QVERIFY(!QFileInfo::exists(entry.snapshotPath));
}

void TestProjectRecovery::staleCorruptMetadataIsRemoved() {
    const QString recoveryId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const QString metadataPath = m_directory.filePath(recoveryId + QStringLiteral(".json"));
    const QString snapshotPath = m_directory.filePath(recoveryId + QStringLiteral(".aviqtl"));

    QFile metadata(metadataPath);
    QVERIFY(metadata.open(QIODevice::WriteOnly));
    QVERIFY(metadata.write("{not json") > 0);
    metadata.close();
    QVERIFY(metadata.open(QIODevice::ReadWrite));
    QVERIFY(metadata.setFileTime(QDateTime::currentDateTimeUtc().addDays(-31), QFileDevice::FileModificationTime));
    metadata.close();

    QFile snapshot(snapshotPath);
    QVERIFY(snapshot.open(QIODevice::WriteOnly));
    QVERIFY(snapshot.write("stale snapshot") > 0);
    snapshot.close();

    ProjectRecoveryManager::cleanupStale(30);
    QVERIFY(!QFileInfo::exists(metadataPath));
    QVERIFY(!QFileInfo::exists(snapshotPath));
}

void TestProjectRecovery::staleOrphanedSnapshotsAreRemoved() {
    const QString recoveryId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const QString generationId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const QString orphanPath = m_directory.filePath(recoveryId + QLatin1Char('-') + generationId + QStringLiteral(".aviqtl"));
    QFile orphan(orphanPath);
    QVERIFY(orphan.open(QIODevice::WriteOnly));
    QVERIFY(orphan.write("orphan") > 0);
    orphan.close();
    QVERIFY(orphan.open(QIODevice::ReadWrite));
    QVERIFY(orphan.setFileTime(QDateTime::currentDateTimeUtc().addDays(-31), QFileDevice::FileModificationTime));
    orphan.close();

    ProjectRecoveryManager::cleanupStale(30);
    QVERIFY(!QFileInfo::exists(orphanPath));
}

void TestProjectRecovery::timerBackupCompletesAsynchronously() {
    TimelineController controller;
    markDirty(controller);
    QTimer *timer = controller.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    QVERIFY(timer != nullptr);

    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    QTRY_COMPARE_WITH_TIMEOUT(ProjectRecoveryManager::entries().size(), 1, 5000);
    QVERIFY(ProjectRecoveryManager::entries().first().valid);
}

void TestProjectRecovery::timerBackupQueuesLatestSnapshot() {
    TimelineController controller;
    QSemaphore asyncWriteStarted;
    QSemaphore releaseAsyncWrite;
    std::atomic_bool blockFirstAsyncWrite = true;
    ProjectRecoveryManager::setWriteBarrierForTests([&](ProjectRecoveryWriteBarrierPoint point) {
        if (point == ProjectRecoveryWriteBarrierPoint::AsyncWriteStarted && blockFirstAsyncWrite.exchange(false)) {
            asyncWriteStarted.release();
            releaseAsyncWrite.acquire();
        }
    });
    const auto resetBarrier = qScopeGuard([&]() {
        releaseAsyncWrite.release();
        ProjectRecoveryManager::setWriteBarrierForTests({});
    });
    markDirty(controller);
    QTimer *timer = controller.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    auto *watcher = controller.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(timer != nullptr);
    QVERIFY(watcher != nullptr);
    QSignalSpy finishedSpy(watcher, &QFutureWatcherBase::finished);

    controller.project()->setWidth(1111);
    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    QVERIFY(asyncWriteStarted.tryAcquire(1, 5000));
    QVERIFY(watcher->isRunning());
    controller.project()->setWidth(2222);
    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    releaseAsyncWrite.release();

    QTRY_COMPARE_WITH_TIMEOUT(finishedSpy.count(), 2, 5000);
    const QList<ProjectRecoveryEntry> entries = ProjectRecoveryManager::entries();
    QCOMPARE(entries.size(), 1);
    QCOMPARE(QDir(m_directory.path()).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);

    TimelineController recovered;
    QVERIFY(recovered.loadRecovery(entries.first().snapshotPath, entries.first().id, entries.first().originalProjectUrl));
    QCOMPARE(recovered.project()->width(), 2222);
}

void TestProjectRecovery::synchronousBackupWaitsForAsyncWrite() {
    TimelineController controller;
    QSemaphore asyncWriteStarted;
    QSemaphore releaseAsyncWrite;
    ProjectRecoveryManager::setWriteBarrierForTests([&](ProjectRecoveryWriteBarrierPoint point) {
        if (point == ProjectRecoveryWriteBarrierPoint::AsyncWriteStarted) {
            asyncWriteStarted.release();
            releaseAsyncWrite.acquire();
        } else {
            releaseAsyncWrite.release();
        }
    });
    const auto resetBarrier = qScopeGuard([&]() {
        releaseAsyncWrite.release();
        ProjectRecoveryManager::setWriteBarrierForTests({});
    });
    markDirty(controller);
    QTimer *timer = controller.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    auto *watcher = controller.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(timer != nullptr);
    QVERIFY(watcher != nullptr);

    controller.project()->setWidth(1111);
    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    QVERIFY(asyncWriteStarted.tryAcquire(1, 5000));
    QVERIFY(watcher->isRunning());
    controller.project()->setWidth(3333);
    QVERIFY(controller.writeRecoveryNow());

    const QList<ProjectRecoveryEntry> entries = ProjectRecoveryManager::entries();
    QCOMPARE(entries.size(), 1);
    QCOMPARE(QDir(m_directory.path()).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);

    TimelineController recovered;
    QVERIFY(recovered.loadRecovery(entries.first().snapshotPath, entries.first().id, entries.first().originalProjectUrl));
    QCOMPARE(recovered.project()->width(), 3333);
}

QTEST_MAIN(TestProjectRecovery)
#include "test_project_recovery.moc"
