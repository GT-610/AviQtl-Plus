#include "commands.hpp"
#include "timeline_service.hpp"
#include <QDebug>
#include <bitset>

namespace AviQtl::UI {

auto TimelineService::isLayerLocked(int layer) const -> bool { return currentScene()->lockedLayers.contains(layer); }

auto TimelineService::isLayerHidden(int layer) const -> bool { return currentScene()->hiddenLayers.contains(layer); }

void TimelineService::setLayerState(int layer, bool value, int type) { m_undoStack->push(new UpdateLayerStateCommand(this, m_currentSceneId, layer, value, static_cast<UpdateLayerStateCommand::StateType>(type))); }

void TimelineService::setLayerStateInternal(int sceneId, int layer, bool value, int type) { // NOLINT(bugprone-easily-swappable-parameters)
    auto it = std::ranges::find_if(m_scenes, [sceneId](const SceneData &s) -> bool { return s.id == sceneId; });
    if (it == m_scenes.end()) {
        return;
    }
    SceneData updated = *it;
    QSet<int> &layers = type == UpdateLayerStateCommand::Lock ? updated.lockedLayers
                                                              : updated.hiddenLayers;
    if (value) {
        layers.insert(layer);
    } else {
        layers.remove(layer);
    }
    const QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("update_scene")},
        {QStringLiteral("scene_id"), sceneId},
        {QStringLiteral("scene"), timelineSceneDocument(updated)},
    };
    if (!commitTimelineStateMutation(request)) {
        qWarning() << "Rust rejected layer-state update";
        return;
    }
    emit layerStateChanged(layer);
}

} // namespace AviQtl::UI
