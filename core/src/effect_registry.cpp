#include "effect_registry.hpp"
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>
#include <QSet>
#include <QUrl>

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
} // namespace

void EffectRegistry::loadEffectsFromDirectory(const QString &path) {
    QDir dir(path);
    if (!dir.exists()) {
        return;
    }

    int loadedCount = 0;
    qDebug().noquote() << "[EffectRegistry] Scanning:" << path;

    // *.json ファイルをサブディレクトリを含めて検索
    QDirIterator it(path, {QStringLiteral("*.json")}, QDir::Files, QDirIterator::Subdirectories);

    while (it.hasNext()) {
        QFile file(it.next());
        if (!file.open(QIODevice::ReadOnly)) {
            continue;
        }

        QJsonParseError error;
        const auto data = file.readAll();
        const auto doc = QJsonDocument::fromJson(data, &error);

        if (error.error != QJsonParseError::NoError || !doc.isObject()) {
            qWarning().noquote() << QStringLiteral("Invalid JSON in") << file.fileName() << QStringLiteral(":") << error.errorString();
            continue;
        }

        QJsonObject obj = doc.object();
        QString id = obj.value(QStringLiteral("id")).toString();
        QString name = obj.value(QStringLiteral("name")).toString();
        QString qmlFileName = obj.value(QStringLiteral("qml")).toString();
        QVariantMap params = obj.value(QStringLiteral("params")).toObject().toVariantMap();
        QVariantMap uiDef = obj.value(QStringLiteral("ui")).toObject().toVariantMap();

        if (!uiDef.contains(QStringLiteral("controls"))) {
            qWarning().noquote() << QStringLiteral("Skipping invalid definition (ui.controls missing):") << file.fileName();
            continue;
        }

        QString version = obj.value(QStringLiteral("version")).toString();
        QRegularExpression versionRegex(QStringLiteral("^\\d+\\.\\d+\\.\\d+$"));
        if (!versionRegex.match(version).hasMatch()) {
            qWarning().noquote() << QStringLiteral("Version format is invalid or missing (x.x.x required):") << file.fileName();
            continue;
        }

        QString kind = obj.value(QStringLiteral("kind")).toString();
        if (kind != QStringLiteral("effect") && kind != QStringLiteral("object")) {
            qWarning().noquote() << QStringLiteral("Invalid kind ('effect' or 'object' required):") << file.fileName();
            continue;
        }

        QStringList categories;
        QJsonArray catArray = obj.value(QStringLiteral("categories")).toArray();
        for (const auto &val : catArray) {
            if (val.isString()) {
                categories.append(val.toString());
            }
        }
        if (categories.isEmpty()) {
            qWarning().noquote() << QStringLiteral("Categories is empty or missing (at least one category required):") << file.fileName();
            continue;
        }

        if (id.isEmpty() || name.isEmpty() || qmlFileName.isEmpty()) {
            qWarning().noquote() << QStringLiteral("Skipping incomplete definition:") << file.fileName();
            continue;
        }

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
        meta.color = obj.value(QStringLiteral("color")).toString();

        // qrc: で始まる場合は絶対パスとしてそのまま使用
        if (qmlFileName.startsWith(QStringLiteral("qrc:"))) {
            meta.qmlSource = qmlFileName;
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
        if (!canonicalQmlPath.startsWith(canonicalBaseDir + QLatin1Char('/')) &&
            canonicalQmlPath != canonicalBaseDir) {
            qWarning().noquote() << "[EffectRegistry] Path traversal detected in QML reference. Effect:" << id << "Path:" << qmlFileName;
            continue;
        }

        if (QFile::exists(absoluteQmlPath)) {
            meta.qmlSource = QUrl::fromLocalFile(absoluteQmlPath).toString();
        } else {
            qWarning().noquote() << "[EffectRegistry] Referenced QML file not found. Effect:" << id << "Path:" << absoluteQmlPath;
            continue;
        }

        registerEffect(meta);
        loadedCount++;
    }

    qDebug().noquote() << "[EffectRegistry]" << dir.dirName() << "→" << loadedCount << " loaded";
}

} // namespace AviQtl::Core
