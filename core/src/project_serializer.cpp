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
#include <QUrl>
#include <algorithm>

namespace AviQtl::Core {

inline constexpr int PROJECT_VERSION = 2;

namespace {
constexpr int kMaxDimension = 32768;
constexpr double kMaxFps = 1000.0;
constexpr int kMaxSampleRate = 192000;

int clampDimension(int value, int fallback) {
    return (value <= 0 || value > kMaxDimension) ? fallback : value;
}
double clampFps(double value, double fallback) {
    return (value <= 0.0 || value > kMaxFps) ? fallback : value;
}
int clampSampleRate(int value, int fallback) {
    return (value <= 0 || value > kMaxSampleRate) ? fallback : value;
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

auto ProjectSerializer::save(const QString &fileUrl, const UI::TimelineService *timeline, const UI::ProjectService *project, QString *errorMessage) -> bool {
    QString path = QUrl(fileUrl).toLocalFile();
    if (path.isEmpty()) {
        path = fileUrl;
    }

    const QString projectDir = QFileInfo(path).absolutePath();

    QJsonObject root;
    root.insert(QStringLiteral("version"), PROJECT_VERSION);

    QJsonObject settings;
    settings.insert(QStringLiteral("width"), project->width());
    settings.insert(QStringLiteral("height"), project->height());
    settings.insert(QStringLiteral("fps"), project->fps());
    settings.insert(QStringLiteral("sampleRate"), project->sampleRate());
    root.insert(QStringLiteral("settings"), settings);

    QJsonArray scenesArray;
    for (const auto &scene : timeline->getAllScenes()) {
        QJsonObject sObj;
        sObj.insert(QStringLiteral("id"), scene.id);
        sObj.insert(QStringLiteral("name"), scene.name);
        sObj.insert(QStringLiteral("width"), scene.width);
        sObj.insert(QStringLiteral("height"), scene.height);
        sObj.insert(QStringLiteral("fps"), scene.fps);
        sObj.insert(QStringLiteral("start"), scene.startFrame);
        sObj.insert(QStringLiteral("duration"), scene.totalFrames);
        scenesArray.append(sObj);
    }
    root.insert(QStringLiteral("scenes"), scenesArray);

    QJsonArray clipsArray;
    for (const auto &scene : timeline->getAllScenes()) {
        for (const auto &clip : std::as_const(scene.clips)) {
            QJsonObject clipObj;
            clipObj.insert(QStringLiteral("id"), clip.id);
            clipObj.insert(QStringLiteral("sceneId"), clip.sceneId);
            clipObj.insert(QStringLiteral("type"), clip.type);
            clipObj.insert(QStringLiteral("start"), clip.startFrame);
            clipObj.insert(QStringLiteral("duration"), clip.durationFrames);
            clipObj.insert(QStringLiteral("layer"), clip.layer);
            clipObj.insert(QStringLiteral("clipByUpperObject"), clip.clipByUpperObject);

            QVariantMap paramsCopy = clip.params;
            convertMediaPaths(paramsCopy, projectDir, true);
            clipObj.insert(QStringLiteral("params"), QJsonObject::fromVariantMap(paramsCopy));

            QJsonArray audioPluginsArray;
            for (const auto &plugin : std::as_const(clip.audioPlugins)) {
                QJsonObject pObj;
                pObj.insert(QStringLiteral("id"), plugin.id);
                pObj.insert(QStringLiteral("enabled"), plugin.enabled);
                pObj.insert(QStringLiteral("params"), QJsonObject::fromVariantMap(plugin.params));
                if (!plugin.keyframeTracks.isEmpty()) {
                    pObj.insert(QStringLiteral("keyframes"), QJsonObject::fromVariantMap(plugin.keyframeTracks));
                }
                audioPluginsArray.append(pObj);
            }
            clipObj.insert(QStringLiteral("audioPlugins"), audioPluginsArray);

            QJsonArray effArray;
            for (const auto *eff : std::as_const(clip.effects)) {
                QJsonObject eObj;
                eObj.insert(QStringLiteral("id"), eff->id());
                eObj.insert(QStringLiteral("name"), eff->name());
                eObj.insert(QStringLiteral("enabled"), eff->isEnabled());
                eObj.insert(QStringLiteral("params"), QJsonObject::fromVariantMap(eff->params()));
                eObj.insert(QStringLiteral("keyframes"), QJsonObject::fromVariantMap(eff->keyframeTracks()));
                effArray.append(eObj);
            }
            clipObj.insert(QStringLiteral("effects"), effArray);
            clipsArray.append(clipObj);
        }
    }
    root.insert(QStringLiteral("clips"), clipsArray);

    QFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        if (errorMessage != nullptr) {
            *errorMessage = file.errorString();
        }
        return false;
    }
    file.write(QJsonDocument(root).toJson());
    return true;
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

    auto jsonData = file.readAll();
    QJsonDocument doc = QJsonDocument::fromJson(jsonData);
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
            auto *eff = new UI::EffectModel(effId, displayName, meta.kind, meta.categories, eObj.value(QStringLiteral("params")).toObject().toVariantMap(), meta.qmlSource, meta.uiDefinition, timeline);
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
