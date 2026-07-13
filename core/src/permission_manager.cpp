#include "permission_manager.hpp"
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
    if (!allPermissionNames().contains(permissionName)) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return false;
    }
    return hasPermission(pluginId, permissionFromName(permissionName));
}

void PermissionManager::grantPermission(const QString &pluginId, PluginPermission permission) {
    m_permissions[pluginId].insert(permission);
    savePermissions();
    emit permissionsChanged(pluginId);
}

void PermissionManager::grantPermission(const QString &pluginId, const QString &permissionName) {
    if (!allPermissionNames().contains(permissionName)) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return;
    }
    grantPermission(pluginId, permissionFromName(permissionName));
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
    if (!allPermissionNames().contains(permissionName)) {
        qWarning() << "[PermissionManager] Unknown permission name:" << permissionName;
        return;
    }
    revokePermission(pluginId, permissionFromName(permissionName));
}

void PermissionManager::grantAllPermissions(const QString &pluginId) {
    QSet<PluginPermission> allPerms;
    allPerms << PluginPermission::TransportControl
             << PluginPermission::ClipRead
             << PluginPermission::ClipModify
             << PluginPermission::EffectRead
             << PluginPermission::EffectModify
             << PluginPermission::ProjectRead
             << PluginPermission::ProjectSave
             << PluginPermission::ProjectLoad
             << PluginPermission::SceneManage
             << PluginPermission::SettingsRead
             << PluginPermission::SettingsWrite
             << PluginPermission::ClipboardAccess
             << PluginPermission::LogOutput;
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
    switch (permission) {
    case PluginPermission::TransportControl:
        return QStringLiteral("transport.control");
    case PluginPermission::ClipRead:
        return QStringLiteral("clip.read");
    case PluginPermission::ClipModify:
        return QStringLiteral("clip.modify");
    case PluginPermission::EffectRead:
        return QStringLiteral("effect.read");
    case PluginPermission::EffectModify:
        return QStringLiteral("effect.modify");
    case PluginPermission::ProjectRead:
        return QStringLiteral("project.read");
    case PluginPermission::ProjectSave:
        return QStringLiteral("project.save");
    case PluginPermission::ProjectLoad:
        return QStringLiteral("project.load");
    case PluginPermission::SceneManage:
        return QStringLiteral("scene.manage");
    case PluginPermission::SettingsRead:
        return QStringLiteral("settings.read");
    case PluginPermission::SettingsWrite:
        return QStringLiteral("settings.write");
    case PluginPermission::ClipboardAccess:
        return QStringLiteral("clipboard.access");
    case PluginPermission::LogOutput:
        return QStringLiteral("log.output");
    }
    return QStringLiteral("unknown");
}

PluginPermission PermissionManager::permissionFromName(const QString &name) {
    if (name == QStringLiteral("transport.control"))
        return PluginPermission::TransportControl;
    if (name == QStringLiteral("clip.read"))
        return PluginPermission::ClipRead;
    if (name == QStringLiteral("clip.modify"))
        return PluginPermission::ClipModify;
    if (name == QStringLiteral("effect.read"))
        return PluginPermission::EffectRead;
    if (name == QStringLiteral("effect.modify"))
        return PluginPermission::EffectModify;
    if (name == QStringLiteral("project.read"))
        return PluginPermission::ProjectRead;
    if (name == QStringLiteral("project.save"))
        return PluginPermission::ProjectSave;
    if (name == QStringLiteral("project.load"))
        return PluginPermission::ProjectLoad;
    if (name == QStringLiteral("scene.manage"))
        return PluginPermission::SceneManage;
    if (name == QStringLiteral("settings.read"))
        return PluginPermission::SettingsRead;
    if (name == QStringLiteral("settings.write"))
        return PluginPermission::SettingsWrite;
    if (name == QStringLiteral("clipboard.access"))
        return PluginPermission::ClipboardAccess;
    if (name == QStringLiteral("log.output"))
        return PluginPermission::LogOutput;
    // Return a clearly invalid value instead of silently defaulting to LogOutput.
    // Callers must check via allPermissionNames() before using this function.
    return static_cast<PluginPermission>(-1);
}

QStringList PermissionManager::allPermissionNames() {
    return {
        QStringLiteral("transport.control"),
        QStringLiteral("clip.read"),
        QStringLiteral("clip.modify"),
        QStringLiteral("effect.read"),
        QStringLiteral("effect.modify"),
        QStringLiteral("project.read"),
        QStringLiteral("project.save"),
        QStringLiteral("project.load"),
        QStringLiteral("scene.manage"),
        QStringLiteral("settings.read"),
        QStringLiteral("settings.write"),
        QStringLiteral("clipboard.access"),
        QStringLiteral("log.output")
    };
}

QString PermissionManager::permissionDescription(PluginPermission permission) {
    switch (permission) {
    case PluginPermission::TransportControl:
        return QObject::tr("再生、一時停止、シークなどの再生制御");
    case PluginPermission::ClipRead:
        return QObject::tr("クリップ情報の一覧表示と読み取り");
    case PluginPermission::ClipModify:
        return QObject::tr("クリップの作成、削除、移動、変更");
    case PluginPermission::EffectRead:
        return QObject::tr("エフェクト情報の一覧表示");
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
            if (allPermissionNames().contains(name)) {
                perms.insert(permissionFromName(name));
            }
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
