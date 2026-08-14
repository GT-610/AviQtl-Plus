#include "permission_manager.hpp"
#include "rust_core_policy.hpp"
#include "settings_manager.hpp"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

namespace AviQtl::Core {

PermissionManager &PermissionManager::instance() {
    static PermissionManager inst;
    return inst;
}

PermissionManager::PermissionManager(QObject *parent) : QObject(parent) {
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
    const int permission = RustCore::Policy::permissionFromName(permissionName);
    if (permission < 0) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return false;
    }
    return hasPermission(pluginId, static_cast<PluginPermission>(permission));
}

bool PermissionManager::hasApiPermission(const QString &pluginId, const char *apiName) const {
    const int permission = RustCore::Policy::permissionForApi(apiName);
    if (permission >= 0)
        return hasPermission(pluginId, static_cast<PluginPermission>(permission));
    qWarning() << "[PermissionManager] Unknown API name:" << (apiName == nullptr ? "<null>" : apiName);
    return false;
}

void PermissionManager::grantPermission(const QString &pluginId, PluginPermission permission) {
    m_permissions[pluginId].insert(permission);
    savePermissions();
    emit permissionsChanged(pluginId);
}

void PermissionManager::grantPermission(const QString &pluginId, const QString &permissionName) {
    const int permission = RustCore::Policy::permissionFromName(permissionName);
    if (permission < 0) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return;
    }
    grantPermission(pluginId, static_cast<PluginPermission>(permission));
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
    const int permission = RustCore::Policy::permissionFromName(permissionName);
    if (permission < 0) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return;
    }
    revokePermission(pluginId, static_cast<PluginPermission>(permission));
}

void PermissionManager::grantAllPermissions(const QString &pluginId) {
    QSet<PluginPermission> allPerms;
    for (int permission = 0; permission < RustCore::Policy::permissionCount(); ++permission)
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

QString PermissionManager::permissionName(PluginPermission permission) {
    const QString name = RustCore::Policy::permissionName(static_cast<int>(permission));
    return name.isEmpty() ? QStringLiteral("unknown") : name;
}

PluginPermission PermissionManager::permissionFromName(const QString &name) {
    return static_cast<PluginPermission>(RustCore::Policy::permissionFromName(name));
}

QStringList PermissionManager::allPermissionNames() {
    return RustCore::Policy::allPermissionNames();
}

QString PermissionManager::permissionDescription(PluginPermission permission) {
    switch (permission) {
    case PluginPermission::TransportControl:
        return QObject::tr("再生、一時停止、シークなどの再生制御");
    case PluginPermission::ClipRead:
        return QObject::tr("クリップ情報の一覧表示と読み取り");
    case PluginPermission::ClipModify:
        return QObject::tr("クリップの作成、削除、移動、変更");
    case PluginPermission::EffectModify:
        return QObject::tr("エフェクトの追加、削除、パラメータ変更");
    case PluginPermission::ProjectRead:
        return QObject::tr("プロジェクト情報（解像度、FPS等）の読み取り");
    case PluginPermission::ProjectSave:
        return QObject::tr("プロジェクトファイルの保存");
    case PluginPermission::ProjectLoad:
        return QObject::tr("プロジェクトファイルの読み込み");
    case PluginPermission::SceneManage:
        return QObject::tr("シーンの作成、削除、切り替え");
    case PluginPermission::SettingsRead:
        return QObject::tr("プラグイン設定の読み取り");
    case PluginPermission::SettingsWrite:
        return QObject::tr("プラグイン設定の書き込み");
    case PluginPermission::ClipboardAccess:
        return QObject::tr("クリップボードへのコピー、切り取り、貼り付け");
    case PluginPermission::HistoryControl:
        return QObject::tr("元に戻す、やり直し、コマンドのグループ化");
    case PluginPermission::LogOutput:
        return QObject::tr("コンソールへのログ出力");
    }
    return QObject::tr("不明な権限");
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
            if (static_cast<int>(permission) >= 0)
                perms.insert(permission);
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
