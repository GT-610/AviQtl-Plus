#include "project_serializer.hpp"
#include "../../ui/include/project_service.hpp"
#include "../../ui/include/timeline_service.hpp"
#include "effect_model.hpp"
#include "effect_registry.hpp"
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
#include <cmath>

namespace AviQtl::Core {

inline constexpr int PROJECT_VERSION = 3;

namespace {
constexpr int kMaxDimension = 32768;
constexpr double kMaxFps = 1000.0;
constexpr int kMaxSampleRate = 192000;
constexpr double kMaxGridBpm = 1000.0;
constexpr double kMaxGridOffset = 86400.0;
constexpr int kMaxGridInterval = 1'000'000;
constexpr int kMaxGridSubdivision = 128;
constexpr int kMaxMagneticSnapRange = 100;

int clampDimension(int value, int fallback) {
    return (value <= 0 || value > kMaxDimension) ? fallback : value;
}
double clampFps(double value, double fallback) {
    return (value <= 0.0 || value > kMaxFps) ? fallback : value;
}
int clampSampleRate(int value, int fallback) {
    return (value <= 0 || value > kMaxSampleRate) ? fallback : value;
}
double clampPositiveGridValue(double value, double maximum, double fallback) {
    return (!std::isfinite(value) || value <= 0.0 || value > maximum) ? fallback : value;
}
double clampGridOffset(double value, double fallback) {
    return (!std::isfinite(value) || value < 0.0 || value > kMaxGridOffset) ? fallback : value;
}
int clampPositiveGridValue(int value, int maximum, int fallback) {
    return (value <= 0 || value > maximum) ? fallback : value;
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

QVariantMap ProjectSerializer::captureSnapshot(const UI::TimelineService *timeline, const UI::ProjectService *project) {
    QVariantMap root;
    root.insert(QStringLiteral("version"), PROJECT_VERSION);

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

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        if (errorMessage != nullptr) {
            *errorMessage = file.errorString();
        }
        return false;
    }

    const QByteArray document = QJsonDocument(QJsonObject::fromVariantMap(snapshot)).toJson();
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

auto ProjectSerializer::save(const QString &fileUrl, const UI::TimelineService *timeline, const UI::ProjectService *project, QString *errorMessage) -> bool {
    return saveSnapshot(fileUrl, captureSnapshot(timeline, project), errorMessage);
}

auto ProjectSerializer::load(const QString &fileUrl, UI::TimelineService *timeline, UI::ProjectService *project, QString *errorMessage) -> bool {
    QString path = QUrl(fileUrl).toLocalFile();
    if (path.isEmpty()) {
        path = fileUrl;
    }

    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        if (errorMessage != nullptr) {
            *errorMessage = file.errorString();
        }
        return false;
    }

    const QByteArray jsonData = file.readAll();
    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(jsonData, &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
        if (errorMessage != nullptr) {
            *errorMessage = parseError.error != QJsonParseError::NoError ? parseError.errorString() : QStringLiteral("Project root must be a JSON object");
        }
        return false;
    }
    QJsonObject root = doc.object();

    const int version = root.value(QStringLiteral("version")).toInt(1);
    if (version < 1 || version > PROJECT_VERSION) {
        if (errorMessage != nullptr) {
            *errorMessage = QStringLiteral("Unsupported project version: %1 (supported: 1-%2)").arg(version).arg(PROJECT_VERSION);
        }
        return false;
    }
    const QString projectDir = QFileInfo(path).absolutePath();

    QJsonObject s = root.value(QStringLiteral("settings")).toObject();
    int w = s.value(QStringLiteral("width")).toInt(AviQtl::kDefaultWidth);
    int h = s.value(QStringLiteral("height")).toInt(AviQtl::kDefaultHeight);
    double fps = s.value(QStringLiteral("fps")).toDouble(AviQtl::kDefaultFps);
    int sampleRate = s.value(QStringLiteral("sampleRate")).toInt(AviQtl::kDefaultSampleRate);

    w = clampDimension(w, AviQtl::kDefaultWidth);
    h = clampDimension(h, AviQtl::kDefaultHeight);
    fps = clampFps(fps, AviQtl::kDefaultFps);
    sampleRate = clampSampleRate(sampleRate, AviQtl::kDefaultSampleRate);

    project->setWidth(w);
    project->setHeight(h);
    project->setFps(fps);
    project->setSampleRate(sampleRate);

    QList<UI::SceneData> tempScenes;
    int maxSceneId = 0;
    QJsonArray scenesArray = root.value(QStringLiteral("scenes")).toArray();
    for (const auto &val : std::as_const(scenesArray)) {
        QJsonObject sobj = val.toObject();
        UI::SceneData scene;
        scene.id = sobj.value(QStringLiteral("id")).toInt();
        scene.name = sobj.value(QStringLiteral("name")).toString();
        scene.width = clampDimension(sobj.value(QStringLiteral("width")).toInt(project->width()), project->width());
        scene.height = clampDimension(sobj.value(QStringLiteral("height")).toInt(project->height()), project->height());
        scene.fps = clampFps(sobj.value(QStringLiteral("fps")).toDouble(project->fps()), project->fps());
        scene.startFrame = sobj.value(QStringLiteral("start")).toInt(0);
        const int totalFrames = sobj.value(QStringLiteral("duration")).toInt(AviQtl::kDefaultTotalFrames);
        scene.totalFrames = totalFrames > 0 ? totalFrames : AviQtl::kDefaultTotalFrames;
        if (version >= 3) {
            scene.durationFrames = std::max(0, sobj.value(QStringLiteral("nestedDuration")).toInt(0));
            scene.lockedLayers = layerSetFromJson(sobj.value(QStringLiteral("lockedLayers")));
            scene.hiddenLayers = layerSetFromJson(sobj.value(QStringLiteral("hiddenLayers")));
            scene.gridMode = sobj.value(QStringLiteral("gridMode")).toString(QStringLiteral("Auto"));
            scene.gridBpm = clampPositiveGridValue(sobj.value(QStringLiteral("gridBpm")).toDouble(120.0), kMaxGridBpm, 120.0);
            scene.gridOffset = clampGridOffset(sobj.value(QStringLiteral("gridOffset")).toDouble(0.0), 0.0);
            scene.gridInterval = clampPositiveGridValue(sobj.value(QStringLiteral("gridInterval")).toInt(10), kMaxGridInterval, 10);
            scene.gridSubdivision = clampPositiveGridValue(sobj.value(QStringLiteral("gridSubdivision")).toInt(4), kMaxGridSubdivision, 4);
            scene.enableSnap = sobj.value(QStringLiteral("enableSnap")).toBool(true);
            scene.magneticSnapRange = clampPositiveGridValue(sobj.value(QStringLiteral("magneticSnapRange")).toInt(10), kMaxMagneticSnapRange, 10);
        }
        tempScenes.append(scene);
        maxSceneId = std::max(scene.id, maxSceneId);
    }

    QJsonArray clipsArray = root.value(QStringLiteral("clips")).toArray();
    int maxClipId = 0;
    for (const auto &val : std::as_const(clipsArray)) {
        QJsonObject c = val.toObject();
        UI::ClipData clip;
        clip.id = c.value(QStringLiteral("id")).toInt();
        clip.sceneId = c.value(QStringLiteral("sceneId")).toInt(0);
        maxClipId = std::max(clip.id, maxClipId);
        clip.type = c.value(QStringLiteral("type")).toString();
        if (clip.type == QLatin1String("camera")) {
            clip.type = QStringLiteral("camera_control");
        }
        clip.startFrame = c.value(QStringLiteral("start")).toInt();
        clip.durationFrames = c.value(QStringLiteral("duration")).toInt();
        clip.layer = std::clamp(c.value(QStringLiteral("layer")).toInt(), 0, 127);
        clip.clipByUpperObject = c.value(QStringLiteral("clipByUpperObject")).toBool(false);
        clip.params = c.value(QStringLiteral("params")).toObject().toVariantMap();

        if (version >= 2) {
            convertMediaPaths(clip.params, projectDir, false);
        }

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
        for (const auto &ev : std::as_const(effArr)) {
            QJsonObject eObj = ev.toObject();
            QString effId = eObj.value(QStringLiteral("id")).toString();
            if (effId == QLatin1String("camera")) {
                effId = QStringLiteral("camera_control");
            }
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
            auto *eff = new UI::EffectModel(effId, displayName, meta.kind, meta.categories, effectParams, meta.qmlSource, meta.uiDefinition, timeline);
            eff->setEnabled(eObj.value(QStringLiteral("enabled")).toBool(true));
            auto it = eObj.find(QStringLiteral("keyframes"));
            if (it != eObj.end()) {
                eff->setKeyframeTracks(it.value().toObject().toVariantMap());
            }
            clip.effects.append(eff);
        }

        for (auto &scene : tempScenes) {
            if (scene.id == clip.sceneId) {
                scene.clips.append(clip);
                break;
            }
        }
    }

    timeline->setScenes(tempScenes);
    timeline->setNextClipId(maxClipId + 1);
    timeline->setNextSceneId(maxSceneId + 1);
    QMetaObject::invokeMethod(timeline, "clipsChanged");

    return true;
}

} // namespace AviQtl::Core
