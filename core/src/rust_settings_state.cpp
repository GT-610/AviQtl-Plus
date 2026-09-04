#include "rust_settings_document.hpp"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <limits>
#include <optional>
#include <utility>

namespace AviQtl::RustCore::Settings {
namespace {

QByteArray encode(const QVariantMap &value) {
    return QJsonDocument(QJsonObject::fromVariantMap(value)).toJson(QJsonDocument::Compact);
}

std::optional<QVariantMap> decode(const QByteArray &value) {
    const QJsonDocument document = QJsonDocument::fromJson(value);
    if (!document.isObject()) {
        return std::nullopt;
    }
    return document.object().toVariantMap();
}

template <typename Function>
auto withUtf8(const QString &value, Function function) -> decltype(function(nullptr, 0)) {
    const QByteArray utf8 = value.toUtf8();
    return function(reinterpret_cast<const std::uint8_t *>(utf8.constData()),
                    static_cast<std::size_t>(utf8.size()));
}

template <typename Function> Status collectJson(Function function, QByteArray &output) {
    output.clear();
    std::size_t required = 0;
    auto status = static_cast<Status>(function(nullptr, 0, &required));
    if (status != Status::BufferTooSmall || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        return status == Status::Ok ? Status::InvalidArgument : status;
    }
    output.resize(static_cast<qsizetype>(required));
    std::size_t written = 0;
    status = static_cast<Status>(function(reinterpret_cast<std::uint8_t *>(output.data()),
                                          required, &written));
    if (status != Status::Ok || written != required) {
        output.clear();
        return status == Status::Ok ? Status::InvalidArgument : status;
    }
    return status;
}

} // namespace

State::State(State &&other) noexcept : m_handle(std::exchange(other.m_handle, nullptr)) {}

State &State::operator=(State &&other) noexcept {
    if (this != &other) {
        aviqtl_settings_state_destroy(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

State::~State() { aviqtl_settings_state_destroy(m_handle); }

Status State::initializeDefaults(const QVariantMap &platformDefaults) {
    const QByteArray input = encode(platformDefaults);
    AviQtlSettingsState *replacement = nullptr;
    const auto status = static_cast<Status>(aviqtl_settings_state_create_defaults(
        reinterpret_cast<const std::uint8_t *>(input.constData()),
        static_cast<std::size_t>(input.size()), &replacement));
    if (status == Status::Ok) {
        aviqtl_settings_state_destroy(m_handle);
        m_handle = replacement;
    }
    return status;
}

Status State::reset(const QVariantMap &settings) {
    const QByteArray input = encode(settings);
    const auto *data = reinterpret_cast<const std::uint8_t *>(input.constData());
    const auto length = static_cast<std::size_t>(input.size());
    if (m_handle == nullptr) {
        return static_cast<Status>(aviqtl_settings_state_create(data, length, &m_handle));
    }
    return static_cast<Status>(aviqtl_settings_state_reset(m_handle, data, length));
}

Status State::mergeLoaded(const QByteArray &loaded, bool &migrated) {
    migrated = false;
    if (m_handle == nullptr) {
        return Status::InvalidArgument;
    }
    std::uint32_t migratedValue = 0;
    const auto status = static_cast<Status>(aviqtl_settings_state_merge_json(
        m_handle, reinterpret_cast<const std::uint8_t *>(loaded.constData()),
        static_cast<std::size_t>(loaded.size()), &migratedValue));
    if (status == Status::Ok) {
        migrated = migratedValue != 0;
    }
    return status;
}

Status State::snapshot(QVariantMap &settings) const {
    settings.clear();
    if (m_handle == nullptr) {
        return Status::InvalidArgument;
    }
    QByteArray output;
    const auto status = collectJson(
        [this](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
            return aviqtl_settings_state_snapshot_json(m_handle, data, capacity, length);
        },
        output);
    if (status != Status::Ok) {
        return status;
    }
    const auto decoded = decode(output);
    if (!decoded.has_value()) {
        return Status::InvalidJson;
    }
    settings = *decoded;
    return Status::Ok;
}

Status State::persistentJson(QByteArray &output) const {
    if (m_handle == nullptr) {
        output.clear();
        return Status::InvalidArgument;
    }
    return collectJson(
        [this](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
            return aviqtl_settings_state_persistent_json(m_handle, data, capacity, length);
        },
        output);
}

Status State::setValue(const QString &key, const QVariant &value, bool &changed, bool &persistent) {
    changed = false;
    persistent = false;
    if (m_handle == nullptr) {
        return Status::InvalidArgument;
    }
    const QByteArray valueDocument =
        QJsonDocument(QJsonArray{QJsonValue::fromVariant(value)}).toJson(QJsonDocument::Compact);
    std::uint32_t changedValue = 0;
    std::uint32_t persistentValue = 0;
    const auto status = static_cast<Status>(withUtf8(
        key, [this, &valueDocument, &changedValue, &persistentValue](const std::uint8_t *data,
                                                                   std::size_t length) {
            return aviqtl_settings_state_set_value_json(
                m_handle, data, length,
                reinterpret_cast<const std::uint8_t *>(valueDocument.constData()),
                static_cast<std::size_t>(valueDocument.size()), &changedValue, &persistentValue);
        }));
    if (status == Status::Ok) {
        changed = changedValue != 0;
        persistent = persistentValue != 0;
    }
    return status;
}

Status State::removeValue(const QString &key, bool &changed, bool &persistent) {
    changed = false;
    persistent = false;
    if (m_handle == nullptr) {
        return Status::InvalidArgument;
    }
    std::uint32_t changedValue = 0;
    std::uint32_t persistentValue = 0;
    const auto status = static_cast<Status>(withUtf8(
        key, [this, &changedValue, &persistentValue](const std::uint8_t *data,
                                                    std::size_t length) {
            return aviqtl_settings_state_remove_value(m_handle, data, length, &changedValue,
                                                      &persistentValue);
        }));
    if (status == Status::Ok) {
        changed = changedValue != 0;
        persistent = persistentValue != 0;
    }
    return status;
}

int State::intValue(const QString &key, int fallback) const {
    if (m_handle == nullptr) {
        return fallback;
    }
    return withUtf8(key, [this, fallback](const std::uint8_t *data, std::size_t length) {
        return aviqtl_settings_state_get_i32(m_handle, data, length, fallback);
    });
}

double State::doubleValue(const QString &key, double fallback) const {
    if (m_handle == nullptr) {
        return fallback;
    }
    return withUtf8(key, [this, fallback](const std::uint8_t *data, std::size_t length) {
        return aviqtl_settings_state_get_f64(m_handle, data, length, fallback);
    });
}

bool State::boolValue(const QString &key, bool fallback) const {
    if (m_handle == nullptr) {
        return fallback;
    }
    return withUtf8(key, [this, fallback](const std::uint8_t *data, std::size_t length) {
               return aviqtl_settings_state_get_bool(m_handle, data, length, fallback ? 1U : 0U);
           }) != 0;
}

} // namespace AviQtl::RustCore::Settings
