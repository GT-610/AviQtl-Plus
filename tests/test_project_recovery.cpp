#include "project_recovery_manager.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "workspace.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QDateTime>
#include <QFile>
#include <QFileInfo>
#include <QFutureWatcher>
#include <QJsonDocument>
#include <QJsonObject>
#include <QScopeGuard>
#include <QSemaphore>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QThreadPool>
#include <QTimer>
#include <QTemporaryDir>
#include <QTest>
#include <QUuid>
#include <QUndoCommand>
#include <QUndoStack>
#include <QUrl>
#include <QtConcurrent>
#include <algorithm>

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
    void queuedWriteUsesLatestGeneration();
    void removalInvalidatesQueuedWrite();

  private:
    void markDirty(TimelineController &controller);
    void writeRecovery(TimelineController &controller);
    ProjectRecoveryEntry recoveryEntryFor(const TimelineController &controller) const;
    QTemporaryDir m_directory;
    QString m_recoveryRoot;
    QVariant m_originalAutoBackup;
    QVariant m_originalBackupInterval;
};

void TestProjectRecovery::initTestCase() {
    QVERIFY(m_directory.isValid());
    QStandardPaths::setTestModeEnabled(true);
    QCoreApplication::setOrganizationName(QStringLiteral("AviQtl"));
    QCoreApplication::setApplicationName(QStringLiteral("AviQtl_Test_project_recovery_%1").arg(QUuid::createUuid().toString(QUuid::WithoutBraces)));
    m_recoveryRoot = ProjectRecoveryManager::recoveryRoot();
    QVERIFY(!m_recoveryRoot.isEmpty());
    m_originalAutoBackup = SettingsManager::instance().value(QStringLiteral("enableAutoBackup"), true);
    m_originalBackupInterval = SettingsManager::instance().value(QStringLiteral("backupInterval"), 5);
}

void TestProjectRecovery::init() {
    QDir recoveryRoot(m_recoveryRoot);
    if (recoveryRoot.exists())
        QVERIFY(recoveryRoot.removeRecursively());
    QVERIFY(QDir().mkpath(m_recoveryRoot));
    QDir projectRoot(m_directory.path());
    for (const QString &file : projectRoot.entryList(QDir::Files))
        QVERIFY(projectRoot.remove(file));
    SettingsManager::instance().setValue(QStringLiteral("enableAutoBackup"), true);
    SettingsManager::instance().setValue(QStringLiteral("backupInterval"), 5);
}

void TestProjectRecovery::cleanupTestCase() {
    SettingsManager::instance().setValue(QStringLiteral("enableAutoBackup"), m_originalAutoBackup);
    SettingsManager::instance().setValue(QStringLiteral("backupInterval"), m_originalBackupInterval);
    QDir recoveryRoot(m_recoveryRoot);
    if (recoveryRoot.exists())
        QVERIFY(recoveryRoot.removeRecursively());
}

void TestProjectRecovery::markDirty(TimelineController &controller) { controller.timeline()->undoStack()->push(new QUndoCommand(QStringLiteral("test edit"))); }

void TestProjectRecovery::writeRecovery(TimelineController &controller) {
    QTimer *timer = controller.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    auto *watcher = controller.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(timer != nullptr);
    QVERIFY(watcher != nullptr);
    QSignalSpy finishedSpy(watcher, &QFutureWatcherBase::finished);

    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    QTRY_COMPARE_WITH_TIMEOUT(finishedSpy.count(), 1, 5000);
    QVERIFY(!watcher->isRunning());
}

ProjectRecoveryEntry TestProjectRecovery::recoveryEntryFor(const TimelineController &controller) const {
    const auto entries = ProjectRecoveryManager::entries();
    const auto it = std::find_if(entries.cbegin(), entries.cend(), [&controller](const ProjectRecoveryEntry &entry) { return entry.id == controller.recoveryId(); });
    return it == entries.cend() ? ProjectRecoveryEntry{} : *it;
}

void TestProjectRecovery::dirtyProjectCreatesIndependentSnapshot() {
    TimelineController controller;
    controller.setProperty("untitledName", QStringLiteral("Draft"));
    markDirty(controller);

    writeRecovery(controller);
    QVERIFY(controller.hasUnsavedChanges());
    QVERIFY(controller.currentProjectUrl().isEmpty());

    const ProjectRecoveryEntry entry = recoveryEntryFor(controller);
    QVERIFY(entry.valid);
    QCOMPARE(entry.displayName, QStringLiteral("Draft"));
    QVERIFY(QFileInfo::exists(entry.snapshotPath));
}

