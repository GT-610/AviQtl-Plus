#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QString>
#include <cstdint>
#include <limits>
#include <optional>
#include <vector>

namespace AviQtl::RustCore::Recovery {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

struct MetadataInspection {
    QString id;
    QString originalProjectUrl;
    QString displayName;
    QString savedAt;
    QString snapshotFile;
    QString status;
};

template <typename Function> inline std::optional<QByteArray> collect(Function function) {
    std::size_t required = 0;
    auto status = static_cast<Status>(function(nullptr, 0, &required));
    if (status == Status::Ok && required == 0)
        return QByteArray{};
    if (status != Status::BufferTooSmall || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        return std::nullopt;
    }
    QByteArray output(static_cast<qsizetype>(required), Qt::Uninitialized);
    std::size_t written = 0;
    status = static_cast<Status>(function(
        reinterpret_cast<std::uint8_t *>(output.data()), required, &written));
    if (status != Status::Ok || written != required)
        return std::nullopt;
    return output;
}

inline std::optional<MetadataInspection> inspectMetadata(const QString &id,
                                                         const QByteArray &metadata) {
    const QByteArray encodedId = id.toUtf8();
    const auto output = collect([&](std::uint8_t *data, std::size_t capacity,
                                    std::size_t *length) {
        return aviqtl_recovery_metadata_inspect_json(
            reinterpret_cast<const std::uint8_t *>(encodedId.constData()),
            static_cast<std::size_t>(encodedId.size()),
            reinterpret_cast<const std::uint8_t *>(metadata.constData()),
            static_cast<std::size_t>(metadata.size()), data, capacity, length);
    });
    if (!output.has_value())
        return std::nullopt;
    const QJsonDocument document = QJsonDocument::fromJson(*output);
    if (!document.isObject())
        return std::nullopt;
    const QJsonObject value = document.object();
    return MetadataInspection{
        value.value(QStringLiteral("id")).toString(),
        value.value(QStringLiteral("originalProjectUrl")).toString(),
        value.value(QStringLiteral("displayName")).toString(),
        value.value(QStringLiteral("savedAt")).toString(),
        value.value(QStringLiteral("snapshotFile")).toString(),
        value.value(QStringLiteral("status")).toString(),
    };
}

inline std::optional<QByteArray> buildMetadata(const QString &id,
                                               const QString &originalProjectUrl,
                                               const QString &displayName,
                                               const QString &savedAt,
                                               const QString &snapshotFile) {
    const QByteArray encodedId = id.toUtf8();
    const QByteArray encodedOriginalUrl = originalProjectUrl.toUtf8();
    const QByteArray encodedDisplayName = displayName.toUtf8();
    const QByteArray encodedSavedAt = savedAt.toUtf8();
    const QByteArray encodedSnapshot = snapshotFile.toUtf8();
    return collect([&](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
        return aviqtl_recovery_metadata_build_json(
            reinterpret_cast<const std::uint8_t *>(encodedId.constData()),
            static_cast<std::size_t>(encodedId.size()),
            reinterpret_cast<const std::uint8_t *>(encodedOriginalUrl.constData()),
            static_cast<std::size_t>(encodedOriginalUrl.size()),
            reinterpret_cast<const std::uint8_t *>(encodedDisplayName.constData()),
            static_cast<std::size_t>(encodedDisplayName.size()),
            reinterpret_cast<const std::uint8_t *>(encodedSavedAt.constData()),
            static_cast<std::size_t>(encodedSavedAt.size()),
            reinterpret_cast<const std::uint8_t *>(encodedSnapshot.constData()),
            static_cast<std::size_t>(encodedSnapshot.size()), data, capacity, length);
    });
}

inline QString snapshotId(const QString &fileName) {
    const QByteArray encoded = fileName.toUtf8();
    const auto output = collect([&](std::uint8_t *data, std::size_t capacity,
                                    std::size_t *length) {
        return aviqtl_recovery_snapshot_id(
            reinterpret_cast<const std::uint8_t *>(encoded.constData()),
            static_cast<std::size_t>(encoded.size()), data, capacity, length);
    });
    return output.has_value() ? QString::fromUtf8(*output) : QString();
}

} // namespace AviQtl::RustCore::Recovery
