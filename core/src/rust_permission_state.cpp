#include "rust_permission_state.hpp"
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <limits>
#include <utility>
#include <vector>

namespace AviQtl::RustCore {
namespace {

QByteArray encode(const QVariantMap &permissions) {
    return QJsonDocument(QJsonObject::fromVariantMap(permissions)).toJson(QJsonDocument::Compact);
}

PermissionStateStatus collectSnapshot(const AviQtlPermissionState *handle, QByteArray &output) {
    output.clear();
    std::size_t required = 0;
    auto status = static_cast<PermissionStateStatus>(
        aviqtl_permission_state_snapshot_json(handle, nullptr, 0, &required));
    if (status != PermissionStateStatus::BufferTooSmall || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        return status == PermissionStateStatus::Ok ? PermissionStateStatus::InvalidArgument
                                                   : status;
    }
    output.resize(static_cast<qsizetype>(required));
    std::size_t written = 0;
    status = static_cast<PermissionStateStatus>(aviqtl_permission_state_snapshot_json(
        handle, reinterpret_cast<std::uint8_t *>(output.data()), required, &written));
    if (status != PermissionStateStatus::Ok || written != required) {
        output.clear();
        return status == PermissionStateStatus::Ok ? PermissionStateStatus::InvalidArgument
                                                   : status;
    }
    return status;
}

template <typename Function>
auto withPluginId(const QString &pluginId, Function function) -> decltype(function(nullptr, 0)) {
    const QByteArray utf8 = pluginId.toUtf8();
    return function(reinterpret_cast<const std::uint8_t *>(utf8.constData()),
                    static_cast<std::size_t>(utf8.size()));
}

} // namespace

PermissionState::PermissionState(PermissionState &&other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr)) {}

PermissionState &PermissionState::operator=(PermissionState &&other) noexcept {
    if (this != &other) {
        aviqtl_permission_state_destroy(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

PermissionState::~PermissionState() { aviqtl_permission_state_destroy(m_handle); }

PermissionStateStatus PermissionState::reset(const QVariantMap &permissions) {
    const QByteArray input = encode(permissions);
    const auto *data = reinterpret_cast<const std::uint8_t *>(input.constData());
    const auto length = static_cast<std::size_t>(input.size());
    if (m_handle == nullptr) {
        return static_cast<PermissionStateStatus>(
            aviqtl_permission_state_create(data, length, &m_handle));
    }
    return static_cast<PermissionStateStatus>(
        aviqtl_permission_state_reset(m_handle, data, length));
}

PermissionStateStatus PermissionState::snapshot(QVariantMap &permissions) const {
    permissions.clear();
    if (m_handle == nullptr) {
        return PermissionStateStatus::InvalidArgument;
    }
    QByteArray output;
    const auto status = collectSnapshot(m_handle, output);
    if (status != PermissionStateStatus::Ok) {
        return status;
    }
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isObject()) {
        return PermissionStateStatus::InvalidJson;
    }
    permissions = document.object().toVariantMap();
    return PermissionStateStatus::Ok;
}

bool PermissionState::has(const QString &pluginId, std::int32_t permission) const {
    if (m_handle == nullptr) {
        return false;
    }
    return withPluginId(pluginId, [this, permission](const std::uint8_t *plugin, std::size_t length) {
               return aviqtl_permission_state_has(m_handle, plugin, length, permission);
           }) != 0;
}

PermissionStateStatus PermissionState::grant(const QString &pluginId, std::int32_t permission) {
    if (m_handle == nullptr) {
        return PermissionStateStatus::InvalidArgument;
    }
    return static_cast<PermissionStateStatus>(
        withPluginId(pluginId, [this, permission](const std::uint8_t *plugin, std::size_t length) {
            return aviqtl_permission_state_grant(m_handle, plugin, length, permission);
        }));
}

PermissionStateStatus PermissionState::revoke(const QString &pluginId, std::int32_t permission,
                                              bool &pluginExisted) {
    pluginExisted = false;
    if (m_handle == nullptr) {
        return PermissionStateStatus::InvalidArgument;
    }
    std::uint32_t existed = 0;
    const auto status = static_cast<PermissionStateStatus>(withPluginId(
        pluginId, [this, permission, &existed](const std::uint8_t *plugin, std::size_t length) {
            return aviqtl_permission_state_revoke(m_handle, plugin, length, permission, &existed);
        }));
    if (status == PermissionStateStatus::Ok) {
        pluginExisted = existed != 0;
    }
    return status;
}

PermissionStateStatus PermissionState::grantAll(const QString &pluginId) {
    if (m_handle == nullptr) {
        return PermissionStateStatus::InvalidArgument;
    }
    return static_cast<PermissionStateStatus>(
        withPluginId(pluginId, [this](const std::uint8_t *plugin, std::size_t length) {
            return aviqtl_permission_state_grant_all(m_handle, plugin, length);
        }));
}

PermissionStateStatus PermissionState::revokeAll(const QString &pluginId) {
    if (m_handle == nullptr) {
        return PermissionStateStatus::InvalidArgument;
    }
    return static_cast<PermissionStateStatus>(
        withPluginId(pluginId, [this](const std::uint8_t *plugin, std::size_t length) {
            return aviqtl_permission_state_revoke_all(m_handle, plugin, length);
        }));
}

std::uint64_t PermissionState::mask(const QString &pluginId) const {
    if (m_handle == nullptr) {
        return 0;
    }
    return withPluginId(pluginId, [this](const std::uint8_t *plugin, std::size_t length) {
        return aviqtl_permission_state_mask(m_handle, plugin, length);
    });
}

bool PermissionState::isAuthorized(const QString &pluginId) const {
    if (m_handle == nullptr) {
        return false;
    }
    return withPluginId(pluginId, [this](const std::uint8_t *plugin, std::size_t length) {
               return aviqtl_permission_state_is_authorized(m_handle, plugin, length);
           }) != 0;
}

} // namespace AviQtl::RustCore
