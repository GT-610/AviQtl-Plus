#include "audio_decoder.hpp"
#include "commands.hpp"
#include "constants.hpp"
#include "core/include/media_utils.hpp"
#include "effect_registry.hpp"
#include "engine/plugin/audio_plugin_manager.hpp"
#include "engine/timeline/ecs.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "transport_service.hpp"
#include "video_decoder.hpp"
#include <QFileInfo>
#include <QSet>
#include <QUrl>
#include <QtGlobal>
#include <algorithm>
#include <cmath>

extern "C" {
#include <libavutil/avutil.h>
}

namespace AviQtl::UI {

// 該当エフェクトが存在しない場合は 0 を返す。
// ClipItem.qml の groupLayerCount プロパティへ格納され、タイムライン上の
// カーテン描画に使用される。
static int getControlLayerCount(const ClipData &clip) {
    // カーテンを持つエフェクトID と layerCountパラメータ名のリスト
    static const QList<QLatin1String> kControlEffectIds = {
        QLatin1String("GroupControl"),
        QLatin1String("camera_control"),
        QLatin1String("camera"),
    };
    for (auto *eff : clip.effects) {
        if (kControlEffectIds.contains(eff->id())) {
            return eff->params().value(QStringLiteral("layerCount"), 0).toInt();
        }
    }
    return 0;
}

static QVariantMap clipToVariantMap(const ClipData &clip) {
    QVariantMap map;
    map.insert(QStringLiteral("id"), clip.id);
    map.insert(QStringLiteral("sceneId"), clip.sceneId);
    map.insert(QStringLiteral("type"), clip.type);
    map.insert(QStringLiteral("startFrame"), clip.startFrame);
    map.insert(QStringLiteral("durationFrames"), clip.durationFrames);
    map.insert(QStringLiteral("layer"), clip.layer);
    map.insert(QStringLiteral("clipByUpperObject"), clip.clipByUpperObject);

    const auto meta = AviQtl::Core::EffectRegistry::instance().getEffect(clip.type);
    map.insert(QStringLiteral("name"), !meta.name.isEmpty() ? meta.name : clip.type);
    if (!meta.qmlSource.isEmpty()) {
        map.insert(QStringLiteral("qmlSource"), meta.qmlSource);
    }

    QVariantMap params;
    params.insert(QStringLiteral("layer"), clip.layer);
    params.insert(QStringLiteral("startFrame"), clip.startFrame);
    params.insert(QStringLiteral("durationFrames"), clip.durationFrames);
    params.insert(QStringLiteral("id"), clip.id);
    map.insert(QStringLiteral("groupLayerCount"), getControlLayerCount(clip));

    QList<QObject *> effectModels;
    for (auto *effect : clip.effects) {
        const QVariantMap effectParams = effect->params();
        for (auto it = effectParams.cbegin(); it != effectParams.cend(); ++it) {
            params.insert(it.key(), it.value());
        }
        effectModels.append(effect);
    }
    map.insert(QStringLiteral("params"), params);
    map.insert(QStringLiteral("effectModels"), QVariant::fromValue(effectModels));
    return map;
}

static int findVacantFrameForLinkedMedia(const TimelineService *timeline, int videoLayer, int startFrame, int duration) {
    if (timeline == nullptr) {
        return std::max(0, startFrame);
    }

    int candidate = std::max(0, startFrame);
    for (int i = 0; i < 100; ++i) {
        const int videoStart = timeline->findVacantFrame(videoLayer, candidate, duration, -1);
        const int audioStart = timeline->findVacantFrame(videoLayer + 1, videoStart, duration, -1);
        if (audioStart == videoStart) {
            return videoStart;
        }
        candidate = audioStart;
    }
    const int videoStart = timeline->findVacantFrame(videoLayer, candidate, duration, -1);
    const int audioStart = timeline->findVacantFrame(videoLayer + 1, videoStart, duration, -1);
    return (audioStart == videoStart) ? videoStart : candidate;
}

void TimelineController::handleClipClick(int clipId, int modifiers) { // NOLINT(bugprone-easily-swappable-parameters)
    if (!m_timeline) {
        return;
    }
    if ((modifiers & Qt::ControlModifier) != 0U) {
        m_timeline->toggleSelection(clipId, QVariantMap());
    } else {
        m_timeline->applySelectionIds({clipId});
    }
}

void TimelineController::updateSelectionPreview(int frameA, int frameB, int layerA, int layerB, bool additive) { // NOLINT(bugprone-easily-swappable-parameters)
    if (!m_timeline || !m_selection) {
        return;
    }
    QVariantList ids;
    if (additive) {
        ids = m_selection->selectedClipIds();
    }

    int minF = std::min(frameA, frameB);
    int maxF = std::max(frameA, frameB);
    int minL = std::min(layerA, layerB);
    int maxL = std::max(layerA, layerB);

    for (const auto &clip : m_timeline->clips()) {
        int clipMaxL = clip.layer + getControlLayerCount(clip);

        int clipEnd = clip.startFrame + clip.durationFrames;
        if (clip.startFrame < maxF && minF < clipEnd && clipMaxL >= minL && clip.layer <= maxL) {
            if (!ids.contains(clip.id)) {
                ids.append(clip.id);
            }
        }
    }

    if (m_previewSelectionIds != ids) {
        m_previewSelectionIds = ids;
        emit previewSelectionIdsChanged();
    }
}

void TimelineController::finalizeSelectionPreview() {
    applySelectionIds(m_previewSelectionIds);
    clearSelectionPreview();
}

void TimelineController::clearSelectionPreview() {
    if (!m_previewSelectionIds.isEmpty()) {
        m_previewSelectionIds.clear();
        emit previewSelectionIdsChanged();
    }
}

auto TimelineController::previewSelectionIds() const -> QVariantList { return m_previewSelectionIds; }

void TimelineController::setClipProperty(const QString &name, const QVariant &value) {
    if (!m_selection || !m_timeline) {
        return;
    }
    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    m_timeline->undoStack()->beginMacro(tr("プロパティ変更: %1").arg(name));

    for (const QVariant &vId : ids) {
        int id = vId.toInt();
        const ClipData *clip = m_timeline->findClipById(id);
        if (clip == nullptr) {
            continue;
        }

        int targetEffectIndex = -1;
        for (int i = 0; i < clip->effects.size(); ++i) {
            if (clip->effects.value(i)->params().contains(name)) {
                targetEffectIndex = i;
                break;
            }
        }

        if (targetEffectIndex == -1 && !clip->effects.isEmpty()) {
            targetEffectIndex = 0;
            static const QStringList transformKeys = {"x", "y", "z", "scale", "aspect", "rotationX", "rotationY", "rotationZ", "opacity"};
            if (!transformKeys.contains(name) && clip->effects.size() > 1) {
                targetEffectIndex = 1;
            }
        }

        if (targetEffectIndex != -1 && targetEffectIndex < clip->effects.size()) {
            updateClipEffectParam(id, targetEffectIndex, name, value);
        }
    }

    m_timeline->undoStack()->endMacro();
}

auto TimelineController::getClipProperty(const QString &name) const -> QVariant { return m_selection->selectedClipData().value(name); }

auto TimelineController::clipStartFrame() const -> int { return m_selection->selectedClipData().value(QStringLiteral("startFrame"), 0).toInt(); }
void TimelineController::setClipStartFrame(int frame) {
    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    m_timeline->undoStack()->beginMacro(tr("開始フレーム変更"));
    for (const QVariant &vId : ids) {
        int id = vId.toInt();
        if (const auto *c = m_timeline->findClipById(id)) {
            m_timeline->updateClip(id, c->layer, frame, c->durationFrames);
        }
    }
    m_timeline->undoStack()->endMacro();
}

auto TimelineController::clipDurationFrames() const -> int { return m_selection->selectedClipData().value(QStringLiteral("durationFrames"), 100).toInt(); }
void TimelineController::setClipDurationFrames(int frames) {
    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    m_timeline->undoStack()->beginMacro(tr("長さ変更"));
    for (const QVariant &vId : ids) {
        int id = vId.toInt();
        if (const auto *c = m_timeline->findClipById(id)) {
            m_timeline->updateClip(id, c->layer, c->startFrame, frames);
        }
    }
    m_timeline->undoStack()->endMacro();
}

auto TimelineController::layer() const -> int { return m_selection->selectedClipData().value(QStringLiteral("layer"), 0).toInt(); }
void TimelineController::setLayer(int layer) {
    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    m_timeline->undoStack()->beginMacro(tr("レイヤー変更"));
    for (const QVariant &vId : ids) {
        int id = vId.toInt();
        if (const auto *c = m_timeline->findClipById(id)) {
            m_timeline->updateClip(id, layer, c->startFrame, c->durationFrames);
        }
    }
    m_timeline->undoStack()->endMacro();
}

void TimelineController::setCursorFrame(int frame) {
    if (m_cursorFrame != frame) {
        m_cursorFrame = frame;
        emit cursorFrameChanged();
    }
}

void TimelineController::setSelectedLayer(int layer) {
    if (m_selectedLayer != layer) {
        m_selectedLayer = layer;
        emit selectedLayerChanged();
    }
}

auto TimelineController::isClipActive() const -> bool { return m_isClipActive; }

void TimelineController::updateClipActiveState() {
    int current = m_transport->currentFrame();
    int start = clipStartFrame();
    int duration = clipDurationFrames();
    bool active = (current >= start) && (current < start + duration);
    if (m_isClipActive != active) {
        m_isClipActive = active;
        emit isClipActiveChanged();
    }
}

auto TimelineController::activeObjectType() const -> QString { return m_selection->selectedClipData().value(QStringLiteral("type"), "rect").toString(); }

void TimelineController::createObject(const QString &type, int startFrame, int layer) {
    if (m_timeline != nullptr) {
        m_timeline->createClip(type, startFrame, layer);
    }
}

void TimelineController::createTransition(const QString &type, int startFrame, int layer) {
    if (m_timeline != nullptr) {
        // トランジションは通常のオブジェクトとして作成
        m_timeline->createClip(type, startFrame, layer);
    }
}

double TimelineController::getSceneFps() const {
    const int sceneId = m_timeline->currentSceneId();
    for (const auto &scene : m_timeline->getAllScenes()) {
        if (scene.id == sceneId)
            return scene.fps > 0.0 ? scene.fps : AviQtl::kDefaultFps;
    }
    return AviQtl::kDefaultFps;
}

auto TimelineController::importMediaFile(const QString &fileUrl, int startFrame, int layer) -> QVariantMap {
    auto editResult = [](int frame, int targetLayer, int duration) -> QVariantMap {
        return {{QStringLiteral("ok"), true}, {QStringLiteral("frame"), frame}, {QStringLiteral("layer"), targetLayer}, {QStringLiteral("duration"), duration}, {QStringLiteral("nextFrame"), frame + duration}};
    };

    if (m_timeline == nullptr) {
        return {{QStringLiteral("ok"), false}};
    }

    QUrl url(fileUrl);
    QString filePath = url.toLocalFile();
    if (filePath.isEmpty()) {
        filePath = fileUrl;
    }

    QFileInfo fileInfo(filePath);
    if (!fileInfo.exists()) {
        emit errorOccurred(tr("ファイルが見つかりません: %1").arg(filePath));
        return {{QStringLiteral("ok"), false}};
    }

    QString suffix = fileInfo.suffix().toLower();

    static const QSet<QString> audioExts = {QStringLiteral("wav"), QStringLiteral("mp3"), QStringLiteral("aac"), QStringLiteral("m4a"), QStringLiteral("flac"), QStringLiteral("ogg")};
    static const QSet<QString> imageExts = {QStringLiteral("png"), QStringLiteral("jpg"), QStringLiteral("jpeg"), QStringLiteral("bmp"), QStringLiteral("gif"), QStringLiteral("webp"), QStringLiteral("svg")};

    if (AviQtl::Core::MediaUtils::isVideoFile(filePath)) {
        m_timeline->undoStack()->beginMacro(tr("動画をインポート"));

        const double sceneFps = getSceneFps();
        const double probedSeconds = AviQtl::Core::MediaUtils::mediaDurationSeconds(filePath, AVMEDIA_TYPE_VIDEO);
        const int probedDuration = probedSeconds > 0.0 ? std::max(1, static_cast<int>(std::ceil(probedSeconds * sceneFps))) : 0;
        const int importDuration = probedDuration > 0 ? probedDuration : AviQtl::Core::SettingsManager::instance().value(QStringLiteral("defaultClipDuration"), AviQtl::kDefaultClipDuration).toInt();
        startFrame = findVacantFrameForLinkedMedia(m_timeline, layer, startFrame, importDuration);

        int videoClipId = m_timeline->nextClipId();
        m_timeline->setNextClipId(videoClipId + 1);
        m_timeline->undoStack()->push(new AddClipCommand(m_timeline, videoClipId, QStringLiteral("video"), startFrame, layer, tr("動画"), importDuration, QStringLiteral("video"),
                                                        {{QStringLiteral("path"), filePath}}));

        int audioClipId = m_timeline->nextClipId();
        m_timeline->setNextClipId(audioClipId + 1);
        m_timeline->undoStack()->push(new AddClipCommand(m_timeline, audioClipId, QStringLiteral("audio"), startFrame, layer + 1, tr("音声"), importDuration, QStringLiteral("audio"),
                                                        {{QStringLiteral("source"), filePath},
                                                         {QStringLiteral("linkedVideo"), true},
                                                         {QStringLiteral("speed"), AviQtl::kDefaultSpeed}}));

        m_timeline->undoStack()->endMacro();
        return editResult(startFrame, layer, importDuration);
    } else if (audioExts.contains(suffix)) {
        m_timeline->undoStack()->beginMacro(tr("音声をインポート"));

        const double sceneFps = getSceneFps();
        const double probedSeconds = AviQtl::Core::MediaUtils::mediaDurationSeconds(filePath, AVMEDIA_TYPE_AUDIO);
        const int probedDuration = probedSeconds > 0.0 ? std::max(1, static_cast<int>(std::ceil(probedSeconds * sceneFps))) : 0;
        const int importDuration = probedDuration > 0 ? probedDuration : AviQtl::Core::SettingsManager::instance().value(QStringLiteral("defaultClipDuration"), AviQtl::kDefaultClipDuration).toInt();
        startFrame = m_timeline->findVacantFrame(layer, startFrame, importDuration, -1);

        int clipId = m_timeline->nextClipId();
        m_timeline->setNextClipId(clipId + 1);
        m_timeline->undoStack()->push(new AddClipCommand(m_timeline, clipId, QStringLiteral("audio"), startFrame, layer, tr("音声"), importDuration, QStringLiteral("audio"),
                                                        {{QStringLiteral("source"), filePath}}));

        m_timeline->undoStack()->endMacro();
        return editResult(startFrame, layer, importDuration);
    } else if (imageExts.contains(suffix)) {
        m_timeline->undoStack()->beginMacro(tr("画像をインポート"));

        const int importDuration = AviQtl::Core::SettingsManager::instance().value(QStringLiteral("defaultClipDuration"), AviQtl::kDefaultClipDuration).toInt();
        startFrame = m_timeline->findVacantFrame(layer, startFrame, importDuration, -1);

        int clipId = m_timeline->nextClipId();
        m_timeline->setNextClipId(clipId + 1);
        m_timeline->undoStack()->push(new AddClipCommand(m_timeline, clipId, QStringLiteral("image"), startFrame, layer, tr("画像"), importDuration, QStringLiteral("image"),
                                                        {{QStringLiteral("path"), filePath}}));

        m_timeline->undoStack()->endMacro();
        return editResult(startFrame, layer, importDuration);
    } else {
        emit errorOccurred(tr("サポートされていないファイル形式です: %1").arg(suffix));
        return {{QStringLiteral("ok"), false}};
    }
}

auto TimelineController::getClipEffectsModel(int clipId) const -> QList<QObject *> {
    QList<QObject *> list;
    for (const auto &clip : m_timeline->clips()) {
        if (clip.id == clipId) {
            for (auto *eff : clip.effects) {
                if (clip.type == QLatin1String("audio") && eff->id() == QLatin1String("transform")) {
                    continue;
                }
                list.append(eff);
            }
            break;
        }
    }
    return list;
}

auto TimelineController::getClipEffectIndex(int clipId, QObject *effectModel) const -> int {
    if (effectModel == nullptr) {
        return -1;
    }
    for (const auto &clip : m_timeline->clips()) {
        if (clip.id != clipId) {
            continue;
        }
        int visibleIndex = 0;
        for (int i = 0; i < clip.effects.size(); ++i) {
            if (clip.type == QLatin1String("audio") && clip.effects.value(i)->id() == QLatin1String("transform")) {
                continue;
            }
            if (clip.effects.value(i) == effectModel) {
                return visibleIndex;
            }
            ++visibleIndex;
        }
        break;
    }
    return -1;
}

void TimelineController::updateClipEffectParam(int clipId, int effectIndex, const QString &paramName, const QVariant &value) { m_timeline->updateEffectParam(clipId, effectIndex, paramName, value); }

auto TimelineController::clips() const -> QVariantList {
    QVariantList list;
    list.reserve(m_timeline->clips().size());
    for (const auto &clip : m_timeline->clips()) {
        list.append(clipToVariantMap(clip));
    }
    return list;
}

auto TimelineController::clipsForViewport(int firstFrame, int lastFrame, int firstLayer, int lastLayer, const QVariantList &retainedIds) const -> QVariantList {
    const int normalizedFirstFrame = std::max(0, std::min(firstFrame, lastFrame));
    const int normalizedLastFrame = std::max(firstFrame, lastFrame);
    const int normalizedFirstLayer = std::max(0, std::min(firstLayer, lastLayer));
    const int normalizedLastLayer = std::max(firstLayer, lastLayer);

    QSet<int> retained;
    retained.reserve(retainedIds.size());
    for (const QVariant &id : retainedIds) {
        retained.insert(id.toInt());
    }

    QVariantList list;
    for (const auto &clip : m_timeline->clips()) {
        const int clipEnd = clip.startFrame + std::max(1, clip.durationFrames);
        const int clipLastLayer = clip.layer + getControlLayerCount(clip);
        const bool intersectsFrames = clip.startFrame < normalizedLastFrame && clipEnd > normalizedFirstFrame;
        const bool intersectsLayers = clip.layer <= normalizedLastLayer && clipLastLayer >= normalizedFirstLayer;
        if ((intersectsFrames && intersectsLayers) || retained.contains(clip.id)) {
            list.append(clipToVariantMap(clip));
        }
    }
    return list;
}

auto TimelineController::clipByUpperObject(int clipId) const -> bool { return m_timeline != nullptr ? m_timeline->clipByUpperObject(clipId) : false; }

void TimelineController::setClipByUpperObject(int clipId, bool enabled) {
    if (m_timeline != nullptr) {
        m_timeline->setClipByUpperObject(clipId, enabled);
    }
}

void TimelineController::moveSelectedClips(int deltaLayer, int deltaFrame) {
    if (m_timeline != nullptr) {
        m_timeline->moveSelectedClips(deltaLayer, deltaFrame);
    }
}

void TimelineController::applyClipBatchMove(const QVariantList &moves) {
    if (m_timeline != nullptr) {
        m_timeline->applyClipBatchMove(moves);
    }
}

void TimelineController::resizeSelectedClips(int deltaStartFrame, int deltaDuration) {
    if (m_timeline == nullptr) {
        return;
    }

    const QVariantList ids = m_selection->selectedClipIds();
    if (ids.isEmpty()) {
        return;
    }

    // リサイズ前の状態を値コピー（updateClip 呼び出しでポインタが失効しないよう）
    struct PendingResize {
        int id;
        int layer;
        int oldStart;
        int oldDuration;
    };

    QVector<PendingResize> pending;
    pending.reserve(ids.size());
    for (const QVariant &vId : std::as_const(ids)) {
        const int id = vId.toInt();
        const auto *clip = m_timeline->findClipById(id);
        if (clip == nullptr) {
            continue;
        }
        pending.push_back({id, clip->layer, clip->startFrame, clip->durationFrames});
    }
    if (pending.isEmpty()) {
        return;
    }

    if (deltaStartFrame > 0 || deltaDuration > 0) {
        std::ranges::sort(pending, [](const PendingResize &a, const PendingResize &b) { return a.oldStart != b.oldStart ? a.oldStart > b.oldStart : a.layer > b.layer; });
    } else {
        std::ranges::sort(pending, [](const PendingResize &a, const PendingResize &b) { return a.oldStart != b.oldStart ? a.oldStart < b.oldStart : a.layer < b.layer; });
    }

    m_timeline->undoStack()->beginMacro(tr("複数クリップリサイズ: %1").arg(pending.size()));
    for (const PendingResize &r : std::as_const(pending)) {
        const int newStart = std::max(0, r.oldStart + deltaStartFrame);
        const int newDuration = std::max(1, r.oldDuration + deltaDuration);
        updateClip(r.id, r.layer, newStart, newDuration);
    }
    m_timeline->undoStack()->endMacro();
}

int TimelineController::clampedDuration(int clipId, int newStart, int requestedDuration) const {
    Q_UNUSED(newStart);
    const auto *clip = m_timeline->findClipById(clipId);
    if (clip == nullptr) {
        return requestedDuration;
    }

    const int projectFps = static_cast<int>(project()->fps());

    if (clip->type == QLatin1String("video")) {
        return clampVideoDuration(clipId, requestedDuration, projectFps);
    }
    if (clip->type == QLatin1String("audio")) {
        return clampAudioDuration(clipId, requestedDuration, projectFps);
    }
    if (clip->type == QLatin1String("scene")) {
        return clampSceneDuration(clip, requestedDuration);
    }
    return requestedDuration;
}

int TimelineController::clampVideoDuration(int clipId, int requestedDuration, int projectFps) const {
    auto *vid = qobject_cast<AviQtl::Core::VideoDecoder *>(m_mediaManager->decoderForClip(clipId));
    if ((vid == nullptr) || !vid->isReady()) {
        return requestedDuration;
    }

    int startVideoFrame = 0;
    double speed = AviQtl::kDefaultSpeed;
    bool isDirectMode = false;

    const auto *clip = m_timeline->findClipById(clipId);
    for (const auto *eff : clip->effects) {
        if (eff->id() != QLatin1String("video")) {
            continue;
        }
        const auto &p = eff->params();
        const QString playMode = p.value(QStringLiteral("playMode"), "開始フレーム＋再生速度").toString();
        if (playMode == QStringLiteral("フレーム直接指定")) {
            isDirectMode = true;
            break;
        }
        startVideoFrame = p.value(QStringLiteral("startFrame"), 0).toInt();
        speed = p.value(QStringLiteral("speed"), AviQtl::kDefaultSpeed).toDouble();
        break;
    }

    double srcFps = vid->sourceFps();
    if (srcFps <= 0.0) {
        srcFps = projectFps;
    }
    int maxDuration = requestedDuration;

    if (isDirectMode) {
        const double totalSec = static_cast<double>(vid->totalFrameCount()) / srcFps;
        maxDuration = static_cast<int>(totalSec * projectFps);
    } else if (speed > 0.0) {
        const double startSec = static_cast<double>(startVideoFrame) / srcFps;
        const double remainingSec = (static_cast<double>(vid->totalFrameCount()) / srcFps) - startSec;
        if (remainingSec > 0.0) {
            maxDuration = static_cast<int>(remainingSec / (speed / AviQtl::kDefaultSpeed) * projectFps);
        }
    }
    if (maxDuration > 0 && requestedDuration > maxDuration) {
        return maxDuration;
    }
    return requestedDuration;
}

int TimelineController::clampAudioDuration(int clipId, int requestedDuration, int projectFps) const {
    auto *aud = qobject_cast<AviQtl::Core::AudioDecoder *>(m_mediaManager->decoderForClip(clipId));
    if ((aud == nullptr) || !aud->isReady()) {
        return requestedDuration;
    }

    double startTime = 0.0;
    double speed = AviQtl::kDefaultSpeed;
    bool isDirectMode = false;

    const auto *clip = m_timeline->findClipById(clipId);
    for (const auto *eff : clip->effects) {
        if (eff->id() != QLatin1String("audio")) {
            continue;
        }
        const auto &p = eff->params();
        const QString playMode = p.value(QStringLiteral("playMode"), "開始時間＋再生速度").toString();
        if (playMode == QStringLiteral("時間直接指定")) {
            isDirectMode = true;
            break;
        }
        startTime = p.value(QStringLiteral("startTime"), 0.0).toDouble();
        speed = p.value(QStringLiteral("speed"), AviQtl::kDefaultSpeed).toDouble();
        break;
    }

    const double totalSec = aud->totalDurationSec();
    int maxDuration = requestedDuration;

    if (isDirectMode) {
        maxDuration = static_cast<int>(totalSec * projectFps);
    } else if (speed > 0.0) {
        const double remainingSec = totalSec - startTime;
        if (remainingSec > 0.0) {
            maxDuration = static_cast<int>(remainingSec / (speed / AviQtl::kDefaultSpeed) * projectFps);
        }
    }
    if (maxDuration > 0 && requestedDuration > maxDuration) {
        return maxDuration;
    }
    return requestedDuration;
}

int TimelineController::clampSceneDuration(const ClipData *clip, int requestedDuration) const {
    int targetSceneId = 0;
    double speed = 1.0;
    int offset = 0;

    for (const auto *eff : clip->effects) {
        if (eff->id() != QLatin1String("scene")) {
            continue;
        }
        const auto &p = eff->params();
        targetSceneId = p.value(QStringLiteral("targetSceneId"), 0).toInt();
        speed = p.value(QStringLiteral("speed"), 1.0).toDouble();
        offset = p.value(QStringLiteral("offset"), 0).toInt();
        break;
    }

    const int sceneDur = getSceneDuration(targetSceneId);
    if (sceneDur > 0 && speed > 0.0) {
        const double rhs = (static_cast<double>(sceneDur - 1 - offset)) / speed;
        int maxDuration = std::max(static_cast<int>(rhs) + 1, 1);
        return std::min(requestedDuration, maxDuration);
    }
    return requestedDuration;
}

void TimelineController::updateClip(int id, int layer, int startFrame, int duration) {
    const auto *clip = m_timeline->findClipById(id);
    if (clip == nullptr) {
        return;
    }

    const int clamped = clampedDuration(id, startFrame, duration);
    m_timeline->updateClip(id, layer, startFrame, clamped);
}

void TimelineController::insertLayers(int targetLayer, int count, bool above) { m_timeline->insertLayers(targetLayer, count, above); }

void TimelineController::shiftLayers(int startLayer, int endLayer, int delta) { m_timeline->shiftLayers(startLayer, endLayer, delta); }

void TimelineController::selectClip(int id) {
    if (m_timeline != nullptr) {
        m_timeline->applySelectionIds(QVariantList{id});
    }
}

void TimelineController::toggleSelection(int id, const QVariantMap &data) {
    if (m_timeline != nullptr) {
        m_timeline->toggleSelection(id, data);
    }
}

void TimelineController::applySelectionIds(const QVariantList &ids) {
    if (m_timeline != nullptr) {
        m_timeline->applySelectionIds(ids);
    }
}

void TimelineController::addEffect(int clipId, const QString &effectId) {
    m_timeline->addEffect(clipId, effectId);
    // clipEffectsChanged signal handles sync
}

void TimelineController::removeEffect(int clipId, int effectIndex) {
    m_timeline->removeEffect(clipId, effectIndex);
    // clipEffectsChanged signal handles sync
}

void TimelineController::removeMultipleEffects(int clipId, const QList<int> &indices) {
    m_timeline->removeMultipleEffects(clipId, indices);
    // clipEffectsChanged signal handles sync
}

void TimelineController::setEffectEnabled(int clipId, int effectIndex, bool enabled) {
    if (m_timeline != nullptr) {
        m_timeline->setEffectEnabled(clipId, effectIndex, enabled);
    }
}

void TimelineController::reorderEffects(int clipId, int oldIndex, int newIndex) {
    if (m_timeline != nullptr) {
        m_timeline->reorderEffects(clipId, oldIndex, newIndex);
    }
}

void TimelineController::reorderMultipleEffects(int clipId, const QVariantList &indicesList, int targetIndex) {
    if (m_timeline != nullptr) {
        m_timeline->reorderMultipleEffects(clipId, indicesList, targetIndex);
    }
}

void TimelineController::pasteEffect(int clipId, int targetIndex) { m_timeline->pasteEffect(clipId, targetIndex); }

void TimelineController::addAudioPlugin(int clipId, const QString &pluginId) {
    auto plugin = AviQtl::Engine::Plugin::AudioPluginManager::instance().createPlugin(pluginId);
    if (plugin) {
        qInfo() << "Adding audio plugin:" << plugin->name() << "to clip" << clipId;
        AudioPluginState state;
        state.id = pluginId;
        state.enabled = true;
        for (int i = 0; i < plugin->paramCount(); ++i) {
            state.params.insert(QString::number(i), plugin->getParam(i));
        }
        m_timeline->addAudioPlugin(clipId, state, pluginId);
        m_mediaManager->syncAudioPluginChain(clipId);
        emit clipEffectsChanged(clipId);
    } else {
        qWarning() << "Failed to create audio plugin:" << pluginId;
    }
}

void TimelineController::removeAudioPlugin(int clipId, int index) {
    const auto *clip = m_timeline->findClipById(clipId);
    QString pluginName;
    if (clip != nullptr && index >= 0 && index < clip->audioPlugins.size()) {
        pluginName = clip->audioPlugins.at(index).id;
    }
    m_timeline->removeAudioPlugin(clipId, index, pluginName);
    m_mediaManager->syncAudioPluginChain(clipId);
    emit clipEffectsChanged(clipId);
}

void TimelineController::setAudioPluginEnabled(int clipId, int index, bool enabled) {
    if (m_timeline != nullptr) {
        m_timeline->setAudioPluginEnabled(clipId, index, enabled);
        m_mediaManager->syncAudioPluginChain(clipId);
    }
}

void TimelineController::reorderAudioPlugins(int clipId, int oldIndex, int newIndex) {
    if (m_timeline != nullptr) {
        m_timeline->reorderAudioPlugins(clipId, oldIndex, newIndex);
        m_mediaManager->syncAudioPluginChain(clipId);
    }
}

auto TimelineController::isAudioClip(int clipId) const -> bool {
    const auto *clip = m_timeline->findClipById(clipId);
    return (clip != nullptr) && clip->type == QLatin1String("audio");
}

auto TimelineController::getWaveformPeaks(int clipId, int pixelWidth, int displayDurationFrames) const -> QVariantList { // NOLINT(bugprone-easily-swappable-parameters)
    if (pixelWidth <= 0 || displayDurationFrames <= 0) {
        return {};
    }

    const auto *clip = m_timeline->findClipById(clipId);
    if ((clip == nullptr) || clip->type != QLatin1String("audio")) {
        return {};
    }

    auto *decoder = qobject_cast<AviQtl::Core::AudioDecoder *>((m_mediaManager != nullptr) ? m_mediaManager->decoderForClip(clipId) : nullptr);
    if ((decoder == nullptr) || !decoder->isReady()) {
        return QVariantList(pixelWidth * 2, 0.0);
    }

    double fps = m_project->fps();
    if (fps <= 0.0) {
        fps = AviQtl::kDefaultFps;
    }

    const EffectModel *audioEffect = nullptr;
    for (const auto *effect : clip->effects) {
        if (effect != nullptr && effect->id() == QLatin1String("audio")) {
            audioEffect = effect;
            break;
        }
    }

    const double frameStepSec = 1.0 / fps;
    const double clipDurationSec = static_cast<double>(displayDurationFrames) / fps;
    const QVariantMap params = audioEffect != nullptr ? audioEffect->params() : QVariantMap();
    const QString source = params.value(QStringLiteral("source")).toString();
    const bool sourceIsVideo = AviQtl::Core::MediaUtils::isVideoFile(source);
    const bool linkedVideo = sourceIsVideo && params.value(QStringLiteral("linkedVideo"), false).toBool();
    const QString playMode = params.value(QStringLiteral("playMode")).toString();
    const bool directMode = AviQtl::Core::MediaUtils::isDirectAudioMode(playMode);

    std::vector<float> rawPeaks;
    rawPeaks.reserve(static_cast<std::size_t>(pixelWidth) * 2);
    for (int i = 0; i < pixelWidth; ++i) {
        const int relFrame = std::clamp(static_cast<int>(std::floor(static_cast<double>(displayDurationFrames) * static_cast<double>(i) / static_cast<double>(pixelWidth))), 0, std::max(0, displayDurationFrames - 1));
        const int nextRelFrame = std::clamp(static_cast<int>(std::ceil(static_cast<double>(displayDurationFrames) * static_cast<double>(i + 1) / static_cast<double>(pixelWidth))), relFrame + 1, displayDurationFrames);
        const double relSec = static_cast<double>(relFrame) / fps;
        const double nextRelSec = static_cast<double>(nextRelFrame) / fps;

        double sourceStartSec = 0.0;
        double sourceDurationSec = frameStepSec;
        double volume = 1.0;
        double masterVolume = 1.0;
        double pan = 0.0;
        double fadeIn = 0.0;
        double fadeOut = 0.0;
        bool mute = false;

        if (audioEffect != nullptr) {
            volume = std::max(0.0, audioEffect->evaluatedParam(QStringLiteral("volume"), relFrame, fps).toDouble());
            masterVolume = std::max(0.0, audioEffect->evaluatedParam(QStringLiteral("masterVolume"), relFrame, fps).toDouble());
            pan = std::clamp(audioEffect->evaluatedParam(QStringLiteral("pan"), relFrame, fps).toDouble(), -1.0, 1.0);
            fadeIn = std::max(0.0, audioEffect->evaluatedParam(QStringLiteral("fadeIn"), relFrame, fps).toDouble());
            fadeOut = std::max(0.0, audioEffect->evaluatedParam(QStringLiteral("fadeOut"), relFrame, fps).toDouble());
            mute = audioEffect->evaluatedParam(QStringLiteral("mute"), relFrame, fps).toBool();

            if (directMode) {
                const double directTime = audioEffect->evaluatedParam(QStringLiteral("directTime"), relFrame, fps).toDouble();
                const double nextDirectTime = audioEffect->evaluatedParam(QStringLiteral("directTime"), nextRelFrame, fps).toDouble();
                sourceStartSec = std::min(directTime, nextDirectTime);
                sourceDurationSec = std::max(std::abs(nextDirectTime - directTime), frameStepSec);
            } else {
                const double startTime = std::max(0.0, audioEffect->evaluatedParam(QStringLiteral("startTime"), relFrame, fps).toDouble());
                const double speed = linkedVideo ? AviQtl::kDefaultSpeed : audioEffect->evaluatedParam(QStringLiteral("speed"), relFrame, fps).toDouble();
                const double sourceRate = std::max(0.0, speed / AviQtl::kDefaultSpeed);
                sourceStartSec = startTime + (relSec * sourceRate);
                sourceDurationSec = std::max((nextRelSec - relSec) * sourceRate, frameStepSec);
            }
        } else {
            sourceStartSec = relSec;
            sourceDurationSec = std::max(clipDurationSec / static_cast<double>(pixelWidth), frameStepSec);
        }

        const auto pixelPeaks = decoder->getPeaks(sourceStartSec, sourceDurationSec, 1);
        double fadeGain = 1.0;
        if (fadeIn > 0.0) {
            fadeGain = std::min(fadeGain, std::clamp(relSec / fadeIn, 0.0, 1.0));
        }
        if (fadeOut > 0.0) {
            fadeGain = std::min(fadeGain, std::clamp((clipDurationSec - relSec) / fadeOut, 0.0, 1.0));
        }
        const double outputVolume = volume * masterVolume * fadeGain;
        const double leftVol = mute ? 0.0 : outputVolume * (pan <= 0.0 ? 1.0 : 1.0 - pan);
        const double rightVol = mute ? 0.0 : outputVolume * (pan >= 0.0 ? 1.0 : 1.0 + pan);
        const float displayGain = static_cast<float>(std::clamp((leftVol + rightVol) * 0.5, 0.0, 2.0));
        if (pixelPeaks.size() >= 2) {
            rawPeaks.push_back(pixelPeaks[0] * displayGain);
            rawPeaks.push_back(pixelPeaks[1] * displayGain);
        } else {
            rawPeaks.push_back(0.0F);
            rawPeaks.push_back(0.0F);
        }
    }

    QVariantList result;
    result.reserve(static_cast<qsizetype>(rawPeaks.size()));
    for (float p : rawPeaks) {
        result.append(static_cast<double>(p));
    }

    return result;
}

auto TimelineController::getClipEffectStack(int clipId) const -> QVariantList {
    QVariantList list;
    if (clipId < 0) {
        return list;
    }

    auto chain = m_mediaManager->audioMixer()->getChain(clipId);
    for (int i = 0; i < chain->count(); ++i) {
        auto *plugin = chain->get(i);
        if (plugin != nullptr) {
            QVariantMap effectInfo;
            effectInfo.insert(QStringLiteral("name"), plugin->name());
            effectInfo.insert(QStringLiteral("format"), plugin->format());
            list.append(effectInfo);
        }
    }
    return list;
}

auto TimelineController::getEffectParameters(int clipId, int effectIndex) const -> QVariantList {
    QVariantList list;
    if (clipId < 0) {
        return list;
    }
    auto chain = m_mediaManager->audioMixer()->getChain(clipId);
    auto *plugin = chain->get(effectIndex);
    if (plugin != nullptr) {
        // Get keyframe tracks from AudioPluginState
        const auto *clip = m_timeline->findClipById(clipId);
        const QVariantMap *kfTracks = nullptr;
        if (clip != nullptr && effectIndex >= 0 && effectIndex < clip->audioPlugins.size()) {
            kfTracks = &clip->audioPlugins.at(effectIndex).keyframeTracks;
        }

        for (int i = 0; i < plugin->paramCount(); ++i) {
            QVariantMap paramInfo;
            auto info = plugin->getParamInfo(i);
            const QString paramKey = QString::number(i);

            paramInfo.insert(QStringLiteral("pIdx"), i);
            paramInfo.insert(QStringLiteral("pKey"), paramKey);
            paramInfo.insert(QStringLiteral("name"), info.name);
            paramInfo.insert(QStringLiteral("current"), plugin->getParam(i));
            paramInfo.insert(QStringLiteral("min"), info.min);
            paramInfo.insert(QStringLiteral("max"), info.max);

            if (info.isToggle) {
                paramInfo.insert(QStringLiteral("type"), "bool");
            } else if (info.isInteger) {
                paramInfo.insert(QStringLiteral("type"), "int");
            } else {
                paramInfo.insert(QStringLiteral("type"), "slider");
            }

            // Include keyframe track data if available
            if (kfTracks != nullptr && kfTracks->contains(paramKey)) {
                paramInfo.insert(QStringLiteral("hasKeyframes"), true);
            }

            list.append(paramInfo);
        }
    }
    return list;
}

void TimelineController::setEffectParameter(int clipId, int effectIndex, int paramIndex, float value) {
    if (clipId < 0) {
        return;
    }
    auto chain = m_mediaManager->audioMixer()->getChain(clipId);
    auto *plugin = chain->get(effectIndex); // NOLINT(bugprone-easily-swappable-parameters)
    if (plugin != nullptr) {
        plugin->setParam(paramIndex, value);
        m_timeline->setAudioPluginParam(clipId, effectIndex, paramIndex, value);
    }
}

void TimelineController::setKeyframe(int clipId, int effectIndex, const QString &paramName, int frame, const QVariant &value, const QVariantMap &options) { m_timeline->setKeyframe(clipId, effectIndex, paramName, frame, value, options); }

void TimelineController::removeKeyframe(int clipId, int effectIndex, const QString &paramName, int frame) { m_timeline->removeKeyframe(clipId, effectIndex, paramName, frame); }

void TimelineController::moveKeyframe(int clipId, int effectIndex, const QString &paramName, int oldFrame, int newFrame) { m_timeline->moveKeyframe(clipId, effectIndex, paramName, oldFrame, newFrame); }

void TimelineController::setAudioPluginKeyframe(int clipId, int pluginIndex, const QString &paramKey, int frame, const QVariant &value, const QVariantMap &options) { m_timeline->setAudioPluginKeyframe(clipId, pluginIndex, paramKey, frame, value, options); }

void TimelineController::removeAudioPluginKeyframe(int clipId, int pluginIndex, const QString &paramKey, int frame) { m_timeline->removeAudioPluginKeyframe(clipId, pluginIndex, paramKey, frame); }

void TimelineController::moveAudioPluginKeyframe(int clipId, int pluginIndex, const QString &paramKey, int oldFrame, int newFrame) { m_timeline->moveAudioPluginKeyframe(clipId, pluginIndex, paramKey, oldFrame, newFrame); }

auto TimelineController::audioPluginKeyframeListForUi(int clipId, int pluginIndex, const QString &paramKey) const -> QVariantList {
    const auto *clip = m_timeline->findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return {};
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);
    const QVariant raw = plugin.keyframeTracks.value(paramKey);
    if (raw.typeId() == QMetaType::QVariantMap && raw.toMap().contains(QStringLiteral("start"))) {
        QVariantMap track = raw.toMap();
        QVariantList flat;
        flat.append(track.value(QStringLiteral("start")).toMap());
        const QVariantList points = track.value(QStringLiteral("points")).toList();
        for (const auto &v : points) {
            flat.append(v);
        }
        std::sort(flat.begin(), flat.end(), [](const QVariant &a, const QVariant &b) { return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt(); });
        return flat;
    }
    QVariantList list = raw.toList();
    std::sort(list.begin(), list.end(), [](const QVariant &a, const QVariant &b) { return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt(); });
    return list;
}

