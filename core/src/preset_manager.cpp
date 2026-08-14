#include "preset_manager.hpp"
#include "rust_preset_document.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QSaveFile>
#include <QStandardPaths>

namespace AviQtl::Core {

namespace {
bool isUnsafeName(const QString &s) { return !RustCore::Preset::isSafeName(s); }

bool isPathWithin(const QString &basePath, const QString &candidatePath) {
    const QString base = QDir::fromNativeSeparators(QDir::cleanPath(basePath));
    const QString candidate = QDir::fromNativeSeparators(QDir::cleanPath(candidatePath));
    return candidate == base || candidate.startsWith(base + QLatin1Char('/'));
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

    const QString basePath = QDir(resolveBaseDir()).absolutePath();
    const QString candidateDir = QDir(dir).absolutePath();
    if (!isPathWithin(basePath, candidateDir))
        return {};

    const QString canonicalBase = QDir(basePath).canonicalPath();
    const QString canonicalDir = QDir(candidateDir).canonicalPath();
    if (!canonicalBase.isEmpty() && !canonicalDir.isEmpty() && !isPathWithin(canonicalBase, canonicalDir))
        return {};

    const QString path = QDir(dir).filePath(name + QStringLiteral(".json"));
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

    const auto preset = RustCore::Preset::normalize(effectId, name, f.readAll());
    return preset.value_or(QVariantMap{});
}

bool PresetManager::savePreset(const QString &effectId, const QString &name, const QVariantMap &params, const QVariantMap &keyframes, bool enabled) {
    const QString path = presetPath(effectId, name);
    if (path.isEmpty())
        return false;

    QDir().mkpath(presetDir(effectId));

    const auto payload = RustCore::Preset::build(effectId, name, params, keyframes, enabled);
    if (!payload.has_value())
        return false;

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly))
        return false;

    if (file.write(*payload) != payload->size() || !file.commit())
        return false;

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

} // namespace AviQtl::Core
