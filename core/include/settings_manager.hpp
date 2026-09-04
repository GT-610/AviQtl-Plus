#pragma once
#include "rust_settings_document.hpp"
#include <QLoggingCategory>
#include <QObject>
#include <QVariantMap>

Q_DECLARE_LOGGING_CATEGORY(lcSettings)

namespace AviQtl::Core {

class SettingsManager : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap settings READ settings WRITE setSettings NOTIFY settingsChanged)

  public:
    static SettingsManager &instance();

    QVariantMap settings() const { return m_settings; }
    void setSettings(const QVariantMap &settings);

    Q_INVOKABLE void load();
    Q_INVOKABLE void save();

    Q_INVOKABLE void setValue(const QString &key, const QVariant &value);
    void removeValue(const QString &key);
    Q_INVOKABLE QVariant value(const QString &key, const QVariant &defaultValue = QVariant()) const;
    Q_INVOKABLE int intValue(const QString &key, int defaultValue = 0) const;
    Q_INVOKABLE double doubleValue(const QString &key, double defaultValue = 0.0) const;
    Q_INVOKABLE bool boolValue(const QString &key, bool defaultValue = false) const;
    Q_INVOKABLE QVariantMap shortcuts() const;
    Q_INVOKABLE QString shortcut(const QString &actionId, const QString &fallbackValue = QString()) const;

  signals:
    void settingsChanged();

  private:
    explicit SettingsManager(QObject *parent = nullptr);
    static QString getSettingsFilePath();
    bool syncProjection();

    AviQtl::RustCore::Settings::State m_state;
    QVariantMap m_settings;
};

} // namespace AviQtl::Core