auto TimelineController::audioPluginEvaluatedParam(int clipId, int pluginIndex, const QString &paramKey, int frame) const -> QVariant {
    const auto *clip = m_timeline->findClipById(clipId);
    if ((clip == nullptr) || pluginIndex < 0 || pluginIndex >= clip->audioPlugins.size()) {
        return {};
    }
    const auto &plugin = clip->audioPlugins.at(pluginIndex);
    const QVariant fallback = plugin.params.value(paramKey);
    const QVariant raw = plugin.keyframeTracks.value(paramKey);
    if (!raw.isValid()) {
        return fallback;
    }
    QVariantList flat;
    if (raw.typeId() == QMetaType::QVariantMap && raw.toMap().contains(QStringLiteral("start"))) {
        QVariantMap track = raw.toMap();
        flat.append(track.value(QStringLiteral("start")).toMap());
        const QVariantList points = track.value(QStringLiteral("points")).toList();
        for (const auto &v : points) {
            flat.append(v);
        }
        std::sort(flat.begin(), flat.end(), [](const QVariant &a, const QVariant &b) { return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt(); });
    } else {
        flat = raw.toList();
        std::sort(flat.begin(), flat.end(), [](const QVariant &a, const QVariant &b) { return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt(); });
    }
    if (flat.isEmpty()) {
        return fallback;
    }
    QVariantMap firstPoint = flat.first().toMap();
    if (frame <= firstPoint.value(QStringLiteral("frame")).toInt()) {
        return firstPoint.value(QStringLiteral("value"), fallback);
    }
    for (int i = 0; i < flat.size() - 1; ++i) {
        QVariantMap cur = flat[i].toMap();
        QVariantMap nxt = flat[i + 1].toMap();
        int f0 = cur.value(QStringLiteral("frame")).toInt();
        int f1 = nxt.value(QStringLiteral("frame")).toInt();
        if (frame >= f0 && frame <= f1) {
            if (frame == f0) {
                return cur.value(QStringLiteral("value"), fallback);
            }
            if (frame == f1) {
                return nxt.value(QStringLiteral("value"), fallback);
            }
            double t = static_cast<double>(frame - f0) / static_cast<double>(f1 - f0);
            double v0 = cur.value(QStringLiteral("value"), fallback).toDouble();
            double v1 = nxt.value(QStringLiteral("value"), fallback).toDouble();
            // Discrete types: hold current value until crossing the midpoint
            if (fallback.typeId() == QMetaType::Bool) {
                return t < 0.5 ? v0 : v1;
            }
            if (fallback.typeId() == QMetaType::Int || fallback.typeId() == QMetaType::LongLong) {
                return t < 0.5 ? std::floor(v0) : std::floor(v1);
            }
            return v0 + (v1 - v0) * t;
        }
    }
    return flat.last().toMap().value(QStringLiteral("value"), fallback);
}

