#include "effect_registry.hpp"
#include "rust_effect_document.hpp"
#include "shader_compiler.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QLoggingCategory>
#include <QSet>
#include <QUrl>
#include <utility>

Q_LOGGING_CATEGORY(lcEffectRegistry, "aviqtl.effect_registry")

namespace AviQtl::Core {

namespace {
constexpr const char *kMetadataTranslationContext = "AviQtl::Core::EffectRegistry";

auto translatedMetadataString(const QString &source) -> QString {
    if (source.isEmpty()) {
        return source;
    }

    return QCoreApplication::translate(kMetadataTranslationContext, source.toUtf8().constData());
}

auto localizeUiMetadataValue(const QVariant &value) -> QVariant {
    if (value.metaType().id() == QMetaType::QVariantMap) {
        QVariantMap map = value.toMap();
        static const QSet<QString> translatableKeys = {
            QStringLiteral("label"), QStringLiteral("title"), QStringLiteral("text"), QStringLiteral("name"), QStringLiteral("filter"), QStringLiteral("placeholder"), QStringLiteral("unit"),
        };

        for (auto it = map.begin(); it != map.end(); ++it) {
            if (it.key() == QStringLiteral("options") && it.value().metaType().id() == QMetaType::QVariantList) {
                QVariantList options;
                const QVariantList rawOptions = it.value().toList();
                for (const QVariant &option : rawOptions) {
                    if (option.metaType().id() == QMetaType::QString) {
                        const QString rawText = option.toString();
                        QVariantMap displayOption;
                        displayOption.insert(QStringLiteral("value"), rawText);
                        displayOption.insert(QStringLiteral("label"), translatedMetadataString(rawText));
                        options.append(displayOption);
                    } else {
                        options.append(localizeUiMetadataValue(option));
                    }
                }
                it.value() = options;
            } else if (it.value().metaType().id() == QMetaType::QString && translatableKeys.contains(it.key())) {
                it.value() = translatedMetadataString(it.value().toString());
            } else {
                it.value() = localizeUiMetadataValue(it.value());
            }
        }
        return map;
    }

    if (value.metaType().id() == QMetaType::QVariantList) {
        QVariantList list = value.toList();
        for (QVariant &entry : list) {
            entry = localizeUiMetadataValue(entry);
        }
        return list;
    }

    return value;
}

auto stringOrDefault(const QVariantMap &metadata, const QString &key, const QString &fallback) -> QString {
    const QVariant value = metadata.value(key);
    return value.metaType().id() == QMetaType::QString ? value.toString() : fallback;
}

} // namespace

QString filesystemPathIdentity(const QString &path) {
    QFileInfo current(path);
    const QString canonicalPath = current.canonicalFilePath();
    if (!canonicalPath.isEmpty()) {
        return QDir::cleanPath(canonicalPath);
    }

    QStringList missingComponents;
    while (!current.exists() && !current.fileName().isEmpty()) {
        missingComponents.prepend(current.fileName());
        current.setFile(current.absolutePath());
    }

    QString resolvedPath = current.canonicalFilePath();
    if (resolvedPath.isEmpty()) {
        resolvedPath = current.absoluteFilePath();
    }
    for (const QString &component : std::as_const(missingComponents)) {
        resolvedPath = QDir(resolvedPath).filePath(component);
    }
    return QDir::cleanPath(resolvedPath);
}

bool filesystemPathsEqual(const QString &first, const QString &second) {
#ifdef Q_OS_WIN
    constexpr Qt::CaseSensitivity pathCaseSensitivity = Qt::CaseInsensitive;
#else
    constexpr Qt::CaseSensitivity pathCaseSensitivity = Qt::CaseSensitive;
#endif
    return filesystemPathIdentity(first).compare(filesystemPathIdentity(second), pathCaseSensitivity) == 0;
}

void EffectRegistry::loadEffectsFromDirectory(const QString &path, const QString &source) {
    QDir dir(path);
    if (!dir.exists()) {
        return;
    }
    const QString resolvedRootPath = filesystemPathIdentity(path);

    int loadedCount = 0;
    QSet<QString> shaderDirectories;
    QSet<QString> shaderSources;
    qCDebug(lcEffectRegistry).noquote() << "Scanning:" << path;

    // *.json ファイルをサブディレクトリを含めて検索
    QDirIterator it(path, {QStringLiteral("*.json")}, QDir::Files, QDirIterator::Subdirectories);

    while (it.hasNext()) {
        QFile file(it.next());
        if (!file.open(QIODevice::ReadOnly)) {
            continue;
        }

        const auto data = file.readAll();
        const auto normalized = RustCore::Effect::normalizeMetadata(data);
        if (!normalized.has_value()) {
            qWarning().noquote() << QStringLiteral("Skipping invalid effect definition:") << file.fileName();
            continue;
        }
        const QVariantMap &definition = *normalized;
        const QString id = definition.value(QStringLiteral("id")).toString();
        const QString name = definition.value(QStringLiteral("name")).toString();
        const QString qmlFileName = definition.value(QStringLiteral("qml")).toString();
        const QString version = definition.value(QStringLiteral("version")).toString();
        const QString kind = definition.value(QStringLiteral("kind")).toString();
        const QVariantMap params = definition.value(QStringLiteral("params")).toMap();
        const QVariantMap uiDef = definition.value(QStringLiteral("ui")).toMap();
        QStringList categories;
        for (const QVariant &category : definition.value(QStringLiteral("categories")).toList())
            categories.append(category.toString());

        EffectMetadata meta;
        meta.version = version;
        meta.id = id;
        meta.name = translatedMetadataString(name);
        meta.kind = kind;
        for (QString &category : categories) {
            category = translatedMetadataString(category);
        }
        meta.categories = categories;
        meta.defaultParams = params;
        meta.uiDefinition = localizeUiMetadataValue(uiDef).toMap();
        meta.color = definition.value(QStringLiteral("color")).toString();
        meta.packageId = definition.value(QStringLiteral("packageId")).toString();

        // qrc: で始まる場合は絶対パスとしてそのまま使用
        if (qmlFileName.startsWith(QStringLiteral("qrc:"))) {
            meta.qmlSource = qmlFileName;
            meta.source = stringOrDefault(definition, QStringLiteral("source"), source.isEmpty() ? QStringLiteral("built-in") : source);
            meta.sourcePath = filesystemPathIdentity(file.fileName());
            registerEffect(meta);
            loadedCount++;
            continue;
        }

        // QMLファイルの絶対パスを解決 (JSONファイルからの相対パスとして処理)
        QFileInfo jsonInfo(file.fileName());
        QDir jsonDir = jsonInfo.absoluteDir();
        QString absoluteQmlPath = jsonDir.filePath(qmlFileName);

        // Validate path stays within the JSON file's directory (prevent path traversal)
        QFileInfo qmlInfo(absoluteQmlPath);
        QString canonicalQmlPath = qmlInfo.canonicalFilePath();
        QString canonicalBaseDir = jsonDir.canonicalPath();
        if (!canonicalQmlPath.isEmpty() && !canonicalQmlPath.startsWith(canonicalBaseDir + QLatin1Char('/')) && canonicalQmlPath != canonicalBaseDir) {
            qWarning().noquote() << "[EffectRegistry] Path traversal detected in QML reference. Effect:" << id << "Path:" << qmlFileName;
            continue;
        }

        if (QFile::exists(absoluteQmlPath)) {
            meta.qmlSource = QUrl::fromLocalFile(absoluteQmlPath).toString();
            meta.source = stringOrDefault(definition, QStringLiteral("source"), source.isEmpty() ? QStringLiteral("package") : source);
            meta.sourcePath = filesystemPathIdentity(file.fileName());
            if (meta.packageId.isEmpty()) {
                const QString relativePath = QDir(resolvedRootPath).relativeFilePath(filesystemPathIdentity(jsonInfo.absolutePath()));
                meta.packageId = relativePath.section(QLatin1Char('/'), 0, 0);
            }

            shaderDirectories.insert(QDir::cleanPath(jsonDir.absolutePath()));
        } else {
            qWarning().noquote() << "[EffectRegistry] Referenced QML file not found. Effect:" << id << "Path:" << absoluteQmlPath;
            continue;
        }

        registerEffect(meta);
        loadedCount++;
    }

    const QStringList shaderExtensions = {QStringLiteral("*.frag"), QStringLiteral("*.comp"), QStringLiteral("*.vert")};
    for (const QString &shaderDirectory : std::as_const(shaderDirectories)) {
        QDirIterator shaderIt(shaderDirectory, shaderExtensions, QDir::Files, QDirIterator::Subdirectories);
        while (shaderIt.hasNext())
            shaderSources.insert(QDir::cleanPath(shaderIt.next()));
    }
    for (const QString &shaderPath : std::as_const(shaderSources))
        ShaderCompiler::ensureCompiled(shaderPath, QFileInfo(shaderPath).absolutePath());

    qCInfo(lcEffectRegistry).noquote() << dir.dirName() << "→" << loadedCount << " loaded";
}

void EffectRegistry::removeEffectsFromDirectory(const QString &path) {
    const QDir directory(filesystemPathIdentity(path));
    for (auto it = m_orderedIds.begin(); it != m_orderedIds.end();) {
        const auto effectIt = m_effects.constFind(*it);
        const QString relativeSourcePath = effectIt == m_effects.cend() ? QStringLiteral("..") : QDir::cleanPath(directory.relativeFilePath(filesystemPathIdentity(effectIt->sourcePath)));
        if (relativeSourcePath != QStringLiteral("..") && !relativeSourcePath.startsWith(QStringLiteral("../")) && !QDir::isAbsolutePath(relativeSourcePath)) {
            m_effects.remove(*it);
            it = m_orderedIds.erase(it);
        } else {
            ++it;
        }
    }
}

} // namespace AviQtl::Core
