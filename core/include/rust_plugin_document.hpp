#pragma once

#include "rust_core_abi.hpp"
#include <QJsonDocument>
#include <QJsonObject>
#include <QVariantList>
#include <QVariantMap>
#include <cstdint>
#include <optional>
#include <vector>

namespace AviQtl::RustCore::Plugin {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

inline std::optional<QVariantMap> apply(const QVariantMap &input) {
    const QByteArray json =
        QJsonDocument(QJsonObject::fromVariantMap(input)).toJson(QJsonDocument::Compact);
    const auto *data = reinterpret_cast<const std::uint8_t *>(json.constData());
    const auto size = static_cast<std::size_t>(json.size());
    std::size_t required = 0;
    auto status = static_cast<Status>(
        aviqtl_plugin_document_apply_json(data, size, nullptr, 0, &required));
    if (status != Status::BufferTooSmall)
        return std::nullopt;
    std::vector<std::uint8_t> output(required);
    std::size_t written = 0;
    status = static_cast<Status>(aviqtl_plugin_document_apply_json(
        data, size, output.data(), output.size(), &written));
    if (status != Status::Ok || written != output.size())
        return std::nullopt;
    const QJsonDocument document = QJsonDocument::fromJson(QByteArray(
        reinterpret_cast<const char *>(output.data()), static_cast<qsizetype>(output.size())));
    return document.isObject()
               ? std::optional<QVariantMap>{document.object().toVariantMap()}
               : std::nullopt;
}

inline std::optional<QVariantList> parseDiscovery(const QString &output, const QString &format,
                                                  const QString &filePath,
                                                  const QString &fallbackName) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("parseDiscovery")},
        {QStringLiteral("output"), output},
        {QStringLiteral("format"), format},
        {QStringLiteral("filePath"), filePath},
        {QStringLiteral("fallbackName"), fallbackName},
    });
    return result.has_value()
               ? std::optional<QVariantList>{result->value(QStringLiteral("plugins")).toList()}
               : std::nullopt;
}

inline QVariantList deduplicate(const QVariantList &plugins) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("deduplicate")},
        {QStringLiteral("plugins"), plugins},
    });
    return result.has_value() ? result->value(QStringLiteral("plugins")).toList()
                              : QVariantList{};
}

inline QVariantList publicList(const QVariantList &plugins) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("list")},
        {QStringLiteral("plugins"), plugins},
    });
    return result.has_value() ? result->value(QStringLiteral("plugins")).toList()
                              : QVariantList{};
}

inline QVariantList categories(const QVariantList &plugins) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("categories")},
        {QStringLiteral("plugins"), plugins},
    });
    return result.has_value() ? result->value(QStringLiteral("categories")).toList()
                              : QVariantList{};
}

inline QVariantList filter(const QVariantList &plugins, const QString &category) {
    const auto result = apply({
        {QStringLiteral("operation"), QStringLiteral("filter")},
        {QStringLiteral("plugins"), plugins},
        {QStringLiteral("category"), category},
    });
    return result.has_value() ? result->value(QStringLiteral("plugins")).toList()
                              : QVariantList{};
}

} // namespace AviQtl::RustCore::Plugin
