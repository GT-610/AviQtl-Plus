#pragma once

#include <QByteArray>
#include <QFile>
#include <QString>
#include <optional>

namespace AviQtl::Core::Internal {

namespace FileSizeLimit {
inline constexpr qint64 ProjectDocument = 256LL * 1024LL * 1024LL;
inline constexpr qint64 RepositoryMetadata = 16LL * 1024LL * 1024LL;
inline constexpr qint64 PluginScript = 8LL * 1024LL * 1024LL;
inline constexpr qint64 InstalledPackageState = 4LL * 1024LL * 1024LL;
inline constexpr qint64 EffectDefinition = 1LL * 1024LL * 1024LL;
inline constexpr qint64 ComputeShader = 1LL * 1024LL * 1024LL;
inline constexpr qint64 PluginManifest = 256LL * 1024LL;
} // namespace FileSizeLimit

inline std::optional<QByteArray> readFileBounded(QFile &file, qint64 maxBytes,
                                                 QString *errorMessage = nullptr) {
    if (!file.isOpen() && !file.open(QIODevice::ReadOnly)) {
        if (errorMessage != nullptr)
            *errorMessage = file.errorString();
        return std::nullopt;
    }
    if (maxBytes < 0 || file.size() > maxBytes) {
        if (errorMessage != nullptr)
            *errorMessage = QStringLiteral("File exceeds the maximum allowed size of %1 bytes.").arg(maxBytes);
        return std::nullopt;
    }

    QByteArray data = file.read(maxBytes + 1);
    if (data.size() > maxBytes) {
        if (errorMessage != nullptr)
            *errorMessage = QStringLiteral("File exceeds the maximum allowed size of %1 bytes.").arg(maxBytes);
        return std::nullopt;
    }
    if (file.error() != QFileDevice::NoError) {
        if (errorMessage != nullptr)
            *errorMessage = file.errorString();
        return std::nullopt;
    }
    return data;
}

inline std::optional<QByteArray> readFileBounded(const QString &path, qint64 maxBytes,
                                                 QString *errorMessage = nullptr) {
    QFile file(path);
    return readFileBounded(file, maxBytes, errorMessage);
}

} // namespace AviQtl::Core::Internal
