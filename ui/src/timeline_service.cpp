#include "timeline_service.hpp"
#include "commands.hpp"
#include "constants.hpp"
#include "effect_registry.hpp"
#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "settings_manager.hpp"
#include <QDebug>
#include <QPoint>
#include <algorithm>
#include <cstdint>
#include <utility>
#include <vector>

namespace AviQtl::UI {

TimelineService::TimelineService(SelectionService *selection, QObject *parent) : QObject(parent), m_undoStack(new QUndoStack(this)), m_selection(selection) {
    const auto &settings = AviQtl::Core::SettingsManager::instance().settings();
    m_undoStack->setUndoLimit(settings.value(QStringLiteral("undoCount"), 32).toInt());

    // 初期シーンを作成
    SceneData rootScene;
    rootScene.id = 0;
    rootScene.name = QObject::tr("ルート");
    rootScene.width = settings.value(QStringLiteral("defaultProjectWidth"), AviQtl::kDefaultWidth).toInt();
    rootScene.height = settings.value(QStringLiteral("defaultProjectHeight"), AviQtl::kDefaultHeight).toInt();
    rootScene.fps = settings.value(QStringLiteral("defaultProjectFps"), AviQtl::kDefaultFps).toDouble();
    m_scenes.append(rootScene);
}

TimelineService::~TimelineService() {
    for (auto &scene : m_scenes) {
        for (auto &clip : scene.clips) {
            for (auto *eff : clip.effects) {
                if (eff)
                    eff->deleteLater();
            }
        }
    }
    for (auto &clip : m_clipboard) {
        for (auto *eff : clip.effects) {
            if (eff)
                eff->deleteLater();
        }
    }
}

void TimelineService::undo() { m_undoStack->undo(); }
void TimelineService::redo() { m_undoStack->redo(); }

int TimelineService::allocateClipId() {
    const QList<int> ids = allocateClipIds(1);
    return ids.isEmpty() ? -1 : ids.first();
}

QList<int> TimelineService::allocateClipIds(qsizetype count) {
    if (count <= 0) {
        return {};
    }
    std::vector<std::int32_t> existingIds;
    for (const auto &scene : std::as_const(m_scenes)) {
        existingIds.reserve(existingIds.size() + static_cast<std::size_t>(scene.clips.size()));
        for (const auto &clip : std::as_const(scene.clips)) {
            existingIds.push_back(clip.id);
        }
    }

    QList<int> allocatedIds;
    allocatedIds.reserve(count);
    std::int32_t nextHint = m_nextClipId;
    for (qsizetype index = 0; index < count; ++index) {
        AviQtl::RustCore::IdAllocation allocation{};
        if (AviQtl::RustCore::allocateId(existingIds, nextHint, 1, allocation) !=
            AviQtl::RustCore::TimelineDomainStatus::Ok) {
            qWarning() << "Rust clip ID allocation failed";
            return {};
        }
        allocatedIds.append(allocation.allocated_id);
        existingIds.push_back(allocation.allocated_id);
        nextHint = allocation.next_id;
    }
    m_nextClipId = nextHint;
    return allocatedIds;
}

int TimelineService::allocateSceneId() {
    std::vector<std::int32_t> existingIds;
    existingIds.reserve(static_cast<std::size_t>(m_scenes.size()));
    for (const auto &scene : std::as_const(m_scenes)) {
        existingIds.push_back(scene.id);
    }
    AviQtl::RustCore::IdAllocation allocation{};
    if (AviQtl::RustCore::allocateId(existingIds, m_nextSceneId, 1, allocation) !=
        AviQtl::RustCore::TimelineDomainStatus::Ok) {
        qWarning() << "Rust scene ID allocation failed";
        return -1;
    }
    m_nextSceneId = allocation.next_id;
    return allocation.allocated_id;
}

} // namespace AviQtl::UI
