#pragma once

#include "rust_core_abi.hpp"
#include <QJsonDocument>
#include <QJsonObject>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>
#include <cstdint>
#include <optional>
#include <vector>

namespace AviQtl::RustCore::Package {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

enum class RepositoryOperation { Add, Remove, Enabled, Priority };

struct RepositoryMutation {
    QVariantList repositories;
    bool changed = false;
};

struct InstallSelection {
    QString status;
    QString version;
    QString downloadUrl;
    QString sha256;
    QString minAppVersion;
    QString type;
};

inline std::optional<QVariantMap> apply(const QVariantMap &input) {
    const QByteArray json = QJsonDocument(QJsonObject::fromVariantMap(input)).toJson(QJsonDocument::Compact);
    const auto *data = reinterpret_cast<const std::uint8_t *>(json.constData());
    const auto size = static_cast<std::size_t>(json.size());
    std::size_t required = 0;
    auto status = static_cast<Status>(aviqtl_package_document_apply_json(data, size, nullptr, 0, &required));
    if (status != Status::BufferTooSmall)
        return std::nullopt;
    std::vector<std::uint8_t> output(required);
    std::size_t written = 0;
    status = static_cast<Status>(aviqtl_package_document_apply_json(data, size, output.data(), output.size(), &written));
    if (status != Status::Ok || written != output.size())
        return std::nullopt;
    const QJsonDocument document = QJsonDocument::fromJson(QByteArray(reinterpret_cast<const char *>(output.data()), static_cast<qsizetype>(output.size())));
    return document.isObject() ? std::optional<QVariantMap>{document.object().toVariantMap()} : std::nullopt;
}

inline std::optional<QVariantList> mergeCatalog(const QVariantList &catalog, const QVariantMap &package, const QVariantMap &repository, const QVariantList &repositories, const QVariantMap &installed, const QString &language, const QString &appVersion) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("mergeCatalog")},
        {QStringLiteral("catalog"), catalog},
        {QStringLiteral("package"), package},
        {QStringLiteral("repository"), repository},
        {QStringLiteral("repositories"), repositories},
        {QStringLiteral("installed"), installed},
        {QStringLiteral("language"), language},
        {QStringLiteral("appVersion"), appVersion},
    });
    return result.has_value() ? std::optional<QVariantList>{result->value(QStringLiteral("catalog")).toList()} : std::nullopt;
}

inline bool hasUpdates(const QVariantList &catalog) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("hasUpdates")},
        {QStringLiteral("catalog"), catalog},
    });
    return result.has_value() && result->value(QStringLiteral("value")).toBool();
}

inline int compareVersions(const QString &left, const QString &right) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("compareVersions")},
        {QStringLiteral("left"), left},
        {QStringLiteral("right"), right},
    });
    return result.has_value() ? result->value(QStringLiteral("value")).toInt() : 0;
}

inline QStringList upgradeIds(const QVariantList &catalog) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("upgradeIds")},
        {QStringLiteral("catalog"), catalog},
    });
    QStringList ids;
    if (result.has_value()) {
        for (const QVariant &id : result->value(QStringLiteral("ids")).toList())
            ids.append(id.toString());
    }
    return ids;
}

inline QVariantList filter(const QVariantList &catalog, const QString &filter) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("filter")},
        {QStringLiteral("catalog"), catalog},
        {QStringLiteral("filter"), filter},
    });
    return result.has_value() ? result->value(QStringLiteral("catalog")).toList() : QVariantList{};
}

inline QVariantMap find(const QVariantList &catalog, const QString &packageId, const QString &sourceRepository) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("find")},
        {QStringLiteral("catalog"), catalog},
        {QStringLiteral("packageId"), packageId},
        {QStringLiteral("sourceRepository"), sourceRepository},
    });
    return result.has_value() ? result->value(QStringLiteral("package")).toMap() : QVariantMap{};
}

inline QVariantList setInstalled(const QVariantList &catalog, const QString &packageId, const std::optional<QString> &version) {
    QVariantMap input{
        {QStringLiteral("operation"), QStringLiteral("setInstalled")},
        {QStringLiteral("catalog"), catalog},
        {QStringLiteral("packageId"), packageId},
    };
    if (version.has_value())
        input.insert(QStringLiteral("version"), *version);
    const auto result = apply(input);
    return result.has_value() ? result->value(QStringLiteral("catalog")).toList() : catalog;
}

inline RepositoryMutation mutateRepositories(const QVariantList &repositories, RepositoryOperation operation, const QString &url, bool enabled = true, int priority = 10) {
    QString operationName;
    switch (operation) {
    case RepositoryOperation::Add:
        operationName = QStringLiteral("add");
        break;
    case RepositoryOperation::Remove:
        operationName = QStringLiteral("remove");
        break;
    case RepositoryOperation::Enabled:
        operationName = QStringLiteral("enabled");
        break;
    case RepositoryOperation::Priority:
        operationName = QStringLiteral("priority");
        break;
    }
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("repositories")},
        {QStringLiteral("repositories"), repositories},
        {QStringLiteral("repositoryOperation"), operationName},
        {QStringLiteral("url"), url},
        {QStringLiteral("enabled"), enabled},
        {QStringLiteral("priority"), priority},
    });
    return result.has_value() ? RepositoryMutation{result->value(QStringLiteral("repositories")).toList(), result->value(QStringLiteral("changed")).toBool()} : RepositoryMutation{repositories, false};
}

inline QVariantList enabledRepositories(const QVariantList &repositories) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("enabledRepositories")},
        {QStringLiteral("repositories"), repositories},
    });
    return result.has_value() ? result->value(QStringLiteral("repositories")).toList() : QVariantList{};
}

inline std::optional<QVariantMap> normalizeMetadata(const QVariantMap &detail) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("normalizeMetadata")},
        {QStringLiteral("detail"), detail},
    });
    return result.has_value() ? std::optional<QVariantMap>{result->value(QStringLiteral("detail")).toMap()} : std::nullopt;
}

inline std::optional<InstallSelection> selectInstall(const QVariantMap &detail, const QString &requestedVersion, const QString &appVersion) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("selectInstall")},
        {QStringLiteral("detail"), detail},
        {QStringLiteral("requestedVersion"), requestedVersion},
        {QStringLiteral("appVersion"), appVersion},
    });
    if (!result.has_value())
        return std::nullopt;
    const QVariantMap selection = result->value(QStringLiteral("selection")).toMap();
    return InstallSelection{
        selection.value(QStringLiteral("status")).toString(), selection.value(QStringLiteral("version")).toString(),       selection.value(QStringLiteral("downloadUrl")).toString(),
        selection.value(QStringLiteral("sha256")).toString(), selection.value(QStringLiteral("minAppVersion")).toString(), selection.value(QStringLiteral("type")).toString(),
    };
}

} // namespace AviQtl::RustCore::Package
