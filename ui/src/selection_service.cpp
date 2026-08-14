#include "selection_service.hpp"
#include "rust_timeline_domain.hpp"
#include <cstdint>
#include <vector>

namespace AviQtl::UI {

SelectionService::SelectionService(QObject *parent) : QObject(parent) {}

auto SelectionService::selectedClipId() const -> int { return m_selectedClipId; }
auto SelectionService::selectedClipData() const -> QVariantMap { return m_selectedClipData; }
auto SelectionService::selectedClipIds() const -> QVariantList { return idsAsVariantList(); }

auto SelectionService::idsAsVariantList() const -> QVariantList {
    QVariantList ids;
    ids.reserve(m_selectedClipIds.size());
    for (int id : m_selectedClipIds) {
        ids.append(id);
    }
    return ids;
}

auto SelectionService::isSelected(int id) const -> bool { return m_selectedClipIds.contains(id); }

void SelectionService::updatePrimarySelection(int id, const QVariantMap &data) {
    if (m_selectedClipId != id) {
        m_selectedClipId = id;
        emit selectedClipIdChanged();
    }
    m_selectedClipData = data;
    emit selectedClipDataChanged();
}

void SelectionService::clearSelection() {
    const bool hadIds = !m_selectedClipIds.isEmpty();
    m_selectedClipIds.clear();
    if (hadIds) {
        emit selectedClipIdsChanged();
    }
    updatePrimarySelection(-1, QVariantMap());
}

void SelectionService::select(int id, const QVariantMap &data) { replaceSelection(QVariantList{id}, id, data); }

void SelectionService::toggleSelection(int id, const QVariantMap &data) {
    std::vector<std::int32_t> currentIds;
    currentIds.reserve(static_cast<std::size_t>(m_selectedClipIds.size()));
    for (int selectedId : std::as_const(m_selectedClipIds)) {
        currentIds.push_back(selectedId);
    }
    const bool wasSelected = m_selectedClipIds.contains(id);
    const int previousPrimary = m_selectedClipId;
    std::vector<std::int32_t> planned;
    std::int32_t nextPrimary = previousPrimary;
    if (AviQtl::RustCore::toggleSelection(currentIds, previousPrimary, id, planned, nextPrimary) !=
        AviQtl::RustCore::TimelineDomainStatus::Ok) {
        return;
    }

    const QList<int> nextIds(planned.cbegin(), planned.cend());
    if (m_selectedClipIds != nextIds) {
        m_selectedClipIds = nextIds;
        emit selectedClipIdsChanged();
    }
    if (nextPrimary != previousPrimary) {
        const QVariantMap nextData = !wasSelected && nextPrimary == id ? data : QVariantMap();
        updatePrimarySelection(nextPrimary, nextData);
    }
}

void SelectionService::refreshSelectionData(int id, const QVariantMap &data) {
    if (id < 0) {
        return;
    }
    if (!m_selectedClipIds.contains(id)) {
        return;
    }
    if (m_selectedClipId == id) {
        updatePrimarySelection(id, data);
    }
}

void SelectionService::replaceSelection(const QVariantList &ids, int primaryId, const QVariantMap &primaryData) {
    std::vector<std::int32_t> requested;
    requested.reserve(static_cast<std::size_t>(ids.size()));
    for (const QVariant &value : ids) {
        requested.push_back(value.toInt());
    }
    std::vector<std::int32_t> planned;
    std::int32_t nextPrimary = primaryId;
    if (AviQtl::RustCore::replaceSelection(requested, primaryId, planned, nextPrimary) !=
        AviQtl::RustCore::TimelineDomainStatus::Ok) {
        return;
    }
    const QList<int> nextIds(planned.cbegin(), planned.cend());
    if (m_selectedClipIds != nextIds) {
        m_selectedClipIds = nextIds;
        emit selectedClipIdsChanged();
    }
    updatePrimarySelection(nextPrimary, nextPrimary >= 0 ? primaryData : QVariantMap());
}
} // namespace AviQtl::UI
