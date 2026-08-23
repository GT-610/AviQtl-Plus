#include "project_serializer.hpp"
#include "../../ui/include/project_service.hpp"
#include "../../ui/include/timeline_service.hpp"
#include "effect_model.hpp"
#include "effect_registry.hpp"
#include "bounded_file.hpp"
#include "rust_project_document.hpp"
#include "settings_manager.hpp"
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QUrl>
#include <algorithm>
#include <cstdint>
#include <span>
#include <vector>

namespace AviQtl::Core {

namespace {

QString projectNormalizationError(RustCore::ProjectStatus status) {
    switch (status) {
    case RustCore::ProjectStatus::InvalidJson:
        return QStringLiteral("Invalid project JSON");
    case RustCore::ProjectStatus::UnsupportedVersion:
        return QStringLiteral("Unsupported project version");
    case RustCore::ProjectStatus::InvalidArgument:
    case RustCore::ProjectStatus::OverlappingBuffers:
    case RustCore::ProjectStatus::BufferTooSmall:
    case RustCore::ProjectStatus::Ok:
        return QStringLiteral("Rust project normalization failed with status %1").arg(static_cast<std::uint32_t>(status));
    }
    return QStringLiteral("Rust project normalization failed");
}

void deleteSceneEffects(const QList<UI::SceneData> &scenes) {
    for (const auto &scene : scenes) {
        for (const auto &clip : scene.clips) {
            for (auto *effect : clip.effects) {
                delete effect;
            }
        }
    }
}
} // namespace

static QString toRelativePath(const QString &absolutePath, const QString &baseDir) {
    if (absolutePath.isEmpty()) {
        return absolutePath;
    }
    QDir base(baseDir);
    QString rel = base.relativeFilePath(absolutePath);
    return rel.isEmpty() ? absolutePath : rel;
}

static QString toAbsolutePath(const QString &path, const QString &baseDir) {
    if (path.isEmpty()) {
        return path;
    }
    if (QDir::isAbsolutePath(path)) {
        return path;
    }
    return QDir::cleanPath(QDir(baseDir).absoluteFilePath(path));
}

static void convertMediaPaths(QVariantMap &params, const QString &baseDir, bool toRelative) {
    const QStringList mediaKeys = {QStringLiteral("video"), QStringLiteral("image"), QStringLiteral("audio")};
    for (const auto &key : mediaKeys) {
        auto it = params.find(key);
        if (it == params.end() || !it->canConvert<QVariantMap>()) {
            continue;
        }
        QVariantMap media = it->toMap();
        auto pathIt = media.find(QStringLiteral("path"));
        if (pathIt == media.end()) {
            pathIt = media.find(QStringLiteral("source"));
        }
        if (pathIt != media.end()) {
            QString p = pathIt->toString();
            if (!p.isEmpty()) {
                *pathIt = toRelative ? toRelativePath(p, baseDir) : toAbsolutePath(p, baseDir);
            }
        }
        params[key] = media;
    }
}

static void convertEffectMediaPath(const QString &effectId, QVariantMap &params, const QString &baseDir, bool toRelative) {
    QString pathKey;
    if (effectId == QLatin1String("video") || effectId == QLatin1String("image")) {
        pathKey = QStringLiteral("path");
    } else if (effectId == QLatin1String("audio")) {
        pathKey = QStringLiteral("source");
    } else {
        return;
    }

    auto pathIt = params.find(pathKey);
    if (pathIt == params.end() || pathIt->toString().isEmpty()) {
        return;
    }
    *pathIt = toRelative ? toRelativePath(pathIt->toString(), baseDir) : toAbsolutePath(pathIt->toString(), baseDir);
}

static auto layerSetToVariantList(const QSet<int> &layers) -> QVariantList {
    QList<int> sortedLayers(layers.cbegin(), layers.cend());
    std::sort(sortedLayers.begin(), sortedLayers.end());
    QVariantList result;
    for (int layer : std::as_const(sortedLayers)) {
        result.append(QVariant::fromValue(layer));
    }
    return result;
}

static auto layerSetFromJson(const QJsonValue &value) -> QSet<int> {
    QSet<int> result;
    for (const QJsonValue &layer : value.toArray()) {
        const int index = layer.toInt(-1);
        if (index >= 0 && index <= 127) {
            result.insert(index);
        }
    }
    return result;
}

QVariantMap ProjectSerializer::captureSnapshot(UI::TimelineService *timeline,
                                               const UI::ProjectService *project) {
    // Native consumers may still update EffectModel/ClipData directly. Fold those adapter-side
    // changes into one Rust transaction before reading the authoritative snapshot.
    const bool projectionCommitted = timeline->commitTimelineProjection();
    QVariantMap root = projectionCommitted ? timeline->timelineStateSnapshot() : QVariantMap{};
    if (!root.isEmpty()) {
        QVariantMap settings = root.value(QStringLiteral("settings")).toMap();
        settings.insert(QStringLiteral("width"), project->width());
        settings.insert(QStringLiteral("height"), project->height());
        settings.insert(QStringLiteral("fps"), project->fps());
        settings.insert(QStringLiteral("sampleRate"), project->sampleRate());
        root.insert(QStringLiteral("settings"), settings);
        return root;
    }

    root.insert(QStringLiteral("version"), RustCore::currentProjectVersion());

    QVariantMap settings;
    settings.insert(QStringLiteral("width"), project->width());
    settings.insert(QStringLiteral("height"), project->height());
    settings.insert(QStringLiteral("fps"), project->fps());
    settings.insert(QStringLiteral("sampleRate"), project->sampleRate());
    root.insert(QStringLiteral("settings"), settings);

    QVariantList scenes;
    for (const auto &scene : timeline->getAllScenes()) {
        QVariantMap sObj;
        sObj.insert(QStringLiteral("id"), scene.id);
        sObj.insert(QStringLiteral("name"), scene.name);
        sObj.insert(QStringLiteral("width"), scene.width);
        sObj.insert(QStringLiteral("height"), scene.height);
        sObj.insert(QStringLiteral("fps"), scene.fps);
        sObj.insert(QStringLiteral("start"), scene.startFrame);
        sObj.insert(QStringLiteral("duration"), scene.totalFrames);
        sObj.insert(QStringLiteral("nestedDuration"), scene.durationFrames);
        sObj.insert(QStringLiteral("lockedLayers"), layerSetToVariantList(scene.lockedLayers));
        sObj.insert(QStringLiteral("hiddenLayers"), layerSetToVariantList(scene.hiddenLayers));
        sObj.insert(QStringLiteral("gridMode"), scene.gridMode);
        sObj.insert(QStringLiteral("gridBpm"), scene.gridBpm);
        sObj.insert(QStringLiteral("gridOffset"), scene.gridOffset);
        sObj.insert(QStringLiteral("gridInterval"), scene.gridInterval);
        sObj.insert(QStringLiteral("gridSubdivision"), scene.gridSubdivision);
        sObj.insert(QStringLiteral("enableSnap"), scene.enableSnap);
        sObj.insert(QStringLiteral("magneticSnapRange"), scene.magneticSnapRange);
        scenes.append(sObj);
    }
    root.insert(QStringLiteral("scenes"), scenes);

    QVariantList clips;
    for (const auto &scene : timeline->getAllScenes()) {
        for (const auto &clip : std::as_const(scene.clips)) {
            QVariantMap clipObj;
            clipObj.insert(QStringLiteral("id"), clip.id);
            clipObj.insert(QStringLiteral("sceneId"), clip.sceneId);
            clipObj.insert(QStringLiteral("type"), clip.type);
            clipObj.insert(QStringLiteral("start"), clip.startFrame);
            clipObj.insert(QStringLiteral("duration"), clip.durationFrames);
            clipObj.insert(QStringLiteral("layer"), clip.layer);
            clipObj.insert(QStringLiteral("clipByUpperObject"), clip.clipByUpperObject);

            clipObj.insert(QStringLiteral("params"), clip.params);

            QVariantList audioPlugins;
            for (const auto &plugin : std::as_const(clip.audioPlugins)) {
                QVariantMap pObj;
                pObj.insert(QStringLiteral("id"), plugin.id);
                pObj.insert(QStringLiteral("enabled"), plugin.enabled);
                pObj.insert(QStringLiteral("params"), plugin.params);
                if (!plugin.keyframeTracks.isEmpty()) {
                    pObj.insert(QStringLiteral("keyframes"), plugin.keyframeTracks);
                }
                audioPlugins.append(pObj);
            }
            clipObj.insert(QStringLiteral("audioPlugins"), audioPlugins);

            QVariantList effects;
            for (const auto *eff : std::as_const(clip.effects)) {
                QVariantMap eObj;
                eObj.insert(QStringLiteral("id"), eff->id());
                eObj.insert(QStringLiteral("name"), eff->name());
                eObj.insert(QStringLiteral("enabled"), eff->isEnabled());
                eObj.insert(QStringLiteral("params"), eff->params());
                eObj.insert(QStringLiteral("keyframes"), eff->keyframeTracks());
                effects.append(eObj);
            }
            clipObj.insert(QStringLiteral("effects"), effects);
            clips.append(clipObj);
        }
    }
    root.insert(QStringLiteral("clips"), clips);
    return root;
}

auto ProjectSerializer::saveSnapshot(const QString &fileUrl, const QVariantMap &capturedSnapshot, QString *errorMessage) -> bool {
    QString path = QUrl(fileUrl).toLocalFile();
    if (path.isEmpty()) {
        path = fileUrl;
    }

    const QString projectDir = QFileInfo(path).absolutePath();
    QVariantMap snapshot = capturedSnapshot;
    QVariantList clips = snapshot.value(QStringLiteral("clips")).toList();
    for (QVariant &clipValue : clips) {
        QVariantMap clip = clipValue.toMap();
        QVariantMap params = clip.value(QStringLiteral("params")).toMap();
        convertMediaPaths(params, projectDir, true);
        clip.insert(QStringLiteral("params"), params);

        QVariantList effects = clip.value(QStringLiteral("effects")).toList();
        for (QVariant &effectValue : effects) {
            QVariantMap effect = effectValue.toMap();
            QVariantMap effectParams = effect.value(QStringLiteral("params")).toMap();
            convertEffectMediaPath(effect.value(QStringLiteral("id")).toString(), effectParams, projectDir, true);
            effect.insert(QStringLiteral("params"), effectParams);
            effectValue = effect;
        }
        clip.insert(QStringLiteral("effects"), effects);
        clipValue = clip;
    }
    snapshot.insert(QStringLiteral("clips"), clips);

    const QByteArray capturedDocument = QJsonDocument(QJsonObject::fromVariantMap(snapshot)).toJson(QJsonDocument::Compact);
    const auto input = std::span(reinterpret_cast<const std::uint8_t *>(capturedDocument.constData()), static_cast<std::size_t>(capturedDocument.size()));
    std::vector<std::uint8_t> serializedDocument;
    const RustCore::ProjectStatus serializationStatus = RustCore::normalizeProjectJson(input, serializedDocument);
    if (serializationStatus != RustCore::ProjectStatus::Ok) {
        if (errorMessage != nullptr) {
            *errorMessage = projectNormalizationError(serializationStatus);
        }
        return false;
    }
    if (serializedDocument.size() > static_cast<std::size_t>(Internal::FileSizeLimit::ProjectDocument)) {
        if (errorMessage != nullptr) {
            *errorMessage = QStringLiteral("Project exceeds the maximum allowed size of %1 bytes.")
                                .arg(Internal::FileSizeLimit::ProjectDocument);
        }
        return false;
    }

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        if (errorMessage != nullptr) {
            *errorMessage = file.errorString();
        }
        return false;
    }