void TimelineController::deleteClip(int clipId) { requestDelete(clipId); }

void TimelineController::requestDelete(int targetClipId) {
    if ((m_timeline == nullptr) || (m_selection == nullptr)) {
        return;
    }

    QVariantList selected = m_selection->selectedClipIds();

    // 選択が1件以上ある場合
    if (!selected.isEmpty()) {
        if (targetClipId < 0 || selected.contains(targetClipId)) {
            m_timeline->deleteClipsByIds(selected);
            return;
        }
    }

    if (targetClipId >= 0) {
        QVariantList ids{targetClipId};
        m_timeline->applySelectionIds(ids); // 内部的に選択状態を同期
        m_timeline->deleteClipsByIds(ids);
    }
}

void TimelineController::splitClip(int clipId, int frame) {
    if (m_timeline != nullptr) {
        m_timeline->splitClip(clipId, frame);
    }
}

void TimelineController::splitSelectedClips(int frame) {
    if (m_timeline != nullptr) {
        m_timeline->splitSelectedClips(frame);
    }
}

auto TimelineController::evaluateClipParams(int clipId, int relFrame) const -> QVariantMap {
    QVariantMap out;
    const auto *clip = m_timeline->findClipById(clipId);
    if (clip == nullptr) {
        return out;
    }

    const double fps = project() ? project()->fps() : AviQtl::kDefaultFps;

    for (auto *eff : clip->effects) {
        // 各エフェクトの評価済みパラメータを取得
        QVariantMap p = eff->evaluatedParams(relFrame, fps);
        out.insert(eff->id(), p);
        for (auto it = p.begin(); it != p.end(); ++it) {
            out.insert(it.key(), it.value());
        }
    }
    return out;
}
void TimelineController::copyClip(int clipId) { m_timeline->copyClip(clipId); }

void TimelineController::cutClip(int clipId) { m_timeline->cutClip(clipId); }

auto TimelineController::pasteClip(int frame, int layer) -> QVariantMap {
    if (m_timeline == nullptr) {
        return {{QStringLiteral("ok"), false}};
    }

    const int duration = m_timeline->getClipboardDuration();
    if (duration <= 0) {
        return {{QStringLiteral("ok"), false}};
    }

    const int actualFrame = m_timeline->pasteClip(frame, layer);
    const int actualLayer = std::max(layer, 0);
    return {{QStringLiteral("ok"), true}, {QStringLiteral("frame"), actualFrame}, {QStringLiteral("layer"), actualLayer}, {QStringLiteral("duration"), duration}, {QStringLiteral("nextFrame"), actualFrame + duration}};
}

void TimelineController::copySelectedClips() {
    if (m_timeline != nullptr) {
        m_timeline->copySelectedClips();
    }
}

void TimelineController::cutSelectedClips() {
    if (m_timeline != nullptr) {
        m_timeline->cutSelectedClips();
    }
}

void TimelineController::deleteSelectedClips() { requestDelete(-1); }

} // namespace AviQtl::UI
