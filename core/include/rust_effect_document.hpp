#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QVariantMap>
#include <cstdint>
#include <optional>
#include <vector>

namespace AviQtl::RustCore::Effect {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

inline std::optional<QVariantMap> normalizeMetadata(const QByteArray &input) {
    const auto *data = reinterpret_cast<const std::uint8_t *>(input.constData());
    const auto size = static_cast<std::size_t>(input.size());
    std::size_t required = 0;
    auto status = static_cast<Status>(aviqtl_effect_metadata_normalize_json(data, size, nullptr, 0, &required));
    if (status != Status::BufferTooSmall)
        return std::nullopt;
    std::vector<std::uint8_t> output(required);
    std::size_t written = 0;
    status = static_cast<Status>(aviqtl_effect_metadata_normalize_json(data, size, output.data(), output.size(), &written));
    if (status != Status::Ok || written != output.size())
        return std::nullopt;
    const QJsonDocument document = QJsonDocument::fromJson(QByteArray(reinterpret_cast<const char *>(output.data()), static_cast<qsizetype>(output.size())));
    return document.isObject() ? std::optional<QVariantMap>{document.object().toVariantMap()} : std::nullopt;
}

} // namespace AviQtl::RustCore::Effect