    const QByteArray document(reinterpret_cast<const char *>(serializedDocument.data()), static_cast<qsizetype>(serializedDocument.size()));
    if (file.write(document) != document.size()) {
        if (errorMessage != nullptr) {
            *errorMessage = file.errorString();
        }
        file.cancelWriting();
        return false;
    }

    if (!file.commit()) {
        if (errorMessage != nullptr) {
            *errorMessage = file.errorString();
        }
        return false;
    }

    return true;
}

auto ProjectSerializer::save(const QString &fileUrl, UI::TimelineService *timeline,
                             const UI::ProjectService *project, QString *errorMessage) -> bool {
    return saveSnapshot(fileUrl, captureSnapshot(timeline, project), errorMessage);
}

auto ProjectSerializer::load(const QString &fileUrl, UI::TimelineService *timeline, UI::ProjectService *project, QString *errorMessage) -> bool {
    QString path = QUrl(fileUrl).toLocalFile();
    if (path.isEmpty()) {
        path = fileUrl;
    }

    const auto jsonDataResult = Internal::readFileBounded(
        path, Internal::FileSizeLimit::ProjectDocument, errorMessage);
    if (!jsonDataResult.has_value())
        return false;
    const QByteArray &jsonData = *jsonDataResult;
    const auto input = std::span(reinterpret_cast<const std::uint8_t *>(jsonData.constData()), static_cast<std::size_t>(jsonData.size()));
    std::vector<std::uint8_t> normalizedJson;
    const RustCore::ProjectStatus normalizationStatus = RustCore::normalizeProjectJson(input, normalizedJson);
    if (normalizationStatus != RustCore::ProjectStatus::Ok) {
        if (errorMessage != nullptr) {
            *errorMessage = projectNormalizationError(normalizationStatus);
        }
        return false;
    }
    const QByteArray normalizedData(reinterpret_cast<const char *>(normalizedJson.data()), static_cast<qsizetype>(normalizedJson.size()));
    const QJsonDocument doc = QJsonDocument::fromJson(normalizedData);
    if (!doc.isObject()) {
        if (errorMessage != nullptr) {
            *errorMessage = QStringLiteral("Rust project normalization returned invalid JSON");
        }
        return false;
    }
    QJsonObject root = doc.object();
    QVariantMap runtimeSnapshot = root.toVariantMap();
    QVariantList runtimeClips = runtimeSnapshot.value(QStringLiteral("clips")).toList();

    const int version = root.value(QStringLiteral("version")).toInt();
    const QString projectDir = QFileInfo(path).absolutePath();

    QJsonObject s = root.value(QStringLiteral("settings")).toObject();
    const int w = s.value(QStringLiteral("width")).toInt();
    const int h = s.value(QStringLiteral("height")).toInt();
    const double fps = s.value(QStringLiteral("fps")).toDouble();
    const int sampleRate = s.value(QStringLiteral("sampleRate")).toInt();

    QList<UI::SceneData> tempScenes;
    int maxSceneId = 0;
    QJsonArray scenesArray = root.value(QStringLiteral("scenes")).toArray();
    for (const auto &val : std::as_const(scenesArray)) {
        QJsonObject sobj = val.toObject();
        UI::SceneData scene;
        scene.id = sobj.value(QStringLiteral("id")).toInt();
        scene.name = sobj.value(QStringLiteral("name")).toString();
        scene.width = sobj.value(QStringLiteral("width")).toInt();
        scene.height = sobj.value(QStringLiteral("height")).toInt();
        scene.fps = sobj.value(QStringLiteral("fps")).toDouble();
        scene.startFrame = sobj.value(QStringLiteral("start")).toInt();
        scene.totalFrames = sobj.value(QStringLiteral("duration")).toInt();
        scene.durationFrames = sobj.value(QStringLiteral("nestedDuration")).toInt();
        scene.lockedLayers = layerSetFromJson(sobj.value(QStringLiteral("lockedLayers")));
        scene.hiddenLayers = layerSetFromJson(sobj.value(QStringLiteral("hiddenLayers")));
        scene.gridMode = sobj.value(QStringLiteral("gridMode")).toString();
        scene.gridBpm = sobj.value(QStringLiteral("gridBpm")).toDouble();
        scene.gridOffset = sobj.value(QStringLiteral("gridOffset")).toDouble();
        scene.gridInterval = sobj.value(QStringLiteral("gridInterval")).toInt();
        scene.gridSubdivision = sobj.value(QStringLiteral("gridSubdivision")).toInt();
        scene.enableSnap = sobj.value(QStringLiteral("enableSnap")).toBool();
        scene.magneticSnapRange = sobj.value(QStringLiteral("magneticSnapRange")).toInt();
        tempScenes.append(scene);
        maxSceneId = std::max(scene.id, maxSceneId);
    }

    QJsonArray clipsArray = root.value(QStringLiteral("clips")).toArray();
    int maxClipId = 0;
    for (qsizetype clipIndex = 0; clipIndex < clipsArray.size(); ++clipIndex) {
        const QJsonValue val = clipsArray.at(clipIndex);
        QJsonObject c = val.toObject();
        QVariantMap runtimeClip = runtimeClips.value(clipIndex).toMap();
        UI::ClipData clip;
        clip.id = c.value(QStringLiteral("id")).toInt();
        clip.sceneId = c.value(QStringLiteral("sceneId")).toInt(0);
        maxClipId = std::max(clip.id, maxClipId);
        clip.type = c.value(QStringLiteral("type")).toString();
        clip.startFrame = c.value(QStringLiteral("start")).toInt();
        clip.durationFrames = c.value(QStringLiteral("duration")).toInt();
        clip.layer = c.value(QStringLiteral("layer")).toInt();
        clip.clipByUpperObject = c.value(QStringLiteral("clipByUpperObject")).toBool();
        clip.params = c.value(QStringLiteral("params")).toObject().toVariantMap();

        if (version >= 2) {
            convertMediaPaths(clip.params, projectDir, false);
        }
        runtimeClip.insert(QStringLiteral("params"), clip.params);

        QJsonArray audioPluginsArray = c.value(QStringLiteral("audioPlugins")).toArray();
        for (const auto &pv : std::as_const(audioPluginsArray)) {
            QJsonObject pObj = pv.toObject();
            UI::AudioPluginState plugin;
            plugin.id = pObj.value(QStringLiteral("id")).toString();
            plugin.enabled = pObj.value(QStringLiteral("enabled")).toBool(true);
            plugin.params = pObj.value(QStringLiteral("params")).toObject().toVariantMap();
            auto kfIt = pObj.find(QStringLiteral("keyframes"));
            if (kfIt != pObj.end()) {
                plugin.keyframeTracks = kfIt.value().toObject().toVariantMap();
            }
            if (!plugin.id.isEmpty()) {
                clip.audioPlugins.append(plugin);
            }
        }

        QJsonArray effArr = c.value(QStringLiteral("effects")).toArray();
        const QVariantList runtimeEffects = runtimeClip.value(QStringLiteral("effects")).toList();
        QVariantList acceptedRuntimeEffects;
        acceptedRuntimeEffects.reserve(effArr.size());
        for (qsizetype effectIndex = 0; effectIndex < effArr.size(); ++effectIndex) {
            const QJsonValue ev = effArr.at(effectIndex);
            QJsonObject eObj = ev.toObject();
            QString effId = eObj.value(QStringLiteral("id")).toString();
            EffectMetadata meta = EffectRegistry::instance().getEffect(effId);
            if (effId.isEmpty() || meta.id.isEmpty()) {
                qWarning().noquote() << QStringLiteral("Skipping missing effect while loading project:") << effId;
                continue;
            }
            QString displayName = meta.name.isEmpty() ? eObj.value(QStringLiteral("name")).toString() : meta.name;
            QVariantMap effectParams = eObj.value(QStringLiteral("params")).toObject().toVariantMap();
            if (version >= 2) {
                convertEffectMediaPath(effId, effectParams, projectDir, false);
            }
            QVariantMap runtimeEffect;
            if (effectIndex < runtimeEffects.size()) {
                runtimeEffect = runtimeEffects.at(effectIndex).toMap();
            }
            runtimeEffect.insert(QStringLiteral("params"), effectParams);
            acceptedRuntimeEffects.append(runtimeEffect);
            auto *eff = new UI::EffectModel(effId, displayName, meta.kind, meta.categories, effectParams, meta.qmlSource, meta.uiDefinition, timeline);
            eff->setEnabled(eObj.value(QStringLiteral("enabled")).toBool(true));
            auto it = eObj.find(QStringLiteral("keyframes"));
            if (it != eObj.end()) {
                eff->setKeyframeTracks(it.value().toObject().toVariantMap());
            }
            clip.effects.append(eff);
        }
        runtimeClip.insert(QStringLiteral("effects"), acceptedRuntimeEffects);
        if (clipIndex < runtimeClips.size()) {
            runtimeClips[clipIndex] = runtimeClip;
        }

        for (auto &scene : tempScenes) {
            if (scene.id == clip.sceneId) {
                scene.clips.append(clip);
                break;
            }
        }
    }

    runtimeSnapshot.insert(QStringLiteral("clips"), runtimeClips);
    const QVariantMap previousTimelineState = timeline->timelineStateSnapshot();
    const int previousNextClipId = timeline->nextClipId();
    const int previousNextSceneId = timeline->nextSceneId();
    if (!timeline->resetTimelineState(runtimeSnapshot, maxClipId + 1, maxSceneId + 1)) {
        deleteSceneEffects(tempScenes);
        if (errorMessage != nullptr) {
            *errorMessage = QStringLiteral("Rust timeline state rejected the loaded project");
        }
        return false;
    }
    if (!timeline->setScenes(tempScenes)) {
        if (!previousTimelineState.isEmpty() &&
            !timeline->resetTimelineState(previousTimelineState, previousNextClipId,
                                          previousNextSceneId)) {
            qWarning() << "Failed to restore Rust timeline state after load rejection";
        }
        if (errorMessage != nullptr) {
            *errorMessage = QStringLiteral("Rust timeline state rejected the loaded project");
        }
        return false;
    }
    project->setWidth(w);
    project->setHeight(h);
    project->setFps(fps);
    project->setSampleRate(sampleRate);
    QMetaObject::invokeMethod(timeline, "clipsChanged");

    return true;
}

} // namespace AviQtl::Core
