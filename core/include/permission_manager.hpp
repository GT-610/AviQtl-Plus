#pragma once
#include <QObject>
#include <QSet>
#include <QString>
#include <QStringList>
#include <QVariantMap>

namespace AviQtl::Core {

enum class PluginPermission {
    TransportControl,  // Play, pause, seek
    ClipRead,          // List and read clip information
    ClipModify,        // Create, delete, update clips
    EffectRead,        // List effects
    EffectModify,      // Add, remove, modify effects
    ProjectRead,       // Read project info (resolution, fps)
    ProjectSave,       // Save project files
    ProjectLoad,       // Load project files
    SceneManage,       // Create, remove, switch scenes
    SettingsRead,      // Read plugin settings
    SettingsWrite,     // Write plugin settings
    ClipboardAccess,   // Copy, cut, paste operations
    LogOutput,         // Write to console log
};

class PermissionManager : public QObject {
    Q_OBJECT
  public:
    static PermissionManager &instance();

    // Permission checking
    Q_INVOKABLE bool hasPermission(const QString &pluginId, PluginPermission permission) const;
    Q_INVOKABLE bool hasPermission(const QString &pluginId, const QString &permissionName) const;

    // Permission granting/revoking
    void grantPermission(const QString &pluginId, PluginPermission permission);
    Q_INVOKABLE void grantPermission(const QString &pluginId, const QString &permissionName);
    void revokePermission(const QString &pluginId, PluginPermission permission);
    Q_INVOKABLE void revokePermission(const QString &pluginId, const QString &permissionName);
    Q_INVOKABLE void grantAllPermissions(const QString &pluginId);
    Q_INVOKABLE void revokeAllPermissions(const QString &pluginId);

    // Bulk operations
    void setPluginPermissions(const QString &pluginId, const QSet<PluginPermission> &permissions);
    Q_INVOKABLE QVariantList getPluginPermissions(const QString &pluginId) const;

    // Permission name conversion
    static QString permissionName(PluginPermission permission);
    static PluginPermission permissionFromName(const QString &name);
    static QStringList allPermissionNames();
    static QString permissionDescription(PluginPermission permission);

    // Persistence
    void loadPermissions();
    void savePermissions();

    // Check if plugin has any permissions granted
    bool isPluginAuthorized(const QString &pluginId) const;

  signals:
    void permissionsChanged(const QString &pluginId);

  private:
    explicit PermissionManager(QObject *parent = nullptr);

    // Map: pluginId -> set of granted permissions
    QMap<QString, QSet<PluginPermission>> m_permissions;
};

} // namespace AviQtl::Core
