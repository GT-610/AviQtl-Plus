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
    const bool previous = type == UpdateLayerStateCommand::Lock
                              ? it->lockedLayers.contains(layer)
                              : it->hiddenLayers.contains(layer);
    if (type == UpdateLayerStateCommand::Lock) {
        if (value) {
            it->lockedLayers.insert(layer);
        } else {
            it->lockedLayers.remove(layer);
        }
    } else {
        if (value) {
            it->hiddenLayers.insert(layer);
        } else {
            it->hiddenLayers.remove(layer);
        }
    }
    if (!commitTimelineMutation([this, sceneId, layer, type, previous]() {
            auto restored = std::ranges::find_if(
                m_scenes, [sceneId](const SceneData &scene) { return scene.id == sceneId; });
            if (restored == m_scenes.end()) {
                return;
            }
            QSet<int> &layers = type == UpdateLayerStateCommand::Lock
                                    ? restored->lockedLayers
                                    : restored->hiddenLayers;
            if (previous) {
                layers.insert(layer);
            } else {
                layers.remove(layer);
            }
        })) {
        qWarning() << "Rust rejected layer-state update";
        return;
    }
    emit layerStateChanged(layer);
}

} // namespace AviQtl::UI
