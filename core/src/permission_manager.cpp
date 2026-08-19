#include "permission_manager.hpp"
#include "rust_core_policy.hpp"
#include "settings_manager.hpp"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

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
    auto it = m_permissions.constFind(pluginId);
    if (it == m_permissions.constEnd()) {
        return false;
    }
    return it->contains(permission);
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
    m_permissions[pluginId].insert(permission);
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
    auto it = m_permissions.find(pluginId);
    if (it != m_permissions.end()) {
        it->remove(permission);
        if (it->isEmpty()) {
            m_permissions.erase(it);
        }
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
    QSet<PluginPermission> allPerms;
    for (int permission = 0; permission < kPermissionCount; ++permission)
        allPerms.insert(static_cast<PluginPermission>(permission));
    m_permissions[pluginId] = allPerms;
    savePermissions();
    emit permissionsChanged(pluginId);
}

void PermissionManager::revokeAllPermissions(const QString &pluginId) {
    m_permissions.remove(pluginId);
    savePermissions();
    emit permissionsChanged(pluginId);
}

QVariantList PermissionManager::getPluginPermissions(const QString &pluginId) const {
    QVariantList result;
    auto it = m_permissions.constFind(pluginId);
    if (it != m_permissions.constEnd()) {
        for (PluginPermission p : *it) {
            result.append(permissionName(p));
        }
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
    m_permissions.clear();

    auto &sm = SettingsManager::instance();
    QVariantMap permData = sm.value(QStringLiteral("pluginPermissions")).toMap();

    for (auto it = permData.constBegin(); it != permData.constEnd(); ++it) {
        const QString pluginId = it.key();
        const QStringList permNames = it.value().toStringList();
        QSet<PluginPermission> perms;
        for (const QString &name : permNames) {
            const auto permission = permissionFromName(name);
            if (permission.has_value())
                perms.insert(*permission);
        }
        m_permissions[pluginId] = perms;
    }
}

void PermissionManager::savePermissions() {
    QVariantMap permData;
    for (auto it = m_permissions.constBegin(); it != m_permissions.constEnd(); ++it) {
        const QString pluginId = it.key();
        const QSet<PluginPermission> &perms = it.value();
        QStringList permNames;
        for (PluginPermission p : perms) {
            permNames.append(permissionName(p));
        }
        permData[pluginId] = permNames;
    }

    auto &sm = SettingsManager::instance();
    sm.setValue(QStringLiteral("pluginPermissions"), permData);
    sm.save();
}

bool PermissionManager::isPluginAuthorized(const QString &pluginId) const {
    return m_permissions.contains(pluginId) && !m_permissions[pluginId].isEmpty();
}

} // namespace AviQtl::Core
