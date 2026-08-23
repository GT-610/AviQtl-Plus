#include "project_recovery_manager.hpp"
#include "project_serializer.hpp"
#include "rust_core_policy.hpp"
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QHash>
#include <QMutex>
#include <QMutexLocker>
#include <QSaveFile>
#include <QSet>
#include <QStandardPaths>
#include <QUuid>
#include <QtConcurrent>

namespace AviQtl::UI {
namespace {
QString metadataPath(const QString &id) { return QDir(ProjectRecoveryManager::recoveryRoot()).filePath(id + QStringLiteral(".json")); }
QString legacySnapshotFileName(const QString &id) { return id + QStringLiteral(".aviqtl"); }
QString generatedSnapshotFileName(const QString &id) { return id + QLatin1Char('-') + QUuid::createUuid().toString(QUuid::WithoutBraces) + QStringLiteral(".aviqtl"); }
QString snapshotPath(const QString &fileName) { return QDir(ProjectRecoveryManager::recoveryRoot()).filePath(fileName); }

QMutex &recoveryMutationMutex() {
    static QMutex mutex;
    return mutex;
}

QMutex &recoveryGenerationMutex() {
    static QMutex mutex;
    return mutex;
}

QHash<QString, quint64> &recoveryGenerations() {
    static QHash<QString, quint64> generations;
    return generations;
}

quint64 nextRecoveryGeneration() {
    static quint64 generation = 0;
    return ++generation;
}

quint64 registerRecoveryWrite(const QString &id) {
    QMutexLocker locker(&recoveryGenerationMutex());
    const quint64 generation = nextRecoveryGeneration();
    recoveryGenerations().insert(id, generation);
    return generation;
}

bool isCurrentRecoveryGeneration(const QString &id, quint64 generation) {
    return recoveryGenerations().value(id) == generation;
}

bool isValidRecoveryId(const QString &id) {
    return AviQtl::RustCore::Policy::isValidRecoveryId(id);
}

bool isValidSnapshotFileName(const QString &id, const QString &fileName) {
    return AviQtl::RustCore::Policy::isValidRecoverySnapshotName(id, fileName);
}

QString snapshotFileNameFromMetadata(const QString &id, const QJsonObject &metadata) {
    const QString fileName = metadata.value(QStringLiteral("snapshotFile")).toString(legacySnapshotFileName(id));
    return isValidSnapshotFileName(id, fileName) ? fileName : QString();
}

QString existingSnapshotFileName(const QString &id) {
    QFile file(metadataPath(id));
    if (!file.open(QIODevice::ReadOnly))
        return {};
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
    return document.isObject() ? snapshotFileNameFromMetadata(id, document.object()) : QString();
}

QString recoveryIdFromSnapshotFileName(const QString &fileName) {
    constexpr qsizetype uuidLength = 36;
    if (!fileName.endsWith(QStringLiteral(".aviqtl")) || fileName.size() < uuidLength + 7)
        return {};
    const QString id = fileName.first(uuidLength);
    return isValidSnapshotFileName(id, fileName) ? id : QString();
}

bool setError(QString *errorMessage, const QString &message) {
    if (errorMessage != nullptr)
        *errorMessage = message;
    return false;
}

bool writeCapturedSnapshot(const QString &id, quint64 generation, const QString &originalProjectUrl,
                           const QString &displayName, const QVariantMap &snapshot,
                           QString *errorMessage) {
    if (!isValidRecoveryId(id))
        return setError(errorMessage, QStringLiteral("Invalid recovery snapshot identifier"));

    {
        QMutexLocker generationLocker(&recoveryGenerationMutex());
        if (!isCurrentRecoveryGeneration(id, generation))
            return true;
    }

    QDir root(ProjectRecoveryManager::recoveryRoot());
    if (!root.mkpath(QStringLiteral(".")))
        return setError(errorMessage, QStringLiteral("Could not create recovery directory: %1").arg(root.path()));

    const QString newSnapshotFile = generatedSnapshotFileName(id);
    QString serializerError;
    if (!AviQtl::Core::ProjectSerializer::saveSnapshot(snapshotPath(newSnapshotFile), snapshot, &serializerError))
        return setError(errorMessage, serializerError);

    QMutexLocker mutationLocker(&recoveryMutationMutex());
    QMutexLocker generationLocker(&recoveryGenerationMutex());
    if (!isCurrentRecoveryGeneration(id, generation)) {
        QFile::remove(snapshotPath(newSnapshotFile));
        return true;
    }
    const QString previousSnapshotFile = existingSnapshotFileName(id);

    QJsonObject metadata;
    metadata.insert(QStringLiteral("id"), id);
    metadata.insert(QStringLiteral("originalProjectUrl"), originalProjectUrl);
    metadata.insert(QStringLiteral("displayName"), displayName);
    metadata.insert(QStringLiteral("savedAt"), QDateTime::currentDateTimeUtc().toString(Qt::ISODateWithMs));
    metadata.insert(QStringLiteral("snapshotFile"), newSnapshotFile);

    QSaveFile file(metadataPath(id));
    if (!file.open(QIODevice::WriteOnly)) {
        QFile::remove(snapshotPath(newSnapshotFile));
        return setError(errorMessage, file.errorString());
    }
    const QByteArray document = QJsonDocument(metadata).toJson(QJsonDocument::Compact);
    if (file.write(document) != document.size() || !file.commit()) {
        const QString error = file.errorString();
        file.cancelWriting();
        QFile::remove(snapshotPath(newSnapshotFile));
        return setError(errorMessage, error);
    }

    if (!previousSnapshotFile.isEmpty() && previousSnapshotFile != newSnapshotFile)
        QFile::remove(snapshotPath(previousSnapshotFile));
    return true;
}
} // namespace

QString ProjectRecoveryManager::recoveryRoot() { return QDir(QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation)).filePath(QStringLiteral("recovery")); }

