#include "project_recovery_manager.hpp"
#include "project_serializer.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QStandardPaths>

namespace AviQtl::UI {
namespace {
QString recoveryRootOverride;

QString metadataPath(const QString &id) { return QDir(ProjectRecoveryManager::recoveryRoot()).filePath(id + QStringLiteral(".json")); }
QString snapshotPath(const QString &id) { return QDir(ProjectRecoveryManager::recoveryRoot()).filePath(id + QStringLiteral(".aviqtl")); }

bool setError(QString *errorMessage, const QString &message) {
    if (errorMessage != nullptr)
        *errorMessage = message;
    return false;
}
} // namespace

QString ProjectRecoveryManager::recoveryRoot() {
    if (!recoveryRootOverride.isEmpty())
        return recoveryRootOverride;
    return QDir(QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation)).filePath(QStringLiteral("recovery"));
}

void ProjectRecoveryManager::setRecoveryRootForTests(const QString &path) { recoveryRootOverride = path; }

bool ProjectRecoveryManager::write(const QString &id, const QString &originalProjectUrl, const QString &displayName, const TimelineService *timeline, const ProjectService *project, QString *errorMessage) {
    if (id.isEmpty() || timeline == nullptr || project == nullptr)
        return setError(errorMessage, QStringLiteral("Invalid recovery snapshot request"));

    QDir root(recoveryRoot());
    if (!root.mkpath(QStringLiteral(".")))
        return setError(errorMessage, QStringLiteral("Could not create recovery directory: %1").arg(root.path()));

    QString serializerError;
    if (!AviQtl::Core::ProjectSerializer::save(snapshotPath(id), timeline, project, &serializerError))
        return setError(errorMessage, serializerError);

    QJsonObject metadata;
    metadata.insert(QStringLiteral("id"), id);
    metadata.insert(QStringLiteral("originalProjectUrl"), originalProjectUrl);
    metadata.insert(QStringLiteral("displayName"), displayName);
    metadata.insert(QStringLiteral("savedAt"), QDateTime::currentDateTimeUtc().toString(Qt::ISODateWithMs));

    QSaveFile file(metadataPath(id));
    if (!file.open(QIODevice::WriteOnly)) {
        QFile::remove(snapshotPath(id));
        return setError(errorMessage, file.errorString());
    }
    const QByteArray document = QJsonDocument(metadata).toJson(QJsonDocument::Compact);
    if (file.write(document) != document.size() || !file.commit()) {
        const QString error = file.errorString();
        file.cancelWriting();
        QFile::remove(snapshotPath(id));
        return setError(errorMessage, error);
    }
    return true;
}

bool ProjectRecoveryManager::remove(const QString &id) {
    const bool metadataRemoved = !QFileInfo::exists(metadataPath(id)) || QFile::remove(metadataPath(id));
    const bool snapshotRemoved = !QFileInfo::exists(snapshotPath(id)) || QFile::remove(snapshotPath(id));
    return metadataRemoved && snapshotRemoved;
}

QList<ProjectRecoveryEntry> ProjectRecoveryManager::entries() {
    QList<ProjectRecoveryEntry> result;
    const QDir root(recoveryRoot());
    const QFileInfoList files = root.entryInfoList({QStringLiteral("*.json")}, QDir::Files, QDir::Time);
    for (const QFileInfo &info : files) {
        ProjectRecoveryEntry entry;
        entry.id = info.completeBaseName();
        entry.snapshotPath = snapshotPath(entry.id);

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
                if (metadata.value(QStringLiteral("id")).toString() != entry.id) {
                    entry.error = QStringLiteral("Recovery identifier does not match its file name");
                } else if (!entry.savedAt.isValid()) {
                    entry.error = QStringLiteral("Recovery timestamp is invalid");
                } else if (!QFileInfo::exists(entry.snapshotPath)) {
                    entry.error = QStringLiteral("Recovery snapshot is missing");
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
    for (const ProjectRecoveryEntry &entry : entries()) {
        if (entry.savedAt.isValid() && entry.savedAt < cutoff)
            remove(entry.id);
    }
}

} // namespace AviQtl::UI
