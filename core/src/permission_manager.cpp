#include "permission_manager.hpp"
#include "rust_core_policy.hpp"
#include "settings_manager.hpp"

namespace AviQtl::Core {

namespace {
constexpr int kPermissionCount = static_cast<int>(PluginPermission::Count);

std::optional<PluginPermission> checkedPermission(int permission) {
    if (permission < 0 || permission >= kPermissionCount) {
        return std::nullopt;
    }
    return static_cast<PluginPermission>(permission);
}
} // namespace

PermissionManager &PermissionManager::instance() {
    static PermissionManager inst;
    return inst;
}

PermissionManager::PermissionManager(QObject *parent) : QObject(parent) {
    if (RustCore::Policy::permissionCount() != kPermissionCount) {
        qCritical() << "Rust/C++ permission table size mismatch:" << RustCore::Policy::permissionCount()
                    << "!=" << kPermissionCount;
    }
    loadPermissions();
}

bool PermissionManager::hasPermission(const QString &pluginId, PluginPermission permission) const {
    return m_state.has(pluginId, static_cast<int>(permission));
}

bool PermissionManager::hasPermission(const QString &pluginId, const QString &permissionName) const {
    const auto permission = permissionFromName(permissionName);
    if (!permission.has_value()) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return false;
    }
    return hasPermission(pluginId, *permission);
}

bool PermissionManager::hasApiPermission(const QString &pluginId, const char *apiName) const {
    const auto permission = checkedPermission(RustCore::Policy::permissionForApi(apiName));
    if (permission.has_value())
        return hasPermission(pluginId, *permission);
    qWarning() << "[PermissionManager] Unknown API name:" << (apiName == nullptr ? "<null>" : apiName);
    return false;
}

void PermissionManager::grantPermission(const QString &pluginId, PluginPermission permission) {
    if (m_state.grant(pluginId, static_cast<int>(permission)) !=
        RustCore::PermissionStateStatus::Ok) {
        qWarning() << "[PermissionManager] Failed to grant permission for plugin:" << pluginId;
        return;
    }
    savePermissions();
    emit permissionsChanged(pluginId);
}

void PermissionManager::grantPermission(const QString &pluginId, const QString &permissionName) {
    const auto permission = permissionFromName(permissionName);
    if (!permission.has_value()) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return;
    }
    grantPermission(pluginId, *permission);
}

void PermissionManager::revokePermission(const QString &pluginId, PluginPermission permission) {
    bool pluginExisted = false;
    const auto status = m_state.revoke(pluginId, static_cast<int>(permission), pluginExisted);
    if (status != RustCore::PermissionStateStatus::Ok) {
        qWarning() << "[PermissionManager] Failed to revoke permission for plugin:" << pluginId;
        return;
    }
    if (pluginExisted) {
        savePermissions();
        emit permissionsChanged(pluginId);
    }
}

void PermissionManager::revokePermission(const QString &pluginId, const QString &permissionName) {
    const auto permission = permissionFromName(permissionName);
    if (!permission.has_value()) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return;
    }
    revokePermission(pluginId, *permission);
}

void PermissionManager::grantAllPermissions(const QString &pluginId) {
    if (m_state.grantAll(pluginId) != RustCore::PermissionStateStatus::Ok) {
        qWarning() << "[PermissionManager] Failed to grant all permissions for plugin:"
                   << pluginId;
        return;
    }
    savePermissions();
    emit permissionsChanged(pluginId);
}

void PermissionManager::revokeAllPermissions(const QString &pluginId) {
    if (m_state.revokeAll(pluginId) != RustCore::PermissionStateStatus::Ok) {
        qWarning() << "[PermissionManager] Failed to revoke all permissions for plugin:"
                   << pluginId;
        return;
    }
    savePermissions();
    emit permissionsChanged(pluginId);
}

QVariantList PermissionManager::getPluginPermissions(const QString &pluginId) const {
    QVariantList result;
    const std::uint64_t mask = m_state.mask(pluginId);
    for (int permission = 0; permission < kPermissionCount; ++permission) {
        if ((mask & (std::uint64_t{1} << permission)) != 0)
            result.append(permissionName(static_cast<PluginPermission>(permission)));
    }
    return result;
}

QStringList PermissionManager::getAllPermissionNames() const {
    return allPermissionNames();
}

QString PermissionManager::permissionName(PluginPermission permission) {
    const QString name = RustCore::Policy::permissionName(static_cast<int>(permission));
    return name.isEmpty() ? QStringLiteral("unknown") : name;
}

std::optional<PluginPermission> PermissionManager::permissionFromName(const QString &name) {
    return checkedPermission(RustCore::Policy::permissionFromName(name));
}

QStringList PermissionManager::allPermissionNames() {
    return RustCore::Policy::allPermissionNames();
}

void PermissionManager::loadPermissions() {
    auto &sm = SettingsManager::instance();
    const QVariantMap permissionData =
        sm.value(QStringLiteral("pluginPermissions")).toMap();
    if (m_state.reset(permissionData) != RustCore::PermissionStateStatus::Ok) {
        qCritical() << "[PermissionManager] Rust core failed to load permission state";
    }
}

void PermissionManager::savePermissions() {
    QVariantMap permData;
    if (m_state.snapshot(permData) != RustCore::PermissionStateStatus::Ok) {
        qWarning() << "[PermissionManager] Rust core failed to serialize permission state";
        return;
    }

    auto &sm = SettingsManager::instance();
    sm.setValue(QStringLiteral("pluginPermissions"), permData);
}

bool PermissionManager::isPluginAuthorized(const QString &pluginId) const {
    return m_state.isAuthorized(pluginId);
}

} // namespace AviQtl::Core
