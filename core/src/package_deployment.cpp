#include "package_deployment.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QScopeGuard>
#include <QTemporaryDir>
#include <QDebug>
#include <QtCore/private/qzipreader_p.h>
#include <utility>

namespace AviQtl::Core::Internal {
namespace {
constexpr qint64 kMaxPackageExtractedBytes = 1024LL * 1024LL * 1024LL;
constexpr qsizetype kMaxPackageArchiveEntries = 10000;

QString transactionWorkspace(const QString &deployDirectory) {
    const QFileInfo deployInfo(QDir::cleanPath(deployDirectory));
    return QDir(deployInfo.absolutePath())
        .filePath(QStringLiteral(".%1-package-deployment").arg(deployInfo.fileName()));
}

bool copyDirectory(const QString &srcPath, const QString &destPath) {
    QDir srcDir(srcPath);
    if (!srcDir.exists())
        return false;
    if (!QDir(destPath).exists() && !QDir().mkpath(destPath))
        return false;

    const QStringList entries = srcDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
    for (const QString &entry : entries) {
        const QString srcFilePath = srcDir.absoluteFilePath(entry);
        const QString destFilePath = QDir(destPath).filePath(entry);
        const QFileInfo srcInfo(srcFilePath);
        if (srcInfo.isDir()) {
            if (!copyDirectory(srcFilePath, destFilePath))
                return false;
        } else {
            if (QFile::exists(destFilePath) && !QFile::remove(destFilePath))
                return false;
            if (!QFile::copy(srcFilePath, destFilePath))
                return false;
        }
    }
    return true;
}
} // namespace

bool PackageDeployment::isValidPackageId(const QString &packageId) {
    if (packageId.isEmpty() || packageId == QStringLiteral(".") || packageId == QStringLiteral(".."))
        return false;
    for (const QChar ch : packageId) {
        if (!ch.isLetterOrNumber() && ch != QLatin1Char('.') && ch != QLatin1Char('-') && ch != QLatin1Char('_'))
            return false;
    }
    return true;
}

bool PackageDeployment::isValidPackageType(const QString &packageType) {
    return packageType == QStringLiteral("mod") || packageType == QStringLiteral("effect") ||
           packageType == QStringLiteral("object");
}

QString PackageDeployment::deployDirectory(const QString &packageType) {
    const QString appDir = QCoreApplication::applicationDirPath();
    if (packageType == QStringLiteral("mod"))
        return QDir(appDir).filePath(QStringLiteral("plugins"));
    if (packageType == QStringLiteral("effect"))
        return QDir(appDir).filePath(QStringLiteral("effects"));
    if (packageType == QStringLiteral("object"))
        return QDir(appDir).filePath(QStringLiteral("objects"));
    return {};
}

PackageDeployment::FileOperationResult PackageDeployment::deployFiles(
    const QString &packageId, const QString &extractDir, const QString &packageType,
    const std::function<bool()> &commitState) {
    if (!isValidPackageId(packageId) || !isValidPackageType(packageType)) {
        qWarning() << "[PackageDeployment] Invalid package ID or type:" << packageId << packageType;
        return FileOperationResult::Failed;
    }

    const QString deployBase = deployDirectory(packageType);
    if (deployBase.isEmpty())
        return FileOperationResult::Failed;
    const QString packageDir = QDir(deployBase).filePath(packageId);

    QDir sourceDir(extractDir);
    QStringList entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
    if (entries.size() == 1) {
        const QFileInfo wrapper(sourceDir.absoluteFilePath(entries.first()));
        if (wrapper.isDir()) {
            sourceDir.cd(entries.first());
            entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
        }
    }

    if (!QDir().mkpath(deployBase))
        return FileOperationResult::Failed;
    const QString workspace = transactionWorkspace(deployBase);
    if (!QDir().mkpath(workspace))
        return FileOperationResult::Failed;
    const auto workspaceCleanup = qScopeGuard([workspace]() { QDir().rmdir(workspace); });
    QTemporaryDir stagingDir(QDir(workspace).filePath(QStringLiteral(".staging_XXXXXX")));
    if (!stagingDir.isValid()) {
        qWarning() << "[PackageDeployment] Failed to create staging directory.";
        return FileOperationResult::Failed;
    }

    const QString stagingPath = stagingDir.path();
    for (const QString &entry : std::as_const(entries)) {
        const QString srcPath = sourceDir.absoluteFilePath(entry);
        const QString destPath = QDir(stagingPath).filePath(entry);
        const QFileInfo srcInfo(srcPath);
        if (srcInfo.isDir()) {
            if (!copyDirectory(srcPath, destPath))
                return FileOperationResult::Failed;
        } else if (!QFile::copy(srcPath, destPath)) {
            return FileOperationResult::Failed;
        }
    }

    const QString backupDir = QDir(workspace).filePath(QStringLiteral(".backup_") + packageId);
    const FileOperationResult result = runFileTransaction(
        packageDir, backupDir,
        [stagingPath, packageDir] { return QDir().rename(stagingPath, packageDir); },
        [packageDir] { return !QDir(packageDir).exists() || QDir(packageDir).removeRecursively(); },
        commitState, "deploy");
    if (result == FileOperationResult::Success)
        stagingDir.setAutoRemove(false);
    return result;
}

PackageDeployment::FileOperationResult PackageDeployment::removeFiles(
    const QString &packageId, const QString &packageType, const std::function<bool()> &commitState) {
    if (!isValidPackageId(packageId) || !isValidPackageType(packageType) || !commitState)
        return FileOperationResult::Failed;
    const QString deployDir = deployDirectory(packageType);
    if (deployDir.isEmpty())
        return FileOperationResult::Failed;

    const QString workspace = transactionWorkspace(deployDir);
    if (!QDir().mkpath(workspace))
        return FileOperationResult::Failed;
    const auto workspaceCleanup = qScopeGuard([workspace]() { QDir().rmdir(workspace); });
    const QString packageDir = QDir(deployDir).filePath(packageId);
    const QString backupDir = QDir(workspace).filePath(QStringLiteral(".remove_backup_") + packageId);
    return runFileTransaction(packageDir, backupDir, [] { return true; }, [] { return true; }, commitState, "remove");
}

bool PackageDeployment::extractArchive(const QString &archivePath, const QString &destDir) {
    QZipReader reader(archivePath);
    if (!reader.exists() || !reader.isReadable() || reader.status() != QZipReader::NoError) {
        qWarning() << "[PackageDeployment] Package archive is not a readable ZIP file.";
        return false;
    }

    const QList<QZipReader::FileInfo> entries = reader.fileInfoList();
    if (entries.size() > kMaxPackageArchiveEntries)
        return false;

    qint64 extractedBytes = 0;
    for (const QZipReader::FileInfo &entry : entries) {
        if (!entry.isValid() || entry.isSymLink || !isSafeArchivePath(entry.filePath) || entry.size < 0 ||
            extractedBytes > kMaxPackageExtractedBytes - entry.size) {
            qWarning() << "[PackageDeployment] Unsafe package archive entry:" << entry.filePath;
            return false;
        }
        extractedBytes += entry.size;
    }

    return QDir().mkpath(destDir) && reader.extractAll(destDir);
}

bool PackageDeployment::isSafeArchivePath(const QString &path) {
    if (path.isEmpty() || QDir::isAbsolutePath(path) || path.contains(QLatin1Char('\\')))
        return false;
    const QString normalized = QDir::cleanPath(path);
    return normalized != QStringLiteral("..") && !normalized.startsWith(QStringLiteral("../")) &&
           !normalized.contains(QStringLiteral("/../"));
}

PackageDeployment::FileOperationResult PackageDeployment::runFileTransaction(
    const QString &targetDir, const QString &backupDir,
    const std::function<bool()> &applyMutation, const std::function<bool()> &revertMutation,
    const std::function<bool()> &commitState, const char *operationName) {
    const bool hadExisting = QDir(targetDir).exists();
    if (hadExisting) {
        if (QDir(backupDir).exists() && !QDir(backupDir).removeRecursively()) {
            qWarning() << "[PackageDeployment] Could not remove stale backup before" << operationName << ':' << backupDir;
            return FileOperationResult::Failed;
        }
        if (!QDir().rename(targetDir, backupDir)) {
            qWarning() << "[PackageDeployment] Could not create backup before" << operationName << ':' << targetDir;
            return FileOperationResult::Failed;
        }
    }

    if (!applyMutation()) {
        qWarning() << "[PackageDeployment] File mutation failed during" << operationName;
        if (hadExisting && !QDir().rename(backupDir, targetDir)) {
            qCritical() << "[PackageDeployment] Could not restore backup after mutation failure:" << backupDir;
            return FileOperationResult::RollbackFailed;
        }
        return FileOperationResult::Failed;
    }

    if (commitState && !commitState()) {
        qWarning() << "[PackageDeployment] State commit failed during" << operationName << "; rolling back.";
        const bool reverted = revertMutation();
        const bool restored = !hadExisting || (reverted && QDir().rename(backupDir, targetDir));
        if (!reverted || !restored) {
            qCritical() << "[PackageDeployment] Could not restore backup after state commit failure:" << backupDir;
            return FileOperationResult::RollbackFailed;
        }
        return FileOperationResult::StateCommitFailed;
    }

    if (hadExisting && !QDir(backupDir).removeRecursively())
        qWarning() << "[PackageDeployment] Operation succeeded but backup cleanup failed:" << backupDir;
    return FileOperationResult::Success;
}

} // namespace AviQtl::Core::Internal
