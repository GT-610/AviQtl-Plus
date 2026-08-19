#pragma once
#include <QObject>
#include <QSet>
#include <QString>
#include <QStringList>
#include <QVariantMap>
#include <optional>

namespace AviQtl::Core {

enum class PluginPermission : int {
    TransportControl = 0, // Play, pause, seek
    ClipRead = 1,         // List and read clip information
    ClipModify = 2,       // Create, delete, update clips
    EffectModify = 3,     // Add, remove, modify effects
    ProjectRead = 4,      // Read project info (resolution, fps)
    ProjectSave = 5,      // Save project files
    ProjectLoad = 6,      // Load project files
    SceneManage = 7,      // Create, remove, switch scenes
    SettingsRead = 8,     // Read plugin settings
    SettingsWrite = 9,    // Write plugin settings
    ClipboardAccess = 10, // Copy, cut, paste operations
    HistoryControl = 11,  // Undo, redo, and command grouping
    LogOutput = 12,       // Write to console log
    Count = 13,
};

class PermissionManager : public QObject {
    Q_OBJECT
  public:
    static PermissionManager &instance();

    // Permission checking
    Q_INVOKABLE bool hasPermission(const QString &pluginId, PluginPermission permission) const;
    Q_INVOKABLE bool hasPermission(const QString &pluginId, const QString &permissionName) const;
    bool hasApiPermission(const QString &pluginId, const char *apiName) const;

    // Permission granting/revoking
    void grantPermission(const QString &pluginId, PluginPermission permission);
    Q_INVOKABLE void grantPermission(const QString &pluginId, const QString &permissionName);
    void revokePermission(const QString &pluginId, PluginPermission permission);
    Q_INVOKABLE void revokePermission(const QString &pluginId, const QString &permissionName);
    Q_INVOKABLE void grantAllPermissions(const QString &pluginId);
    Q_INVOKABLE void revokeAllPermissions(const QString &pluginId);

    // Bulk operations
    Q_INVOKABLE QVariantList getPluginPermissions(const QString &pluginId) const;
    Q_INVOKABLE QStringList getAllPermissionNames() const;

    // Permission name conversion
    static QString permissionName(PluginPermission permission);
    static std::optional<PluginPermission> permissionFromName(const QString &name);
    static QStringList allPermissionNames();

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