QFuture<ProjectRecoveryWriteResult> ProjectRecoveryManager::writeAsync(const QString &id, const QString &originalProjectUrl, const QString &displayName, TimelineService *timeline, const ProjectService *project) {
    if (!isValidRecoveryId(id) || timeline == nullptr || project == nullptr) {
        return QtConcurrent::run([] { return ProjectRecoveryWriteResult{false, QStringLiteral("Invalid recovery snapshot request")}; });
    }

    const QVariantMap snapshot = AviQtl::Core::ProjectSerializer::captureSnapshot(timeline, project);
    const quint64 generation = registerRecoveryWrite(id);
    return QtConcurrent::run([id, generation, originalProjectUrl, displayName, snapshot]() {
        ProjectRecoveryWriteResult result;
        result.success = writeCapturedSnapshot(id, generation, originalProjectUrl, displayName, snapshot, &result.error);
        return result;
    });
}

bool ProjectRecoveryManager::remove(const QString &id) {
    if (!isValidRecoveryId(id))
        return false;

    QMutexLocker mutationLocker(&recoveryMutationMutex());
    QMutexLocker generationLocker(&recoveryGenerationMutex());
    recoveryGenerations().remove(id);

    bool snapshotsRemoved = true;
    const QDir root(recoveryRoot());
    const QFileInfoList snapshots = root.entryInfoList({id + QStringLiteral("*.aviqtl")}, QDir::Files);
    for (const QFileInfo &snapshot : snapshots) {
        if (isValidSnapshotFileName(id, snapshot.fileName()))
            snapshotsRemoved = QFile::remove(snapshot.filePath()) && snapshotsRemoved;
    }
    const bool metadataRemoved = !QFileInfo::exists(metadataPath(id)) || (snapshotsRemoved && QFile::remove(metadataPath(id)));
    return metadataRemoved && snapshotsRemoved;
}

