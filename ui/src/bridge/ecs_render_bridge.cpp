#include "bridge/ecs_render_bridge.hpp"
#include "core/include/performance_metrics.hpp"
#include "engine/timeline/ecs.hpp"
#include <QColor>
#include <QSet>
#include <algorithm>
#include <cstring>
#include <iterator>

namespace AviQtl::UI {

ECSRenderBridge &ECSRenderBridge::instance() {
    static ECSRenderBridge inst;
    return inst;
}

namespace {

bool equalRenderState(const AviQtl::Engine::Timeline::RenderComponent &a, const AviQtl::Engine::Timeline::RenderComponent &b) {
    return a.clipId == b.clipId && a.layer == b.layer && a.startFrame == b.startFrame && a.durationFrames == b.durationFrames &&
           a.x == b.x && a.y == b.y && a.z == b.z && a.rotX == b.rotX && a.rotY == b.rotY && a.rotZ == b.rotZ &&
           a.scaleX == b.scaleX && a.scaleY == b.scaleY && a.opacity == b.opacity && a.clipByUpperObject == b.clipByUpperObject &&
           a.effectCount == b.effectCount && a.effectStartIndex == b.effectStartIndex;
}

bool equalParamEntry(const AviQtl::Engine::Timeline::EffectParamEntry &left,
                     const AviQtl::Engine::Timeline::EffectParamEntry &right) {
    return left.clipId == right.clipId && left.effectIndex == right.effectIndex && left.paramType == right.paramType &&
           std::strncmp(left.paramName, right.paramName, sizeof(left.paramName)) == 0 &&
           std::equal(std::begin(left.value), std::end(left.value), std::begin(right.value));
}

QVariantMap renderStateToMap(const AviQtl::Engine::Timeline::RenderComponent &rc) {
    return {
        {QStringLiteral("clipId"), rc.clipId},
        {QStringLiteral("layer"), rc.layer},
        {QStringLiteral("startFrame"), rc.startFrame},
        {QStringLiteral("durationFrames"), rc.durationFrames},
        {QStringLiteral("x"), rc.x},
        {QStringLiteral("y"), rc.y},
        {QStringLiteral("z"), rc.z},
        {QStringLiteral("rotX"), rc.rotX},
        {QStringLiteral("rotY"), rc.rotY},
        {QStringLiteral("rotZ"), rc.rotZ},
        {QStringLiteral("scaleX"), rc.scaleX},
        {QStringLiteral("scaleY"), rc.scaleY},
        {QStringLiteral("opacity"), rc.opacity},
        {QStringLiteral("clipByUpperObject"), rc.clipByUpperObject},
        {QStringLiteral("effectCount"), rc.effectCount},
        {QStringLiteral("effectStartIndex"), rc.effectStartIndex},
    };
}

QVariantMap paramEntriesToMap(const QVector<AviQtl::Engine::Timeline::EffectParamEntry> &entries) {
    QVariantMap result;
    for (const auto &entry : entries) {
        const QString paramName = QString::fromUtf8(entry.paramName);
        if (entry.paramType == AviQtl::Engine::Timeline::ParamType::Color) {
            QColor color;
            color.setRedF(static_cast<double>(entry.value[0]));
            color.setGreenF(static_cast<double>(entry.value[1]));
            color.setBlueF(static_cast<double>(entry.value[2]));
            color.setAlphaF(static_cast<double>(entry.value[3]));
            result[paramName] = color.name(QColor::HexArgb);
        } else {
            result[paramName] = static_cast<double>(entry.value[0]);
        }
    }
    return result;
}

} // namespace

bool ECSRenderBridge::syncSnapshot() const {
    if (!m_dirty)
        return false;

    auto &metrics = AviQtl::Core::PerformanceMetrics::instance();
    metrics.add(AviQtl::Core::PerformanceCounter::EcsBridgeSyncs);
    AviQtl::Core::ScopedPerformanceTimer timer(AviQtl::Core::PerformanceCounter::EcsBridgeNanoseconds);

    const auto *snapshot = AviQtl::Engine::Timeline::ECS::instance().getSnapshot();
    if (!snapshot)
        return false;

    bool changed = false;
    bool renderStatesChanged = false;
    QSet<int> aliveClipIds;
    QVector<int> nextClipOrder;
    snapshot->renderStates.forEach([&](int clipId, const AviQtl::Engine::Timeline::RenderComponent &rc) {
        aliveClipIds.insert(clipId);
        nextClipOrder.append(clipId);
        auto componentIt = m_cachedComponents.find(clipId);
        if (componentIt == m_cachedComponents.end() || !equalRenderState(componentIt.value(), rc)) {
            m_cachedComponents.insert(clipId, rc);
            m_cachedStateValues.insert(clipId, renderStateToMap(rc));
            metrics.add(AviQtl::Core::PerformanceCounter::EcsBridgeStatesRebuilt);
            changed = true;
            renderStatesChanged = true;
        } else {
            metrics.add(AviQtl::Core::PerformanceCounter::EcsBridgeStatesReused);
        }
    });

    for (auto it = m_cachedComponents.begin(); it != m_cachedComponents.end();) {
        if (!aliveClipIds.contains(it.key())) {
            m_cachedStateValues.remove(it.key());
            it = m_cachedComponents.erase(it);
            changed = true;
            renderStatesChanged = true;
        } else {
            ++it;
        }
    }

    if (renderStatesChanged || nextClipOrder != m_cachedClipOrder) {
        QVariantMap nextStateMap;
        QVariantList nextStates;
        nextStates.reserve(nextClipOrder.size());
        for (const int clipId : std::as_const(nextClipOrder)) {
            const QVariantMap state = m_cachedStateValues.value(clipId);
            nextStates.append(state);
            nextStateMap.insert(QString::number(clipId), state);
        }
        m_cachedStates = std::move(nextStates);
        m_cachedStateMap = std::move(nextStateMap);
        m_cachedClipOrder = std::move(nextClipOrder);
        changed = true;
    }

    QSet<int> clipsWithParams;
    const auto &entries = snapshot->effectParams.entries;
    for (std::size_t begin = 0; begin < entries.size();) {
        const int clipId = static_cast<int>(entries[begin].clipId);
        std::size_t end = begin + 1;
        while (end < entries.size() && entries[end].clipId == entries[begin].clipId)
            ++end;
        clipsWithParams.insert(clipId);

        const auto cachedIt = m_cachedParamEntries.constFind(clipId);
        const auto rangeSize = static_cast<qsizetype>(end - begin);
        bool paramsChanged = cachedIt == m_cachedParamEntries.cend() || cachedIt->size() != rangeSize;
        if (!paramsChanged) {
            paramsChanged = !std::equal(entries.cbegin() + static_cast<qsizetype>(begin),
                                        entries.cbegin() + static_cast<qsizetype>(end), cachedIt->cbegin(),
                                        equalParamEntry);
        }
        if (paramsChanged) {
            QVector<AviQtl::Engine::Timeline::EffectParamEntry> nextEntries;
            nextEntries.reserve(rangeSize);
            for (std::size_t i = begin; i < end; ++i)
                nextEntries.append(entries[i]);
            m_cachedParamEntries.insert(clipId, nextEntries);
            m_cachedParamValues.insert(clipId, paramEntriesToMap(nextEntries));
            changed = true;
        }
        begin = end;
    }
    for (auto it = m_cachedParamEntries.begin(); it != m_cachedParamEntries.end();) {
        if (!clipsWithParams.contains(it.key())) {
            m_cachedParamValues.remove(it.key());
            it = m_cachedParamEntries.erase(it);
            changed = true;
        } else {
            ++it;
        }
    }

    m_dirty = false;
    return changed;
}

QVariantList ECSRenderBridge::renderStates() const {
    m_pendingChange |= syncSnapshot();
    return m_cachedStates;
}

QVariantMap ECSRenderBridge::renderStateMap() const {
    m_pendingChange |= syncSnapshot();
    return m_cachedStateMap;
}

QVariantMap ECSRenderBridge::getRenderState(int clipId) const {
    m_pendingChange |= syncSnapshot();
    return m_cachedStateValues.value(clipId);
}

QVariantMap ECSRenderBridge::getEffectParams(int clipId) const {
    m_pendingChange |= syncSnapshot();
    return m_cachedParamValues.value(clipId);
}

void ECSRenderBridge::notifyFrameReady() {
    m_dirty = true;
    m_pendingChange |= syncSnapshot();
    if (m_pendingChange) {
        m_pendingChange = false;
        ++m_renderRevision;
        emit renderStatesChanged();
    }
}

} // namespace AviQtl::UI
