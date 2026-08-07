#pragma once

#include <QString>
#include <functional>

namespace AviQtl::Core::Internal {

class PackageDeployment {
  public:
    enum class FileOperationResult {
        Success,
        Failed,
        StateCommitFailed,
        RollbackFailed,
    };

    static bool isValidPackageId(const QString &packageId);
    static bool isValidPackageType(const QString &packageType);
    static QString deployDirectory(const QString &packageType);
    static FileOperationResult deployFiles(const QString &packageId, const QString &extractDir, const QString &packageType,
                                           const std::function<bool()> &commitState = {});
    static FileOperationResult removeFiles(const QString &packageId, const QString &packageType,
                                           const std::function<bool()> &commitState);
    static bool extractArchive(const QString &archivePath, const QString &destDir);
    static bool isSafeArchivePath(const QString &path);

  private:
    static FileOperationResult runFileTransaction(const QString &targetDir, const QString &backupDir,
                                                  const std::function<bool()> &applyMutation,
                                                  const std::function<bool()> &revertMutation,
                                                  const std::function<bool()> &commitState,
                                                  const char *operationName);
};

} // namespace AviQtl::Core::Internal