QList<ProjectRecoveryEntry> ProjectRecoveryManager::entries() {
    QMutexLocker mutationLocker(&recoveryMutationMutex());
    QList<ProjectRecoveryEntry> result;
    const QDir root(recoveryRoot());
    const QFileInfoList files = root.entryInfoList({QStringLiteral("*.json")}, QDir::Files, QDir::Time);
    for (const QFileInfo &info : files) {
        ProjectRecoveryEntry entry;
        entry.id = info.completeBaseName();

        if (!isValidRecoveryId(entry.id)) {
            entry.error = QStringLiteral("Recovery identifier is invalid");
            result.append(entry);
            continue;
        }
        if (info.isSymLink()) {
            entry.error = QStringLiteral("Recovery metadata must not be a symbolic link");
            result.append(entry);
            continue;
        }

        QFile file(info.filePath());
        QJsonParseError parseError;
        if (!file.open(QIODevice::ReadOnly)) {
            entry.error = file.errorString();
        } else {
            const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &parseError);
            if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
                entry.error = parseError.error != QJsonParseError::NoError ? parseError.errorString() : QStringLiteral("Recovery metadata must be a JSON object");
            } else {
                const QJsonObject metadata = document.object();
                entry.originalProjectUrl = metadata.value(QStringLiteral("originalProjectUrl")).toString();
                entry.displayName = metadata.value(QStringLiteral("displayName")).toString();
                entry.savedAt = QDateTime::fromString(metadata.value(QStringLiteral("savedAt")).toString(), Qt::ISODateWithMs);
                const QString snapshotFile = snapshotFileNameFromMetadata(entry.id, metadata);
                entry.snapshotPath = snapshotFile.isEmpty() ? QString() : snapshotPath(snapshotFile);
                if (metadata.value(QStringLiteral("id")).toString() != entry.id) {
                    entry.error = QStringLiteral("Recovery identifier does not match its file name");
                } else if (snapshotFile.isEmpty()) {
                    entry.error = QStringLiteral("Recovery snapshot file name is invalid");
                } else if (!entry.savedAt.isValid()) {
                    entry.error = QStringLiteral("Recovery timestamp is invalid");
                } else if (!QFileInfo::exists(entry.snapshotPath)) {
                    entry.error = QStringLiteral("Recovery snapshot is missing");
                } else if (QFileInfo(entry.snapshotPath).isSymLink()) {
                    entry.error = QStringLiteral("Recovery snapshot must not be a symbolic link");
                } else {
                    entry.valid = true;
                }
            }
        }
        result.append(entry);
    }
    return result;
}

void ProjectRecoveryManager::cleanupStale(int maximumAgeDays) {
    if (maximumAgeDays < 0)
        return;
    const QDateTime cutoff = QDateTime::currentDateTimeUtc().addDays(-maximumAgeDays);
    const QList<ProjectRecoveryEntry> recoveryEntries = entries();
    QSet<QString> referencedSnapshots;
    for (const ProjectRecoveryEntry &entry : recoveryEntries) {
        const QDateTime entryTimestamp = entry.savedAt.isValid() ? entry.savedAt : QFileInfo(metadataPath(entry.id)).lastModified().toUTC();
        if (entryTimestamp.isValid() && entryTimestamp < cutoff)
            remove(entry.id);
        else if (entry.valid)
            referencedSnapshots.insert(QFileInfo(entry.snapshotPath).absoluteFilePath());
    }

    const QDir root(recoveryRoot());
    QMutexLocker mutationLocker(&recoveryMutationMutex());
    const QFileInfoList snapshots = root.entryInfoList({QStringLiteral("*.aviqtl")}, QDir::Files);
    for (const QFileInfo &snapshot : snapshots) {
        const QString id = recoveryIdFromSnapshotFileName(snapshot.fileName());
        if (id.isEmpty())
            continue;
        if (!referencedSnapshots.contains(snapshot.absoluteFilePath()) && snapshot.lastModified().toUTC() < cutoff)
            QFile::remove(snapshot.filePath());
    }
}

} // namespace AviQtl::UI
