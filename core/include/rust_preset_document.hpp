#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QStringView>
#include <QVariantMap>
#include <cstdint>
#include <optional>
#include <vector>

namespace AviQtl::RustCore::Preset {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

inline QByteArray utf8(QStringView value) { return value.toString().toUtf8(); }

inline bool isSafeName(QStringView value) {
    const QByteArray encoded = utf8(value);
    return aviqtl_preset_name_is_safe(reinterpret_cast<const std::uint8_t *>(encoded.constData()), static_cast<std::size_t>(encoded.size())) != 0;
}

template <typename Function> inline std::optional<QByteArray> produce(Function function) {
    std::size_t required = 0;
    auto status = static_cast<Status>(function(nullptr, 0, &required));
    if (status != Status::BufferTooSmall)
        return std::nullopt;
    std::vector<std::uint8_t> output(required);
    std::size_t written = 0;
    status = static_cast<Status>(function(output.data(), output.size(), &written));
    if (status != Status::Ok || written != output.size())
        return std::nullopt;
    return QByteArray(reinterpret_cast<const char *>(output.data()), static_cast<qsizetype>(output.size()));
}

inline std::optional<QByteArray> build(QStringView effectId, QStringView name, const QVariantMap &params, const QVariantMap &keyframes, bool enabled) {
    const QByteArray effectIdUtf8 = utf8(effectId);
    const QByteArray nameUtf8 = utf8(name);
    const QByteArray paramsJson = QJsonDocument(QJsonObject::fromVariantMap(params)).toJson(QJsonDocument::Compact);
    const QByteArray keyframesJson = QJsonDocument(QJsonObject::fromVariantMap(keyframes)).toJson(QJsonDocument::Compact);
    return produce([&](std::uint8_t *output, std::size_t capacity, std::size_t *length) {
        return aviqtl_preset_build_json(reinterpret_cast<const std::uint8_t *>(effectIdUtf8.constData()), static_cast<std::size_t>(effectIdUtf8.size()), reinterpret_cast<const std::uint8_t *>(nameUtf8.constData()),
                                        static_cast<std::size_t>(nameUtf8.size()), enabled ? 1U : 0U, reinterpret_cast<const std::uint8_t *>(paramsJson.constData()), static_cast<std::size_t>(paramsJson.size()),
                                        reinterpret_cast<const std::uint8_t *>(keyframesJson.constData()), static_cast<std::size_t>(keyframesJson.size()), output, capacity, length);
    });
}

inline std::optional<QVariantMap> normalize(QStringView effectId, QStringView name, const QByteArray &input) {
    const QByteArray effectIdUtf8 = utf8(effectId);
    const QByteArray nameUtf8 = utf8(name);
    const auto normalized = produce([&](std::uint8_t *output, std::size_t capacity, std::size_t *length) {
        return aviqtl_preset_normalize_json(reinterpret_cast<const std::uint8_t *>(effectIdUtf8.constData()), static_cast<std::size_t>(effectIdUtf8.size()), reinterpret_cast<const std::uint8_t *>(nameUtf8.constData()),
                                            static_cast<std::size_t>(nameUtf8.size()), reinterpret_cast<const std::uint8_t *>(input.constData()), static_cast<std::size_t>(input.size()), output, capacity, length);
    });
    if (!normalized.has_value())
        return std::nullopt;
    const QJsonDocument document = QJsonDocument::fromJson(*normalized);
    return document.isObject() ? std::optional(document.object().toVariantMap()) : std::nullopt;
}

} // namespace AviQtl::RustCore::Preset
