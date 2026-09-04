#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <QString>
#include <QVariant>
#include <QVariantMap>
#include <cstdint>
#include <optional>
#include <vector>

namespace AviQtl::RustCore::Settings {

enum class Status : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

struct MergeResult {
    QVariantMap settings;
    bool migrated = false;
};

class State final {
  public:
    State() = default;
    State(const State &) = delete;
    State &operator=(const State &) = delete;
    State(State &&other) noexcept;
    State &operator=(State &&other) noexcept;
    ~State();

    [[nodiscard]] bool isValid() const { return m_handle != nullptr; }
    [[nodiscard]] Status reset(const QVariantMap &settings);
    [[nodiscard]] Status mergeLoaded(const QByteArray &loaded, bool &migrated);
    [[nodiscard]] Status snapshot(QVariantMap &settings) const;
    [[nodiscard]] Status persistentJson(QByteArray &output) const;
    [[nodiscard]] Status setValue(const QString &key, const QVariant &value, bool &changed,
                                  bool &persistent);
    [[nodiscard]] Status removeValue(const QString &key, bool &changed, bool &persistent);
    [[nodiscard]] int intValue(const QString &key, int fallback) const;
    [[nodiscard]] double doubleValue(const QString &key, double fallback) const;
    [[nodiscard]] bool boolValue(const QString &key, bool fallback) const;

  private:
    AviQtlSettingsState *m_handle = nullptr;
};

inline QByteArray encode(const QVariantMap &value) { return QJsonDocument(QJsonObject::fromVariantMap(value)).toJson(QJsonDocument::Compact); }

inline std::optional<QVariantMap> decode(const std::vector<std::uint8_t> &value) {
    const QJsonDocument document = QJsonDocument::fromJson(QByteArray(reinterpret_cast<const char *>(value.data()), static_cast<qsizetype>(value.size())));
    if (!document.isObject())
        return std::nullopt;
    return document.object().toVariantMap();
}

template <typename Function> inline Status transformSingle(const QByteArray &input, std::vector<std::uint8_t> &output, Function function) {
    output.clear();
    const auto *data = reinterpret_cast<const std::uint8_t *>(input.constData());
    const auto length = static_cast<std::size_t>(input.size());
    std::size_t required = 0;
    auto status = static_cast<Status>(function(data, length, nullptr, 0, &required));
    if (status != Status::BufferTooSmall)
        return status;
    output.resize(required);
    std::size_t written = 0;
    status = static_cast<Status>(function(data, length, output.data(), output.size(), &written));
    if (status != Status::Ok || written != output.size()) {
        output.clear();
        return status == Status::Ok ? Status::InvalidArgument : status;
    }
    return status;
}

inline std::optional<QVariantMap> defaults(const QVariantMap &platformDefaults) {
    std::vector<std::uint8_t> output;
    const Status status = transformSingle(encode(platformDefaults), output, aviqtl_settings_defaults_json);
    return status == Status::Ok ? decode(output) : std::nullopt;
}

inline std::optional<MergeResult> merge(const QVariantMap &base, const QByteArray &loaded) {
    const QByteArray baseJson = encode(base);
    const auto *baseData = reinterpret_cast<const std::uint8_t *>(baseJson.constData());
    const auto *loadedData = reinterpret_cast<const std::uint8_t *>(loaded.constData());
    std::size_t required = 0;
    std::uint32_t migrated = 0;
    auto status = static_cast<Status>(aviqtl_settings_merge_json(baseData, static_cast<std::size_t>(baseJson.size()), loadedData, static_cast<std::size_t>(loaded.size()), nullptr, 0, &required, &migrated));
    if (status != Status::BufferTooSmall)
        return std::nullopt;

    std::vector<std::uint8_t> output(required);
    std::size_t written = 0;
    status = static_cast<Status>(aviqtl_settings_merge_json(baseData, static_cast<std::size_t>(baseJson.size()), loadedData, static_cast<std::size_t>(loaded.size()), output.data(), output.size(), &written, &migrated));
    if (status != Status::Ok || written != output.size())
        return std::nullopt;
    const auto settings = decode(output);
    if (!settings.has_value())
        return std::nullopt;
    return MergeResult{*settings, migrated != 0};
}

inline std::optional<QByteArray> persistentJson(const QVariantMap &settings) {
    std::vector<std::uint8_t> output;
    const Status status = transformSingle(encode(settings), output, aviqtl_settings_persistent_json);
    if (status != Status::Ok)
        return std::nullopt;
    return QByteArray(reinterpret_cast<const char *>(output.data()), static_cast<qsizetype>(output.size()));
}

} // namespace AviQtl::RustCore::Settings
