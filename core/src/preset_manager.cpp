#include "preset_manager.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QStandardPaths>

namespace AviQtl::Core {

namespace {
bool isUnsafeName(const QString &s) {
    return s.isEmpty() || s.contains(QLatin1Char('/')) || s.contains(QLatin1Char('\\'))
        || s.contains(QLatin1String("..")) || s.startsWith(QLatin1Char('.'));
}
} // namespace

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
    if (isUnsafeName(effectId))
        return {};
    return QDir(resolveBaseDir()).filePath(effectId);
}

QString PresetManager::presetPath(const QString &effectId, const QString &name) const {
    if (isUnsafeName(effectId) || isUnsafeName(name))
        return {};
    const QString dir = presetDir(effectId);
    if (dir.isEmpty())
        return {};
    const QString path = QDir(dir).filePath(name + QStringLiteral(".json"));
    const QString baseCanon = QFileInfo(resolveBaseDir()).canonicalFilePath();
    const QString fileCanon = QFileInfo(path).canonicalFilePath();
    if (!baseCanon.isEmpty() && !fileCanon.isEmpty() && !fileCanon.startsWith(baseCanon))
        return {};
    return path;
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
    const QString path = presetPath(effectId, name);
    if (path.isEmpty())
        return {};

    QFile f(path);
    if (!f.open(QIODevice::ReadOnly))
        return {};

    const QJsonDocument doc = QJsonDocument::fromJson(f.readAll());
    if (!doc.isObject())
        return {};

    return doc.object().toVariantMap();
}

bool PresetManager::savePreset(const QString &effectId, const QString &name, const QVariantMap &params, const QVariantMap &keyframes, bool enabled) {
    const QString path = presetPath(effectId, name);
    if (path.isEmpty())
        return false;

    QDir().mkpath(presetDir(effectId));

    QJsonObject obj;
    obj[QStringLiteral("version")] = 1;
    obj[QStringLiteral("effectId")] = effectId;
    obj[QStringLiteral("name")] = name;
    obj[QStringLiteral("enabled")] = enabled;
    obj[QStringLiteral("params")] = QJsonObject::fromVariantMap(params);
    obj[QStringLiteral("keyframes")] = QJsonObject::fromVariantMap(keyframes);

    QFile f(path);
    if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate))
        return false;

    f.write(QJsonDocument(obj).toJson(QJsonDocument::Compact));
    emit presetsChanged(effectId);
    return true;
}

bool PresetManager::deletePreset(const QString &effectId, const QString &name) {
    const QString path = presetPath(effectId, name);
    if (path.isEmpty())
        return false;

    QFile f(path);
    if (!f.exists())
        return false;

    const bool ok = f.remove();
    if (ok)
        emit presetsChanged(effectId);
    return ok;
}

bool PresetManager::renamePreset(const QString &effectId, const QString &oldName, const QString &newName) {
    const QString oldPath = presetPath(effectId, oldName);
    const QString newPath = presetPath(effectId, newName);
    if (oldPath.isEmpty() || newPath.isEmpty())
        return false;

    QFile f(oldPath);
    if (!f.exists())
        return false;

    if (!f.rename(newPath))
        return false;

    QVariantMap preset = loadPreset(effectId, newName);
    if (!preset.isEmpty()) {
        preset[QStringLiteral("name")] = newName;
        QFile wf(newPath);
        if (wf.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
            wf.write(QJsonDocument(QJsonObject::fromVariantMap(preset)).toJson(QJsonDocument::Compact));
        }
    }

    emit presetsChanged(effectId);
    return true;
}

} // namespace AviQtl::Core