void TestProjectRecovery::projectSettingsAreRecoverableChanges() {
    TimelineController controller;
    QVERIFY(!controller.hasUnsavedChanges());
    controller.project()->setFps(24.0);
    QVERIFY(controller.hasUnsavedChanges());
    writeRecovery(controller);
    QCOMPARE(ProjectRecoveryManager::entries().size(), 1);
}

void TestProjectRecovery::snapshotDoesNotAffectFormalSaveState() {
    TimelineController controller;
    const QString projectUrl = QUrl::fromLocalFile(m_directory.filePath(QStringLiteral("formal.aviqtl"))).toString();
    markDirty(controller);
    QVERIFY(controller.saveProject(projectUrl));
    QVERIFY(!controller.hasUnsavedChanges());

    markDirty(controller);
    writeRecovery(controller);
    QCOMPARE(controller.currentProjectUrl(), projectUrl);
    QVERIFY(controller.hasUnsavedChanges());
}

void TestProjectRecovery::cleanAndDisabledProjectsDoNotCreateSnapshots() {
    TimelineController clean;
    QTimer *cleanTimer = clean.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    auto *cleanWatcher = clean.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(cleanTimer != nullptr);
    QVERIFY(cleanWatcher != nullptr);
    QVERIFY(QMetaObject::invokeMethod(cleanTimer, "timeout", Qt::DirectConnection));
    QVERIFY(!cleanWatcher->isRunning());
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());

    TimelineController disabled;
    markDirty(disabled);
    SettingsManager::instance().setValue(QStringLiteral("enableAutoBackup"), false);
    QTimer *disabledTimer = disabled.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    auto *disabledWatcher = disabled.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(disabledTimer != nullptr);
    QVERIFY(disabledWatcher != nullptr);
    QVERIFY(QMetaObject::invokeMethod(disabledTimer, "timeout", Qt::DirectConnection));
    QVERIFY(!disabledWatcher->isRunning());
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::untitledProjectsUseDistinctSnapshots() {
    TimelineController first;
    TimelineController second;
    markDirty(first);
    markDirty(second);
    writeRecovery(first);
    writeRecovery(second);
    QVERIFY(first.recoveryId() != second.recoveryId());
    QCOMPARE(ProjectRecoveryManager::entries().size(), 2);
}

