#pragma once
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantMap>

namespace AviQtl::Core {

class PresetManager : public QObject {
    Q_OBJECT

public:
    static PresetManager &instance();

    Q_INVOKABLE QStringList presetNames(const QString &effectId) const;
    Q_INVOKABLE QVariantMap loadPreset(const QString &effectId, const QString &name) const;
    Q_INVOKABLE bool savePreset(const QString &effectId, const QString &name, const QVariantMap &params, const QVariantMap &keyframes, bool enabled);
    Q_INVOKABLE bool deletePreset(const QString &effectId, const QString &name);

signals:
    void presetsChanged(const QString &effectId);

private:
    explicit PresetManager(QObject *parent = nullptr);
    QString presetDir(const QString &effectId) const;
    QString presetPath(const QString &effectId, const QString &name) const;
    QString resolveBaseDir() const;
};

} // namespace AviQtl::Core
