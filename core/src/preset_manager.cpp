#include "preset_manager.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QStandardPaths>

namespace AviQtl::Core {

PresetManager &PresetManager::instance() {
    static PresetManager s;
    return s;
}

PresetManager::PresetManager(QObject *parent) : QObject(parent) {}

QString PresetManager::resolveBaseDir() const {
    const QString portable = QCoreApplication::applicationDirPath() + QStringLiteral("/presets");
    if (QDir(portable).exists() || QFileInfo(QCoreApplication::applicationDirPath()).isWritable()) {
        QDir().mkpath(portable);
        return portable;
    }
    return QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation) + QStringLiteral("/presets");
}

QString PresetManager::presetDir(const QString &effectId) const {
    return resolveBaseDir() + QStringLiteral("/") + effectId;
}

QString PresetManager::presetPath(const QString &effectId, const QString &name) const {
    return presetDir(effectId) + QStringLiteral("/") + name + QStringLiteral(".json");
}

QStringList PresetManager::presetNames(const QString &effectId) const {
    QDir dir(presetDir(effectId));
    if (!dir.exists())
        return {};

    QStringList names;
    for (const auto &entry : dir.entryList({QStringLiteral("*.json")}, QDir::Files, QDir::Name)) {
        names << entry.chopped(5); // remove ".json"
    }
    return names;
}

QVariantMap PresetManager::loadPreset(const QString &effectId, const QString &name) const {
    QFile f(presetPath(effectId, name));
    if (!f.open(QIODevice::ReadOnly))
        return {};

    const QJsonDocument doc = QJsonDocument::fromJson(f.readAll());
    if (!doc.isObject())
        return {};

    return doc.object().toVariantMap();
}

bool PresetManager::savePreset(const QString &effectId, const QString &name, const QVariantMap &params, const QVariantMap &keyframes, bool enabled) {
    const QString dir = presetDir(effectId);
    QDir().mkpath(dir);

    QJsonObject obj;
    obj[QStringLiteral("version")] = 1;
    obj[QStringLiteral("effectId")] = effectId;
    obj[QStringLiteral("name")] = name;
    obj[QStringLiteral("enabled")] = enabled;
    obj[QStringLiteral("params")] = QJsonObject::fromVariantMap(params);
    obj[QStringLiteral("keyframes")] = QJsonObject::fromVariantMap(keyframes);

    QFile f(presetPath(effectId, name));
    if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return false;

    f.write(QJsonDocument(obj).toJson(QJsonDocument::Compact));
    emit presetsChanged(effectId);
    return true;
}

bool PresetManager::deletePreset(const QString &effectId, const QString &name) {
    QFile f(presetPath(effectId, name));
    if (!f.exists())
        return false;

    const bool ok = f.remove();
    if (ok)
        emit presetsChanged(effectId);
    return ok;
}

bool PresetManager::renamePreset(const QString &effectId, const QString &oldName, const QString &newName) {
    QFile f(presetPath(effectId, oldName));
    if (!f.exists())
        return false;

    const bool ok = f.rename(presetPath(effectId, newName));
    if (ok)
        emit presetsChanged(effectId);
    return ok;
}

} // namespace AviQtl::Core