void TestProjectRecovery::formalSaveRemovesSnapshot() {
    TimelineController controller;
    markDirty(controller);
    writeRecovery(controller);

    const QString projectPath = m_directory.filePath(QStringLiteral("saved-project.aviqtl"));
    QVERIFY(controller.saveProject(QUrl::fromLocalFile(projectPath).toString()));
    QVERIFY(!controller.hasUnsavedChanges());
    QCOMPARE(controller.currentProjectUrl(), QUrl::fromLocalFile(projectPath).toString());
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::recoveredProjectRemainsUnsaved() {
    TimelineController source;
    const QString originalPath = m_directory.filePath(QStringLiteral("original.aviqtl"));
    const QString originalUrl = QUrl::fromLocalFile(originalPath).toString();
    markDirty(source);
    QVERIFY(source.saveProject(originalUrl));
    source.project()->setWidth(1234);
    QVERIFY(source.hasUnsavedChanges());
    writeRecovery(source);
    const ProjectRecoveryEntry entry = recoveryEntryFor(source);
    QVERIFY(entry.valid);

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
    writeRecovery(controller);

    QFile corrupt(QDir(m_recoveryRoot).filePath(QStringLiteral("corrupt.json")));
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
    writeRecovery(corruptSource);
    const QString corruptId = corruptSource.recoveryId();
    const ProjectRecoveryEntry corruptEntry = recoveryEntryFor(corruptSource);
    QVERIFY(corruptEntry.valid);
    QFile corruptSnapshot(corruptEntry.snapshotPath);
    QVERIFY(corruptSnapshot.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QVERIFY(corruptSnapshot.write("not a project") > 0);
    corruptSnapshot.close();

    TimelineController validSource;
    validSource.project()->setHeight(777);
    markDirty(validSource);
    writeRecovery(validSource);

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
    writeRecovery(*controller);
    QCOMPARE(ProjectRecoveryManager::entries().size(), 1);

    workspace.closeProject(0);
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::invalidIdentifiersAreRejected() {
    TimelineController controller;
    QFuture<ProjectRecoveryWriteResult> future = ProjectRecoveryManager::writeAsync(QStringLiteral("../outside"), QString(), QStringLiteral("Invalid"), controller.timeline(), controller.project());
    future.waitForFinished();
    const ProjectRecoveryWriteResult result = future.result();
    QVERIFY(!result.success);
    QVERIFY(!result.error.isEmpty());
    QVERIFY(!ProjectRecoveryManager::remove(QStringLiteral("../outside")));
    QVERIFY(!QFileInfo::exists(QDir(m_recoveryRoot).filePath(QStringLiteral("../outside.json"))));
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
}

void TestProjectRecovery::successiveWritesReplaceSnapshotGeneration() {
    TimelineController controller;
    markDirty(controller);
    writeRecovery(controller);
    const QString firstSnapshot = recoveryEntryFor(controller).snapshotPath;
    QVERIFY(QFileInfo::exists(firstSnapshot));

    controller.project()->setWidth(1440);
    writeRecovery(controller);
    const ProjectRecoveryEntry secondEntry = recoveryEntryFor(controller);
    QVERIFY(secondEntry.valid);
    QVERIFY(secondEntry.snapshotPath != firstSnapshot);
    QVERIFY(QFileInfo::exists(secondEntry.snapshotPath));
    QVERIFY(!QFileInfo::exists(firstSnapshot));
    QCOMPARE(QDir(m_recoveryRoot).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);
}

void TestProjectRecovery::legacySnapshotMetadataRemainsReadable() {
    TimelineController controller;
    markDirty(controller);
    writeRecovery(controller);
    const ProjectRecoveryEntry generatedEntry = recoveryEntryFor(controller);
    const QString legacyPath = QDir(m_recoveryRoot).filePath(generatedEntry.id + QStringLiteral(".aviqtl"));
    QVERIFY(QFile::rename(generatedEntry.snapshotPath, legacyPath));

    QFile metadata(QDir(m_recoveryRoot).filePath(generatedEntry.id + QStringLiteral(".json")));
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
    writeRecovery(controller);
    const ProjectRecoveryEntry entry = recoveryEntryFor(controller);
    const QString metadataPath = QDir(m_recoveryRoot).filePath(entry.id + QStringLiteral(".json"));
    QVERIFY(QFile::remove(metadataPath));
    QVERIFY(QDir().mkpath(metadataPath));

    QFuture<ProjectRecoveryWriteResult> future = ProjectRecoveryManager::writeAsync(entry.id, QString(), QStringLiteral("Retry"), controller.timeline(), controller.project());
    future.waitForFinished();
    const ProjectRecoveryWriteResult result = future.result();
    QVERIFY(!result.success);
    QVERIFY(!result.error.isEmpty());
    QVERIFY(QFileInfo::exists(entry.snapshotPath));
    QCOMPARE(QDir(m_recoveryRoot).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);
    QVERIFY(QDir(metadataPath).removeRecursively());
}

void TestProjectRecovery::staleRecoveriesAreRemoved() {
    TimelineController controller;
    markDirty(controller);
    writeRecovery(controller);
    const ProjectRecoveryEntry entry = recoveryEntryFor(controller);

    QFile metadata(QDir(m_recoveryRoot).filePath(entry.id + QStringLiteral(".json")));
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
    const QString metadataPath = QDir(m_recoveryRoot).filePath(recoveryId + QStringLiteral(".json"));
    const QString snapshotPath = QDir(m_recoveryRoot).filePath(recoveryId + QStringLiteral(".aviqtl"));

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
    const QString orphanPath = QDir(m_recoveryRoot).filePath(recoveryId + QLatin1Char('-') + generationId + QStringLiteral(".aviqtl"));
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
    auto *watcher = controller.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(timer != nullptr);
    QVERIFY(watcher != nullptr);
    QSignalSpy finishedSpy(watcher, &QFutureWatcherBase::finished);

    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    QVERIFY(watcher->isRunning());
    QTRY_COMPARE_WITH_TIMEOUT(finishedSpy.count(), 1, 5000);
    QVERIFY(recoveryEntryFor(controller).valid);
}

void TestProjectRecovery::timerBackupQueuesLatestSnapshot() {
    QThreadPool *threadPool = QThreadPool::globalInstance();
    const int originalMaxThreadCount = threadPool->maxThreadCount();
    threadPool->setMaxThreadCount(1);

    QSemaphore blockerStarted;
    QSemaphore releaseBlocker;
    QFuture<void> blockerFuture = QtConcurrent::run(threadPool, [&]() {
        blockerStarted.release();
        releaseBlocker.acquire();
    });
    const auto restoreThreadPool = qScopeGuard([&]() {
        releaseBlocker.release();
        blockerFuture.waitForFinished();
        threadPool->setMaxThreadCount(originalMaxThreadCount);
    });
    QVERIFY(blockerStarted.tryAcquire(1, 5000));

    TimelineController controller;
    markDirty(controller);
    QTimer *timer = controller.findChild<QTimer *>(QStringLiteral("projectRecoveryTimer"));
    auto *watcher = controller.findChild<QFutureWatcherBase *>(QStringLiteral("projectRecoveryWriteWatcher"));
    QVERIFY(timer != nullptr);
    QVERIFY(watcher != nullptr);
    QSignalSpy finishedSpy(watcher, &QFutureWatcherBase::finished);

    controller.project()->setWidth(1111);
    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    QVERIFY(watcher->isRunning());
    controller.project()->setWidth(2222);
    QVERIFY(QMetaObject::invokeMethod(timer, "timeout", Qt::DirectConnection));
    releaseBlocker.release();

    QTRY_COMPARE_WITH_TIMEOUT(finishedSpy.count(), 2, 5000);
    const ProjectRecoveryEntry entry = recoveryEntryFor(controller);
    QVERIFY(entry.valid);
    QCOMPARE(ProjectRecoveryManager::entries().size(), 1);
    QCOMPARE(QDir(m_recoveryRoot).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);

    TimelineController recovered;
    QVERIFY(recovered.loadRecovery(entry.snapshotPath, entry.id, entry.originalProjectUrl));
    QCOMPARE(recovered.project()->width(), 2222);
}

void TestProjectRecovery::queuedWriteUsesLatestGeneration() {
    QThreadPool *threadPool = QThreadPool::globalInstance();
    const int originalMaxThreadCount = threadPool->maxThreadCount();
    threadPool->setMaxThreadCount(1);

    QSemaphore blockerStarted;
    QSemaphore releaseBlocker;
    QFuture<void> blockerFuture = QtConcurrent::run(threadPool, [&]() {
        blockerStarted.release();
        releaseBlocker.acquire();
    });
    const auto restoreThreadPool = qScopeGuard([&]() {
        releaseBlocker.release();
        blockerFuture.waitForFinished();
        threadPool->setMaxThreadCount(originalMaxThreadCount);
    });
    QVERIFY(blockerStarted.tryAcquire(1, 5000));

    TimelineController controller;
    markDirty(controller);
    controller.project()->setWidth(1111);
    QFuture<ProjectRecoveryWriteResult> first = ProjectRecoveryManager::writeAsync(
        controller.recoveryId(), QString(), QStringLiteral("First"), controller.timeline(), controller.project());
    controller.project()->setWidth(2222);
    QFuture<ProjectRecoveryWriteResult> second = ProjectRecoveryManager::writeAsync(
        controller.recoveryId(), QString(), QStringLiteral("Second"), controller.timeline(), controller.project());

    releaseBlocker.release();
    first.waitForFinished();
    second.waitForFinished();
    QVERIFY(first.result().success);
    QVERIFY(second.result().success);

    const ProjectRecoveryEntry entry = recoveryEntryFor(controller);
    QVERIFY(entry.valid);
    QCOMPARE(entry.displayName, QStringLiteral("Second"));
    QCOMPARE(QDir(m_recoveryRoot).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).size(), 1);

    TimelineController recovered;
    QVERIFY(recovered.loadRecovery(entry.snapshotPath, entry.id, entry.originalProjectUrl));
    QCOMPARE(recovered.project()->width(), 2222);
}

void TestProjectRecovery::removalInvalidatesQueuedWrite() {
    QThreadPool *threadPool = QThreadPool::globalInstance();
    const int originalMaxThreadCount = threadPool->maxThreadCount();
    threadPool->setMaxThreadCount(1);

    QSemaphore blockerStarted;
    QSemaphore releaseBlocker;
    QFuture<void> blockerFuture = QtConcurrent::run(threadPool, [&]() {
        blockerStarted.release();
        releaseBlocker.acquire();
    });
    const auto restoreThreadPool = qScopeGuard([&]() {
        releaseBlocker.release();
        blockerFuture.waitForFinished();
        threadPool->setMaxThreadCount(originalMaxThreadCount);
    });
    QVERIFY(blockerStarted.tryAcquire(1, 5000));

    TimelineController controller;
    markDirty(controller);
    QFuture<ProjectRecoveryWriteResult> pending = ProjectRecoveryManager::writeAsync(
        controller.recoveryId(), QString(), QStringLiteral("Discarded"), controller.timeline(), controller.project());
    QVERIFY(ProjectRecoveryManager::remove(controller.recoveryId()));

    releaseBlocker.release();
    pending.waitForFinished();
    QVERIFY(pending.result().success);
    QVERIFY(ProjectRecoveryManager::entries().isEmpty());
    QVERIFY(QDir(m_recoveryRoot).entryList({QStringLiteral("*.aviqtl")}, QDir::Files).isEmpty());
}

QTEST_MAIN(TestProjectRecovery)
#include "test_project_recovery.moc"
