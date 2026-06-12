#include "bridge/ecs_render_bridge.hpp"
#include "engine/timeline/ecs.hpp"
#include <QColor>

namespace AviQtl::UI {

ECSRenderBridge &ECSRenderBridge::instance() {
    static ECSRenderBridge inst;
    return inst;
}

QVariantList ECSRenderBridge::renderStates() const {
    if (!m_dirty)
        return m_cachedStates;

    const auto *snapshot = AviQtl::Engine::Timeline::ECS::instance().getSnapshot();
    if (!snapshot)
        return m_cachedStates;

    QVariantList states;
    snapshot->renderStates.forEach([&states](int clipId, const AviQtl::Engine::Timeline::RenderComponent &rc) {
        QVariantMap m;
        m[QStringLiteral("clipId")] = rc.clipId;
        m[QStringLiteral("layer")] = rc.layer;
        m[QStringLiteral("startFrame")] = rc.startFrame;
        m[QStringLiteral("durationFrames")] = rc.durationFrames;
        m[QStringLiteral("x")] = rc.x;
        m[QStringLiteral("y")] = rc.y;
        m[QStringLiteral("z")] = rc.z;
        m[QStringLiteral("rotX")] = rc.rotX;
        m[QStringLiteral("rotY")] = rc.rotY;
        m[QStringLiteral("rotZ")] = rc.rotZ;
        m[QStringLiteral("scaleX")] = rc.scaleX;
        m[QStringLiteral("scaleY")] = rc.scaleY;
        m[QStringLiteral("opacity")] = rc.opacity;
        m[QStringLiteral("clipByUpperObject")] = rc.clipByUpperObject;
        m[QStringLiteral("effectCount")] = rc.effectCount;
        m[QStringLiteral("effectStartIndex")] = rc.effectStartIndex;
        states.append(m);
    });

    m_cachedStates = states;

    // Cache sorted entries and build per-clip param index
    m_cachedEntries = snapshot->effectParams.entries;
    m_clipParamIndex.clear();
    for (std::size_t i = 0; i < m_cachedEntries.size(); ++i) {
        const uint32_t cid = m_cachedEntries[i].clipId;
        auto it = m_clipParamIndex.find(static_cast<int>(cid));
        if (it == m_clipParamIndex.end()) {
            m_clipParamIndex[static_cast<int>(cid)] = {static_cast<uint32_t>(i), 1};
        } else {
            it->second.count++;
        }
    }

    m_dirty = false;
    return m_cachedStates;
}

QVariantMap ECSRenderBridge::getEffectParams(int clipId) const {
    if (m_dirty)
        renderStates();

    QVariantMap result;

    auto idxIt = m_clipParamIndex.find(clipId);
    if (idxIt == m_clipParamIndex.end())
        return result;

    const uint32_t start = idxIt->second.start;
    const uint32_t end = start + idxIt->second.count;

    for (uint32_t i = start; i < end && i < m_cachedEntries.size(); ++i) {
        const auto &entry = m_cachedEntries[i];
        if (entry.clipId != static_cast<uint32_t>(clipId))
            break;

        const QString paramName = QString::fromUtf8(entry.paramName);
        if (entry.paramType == AviQtl::Engine::Timeline::ParamType::Color) {
            QColor c;
            c.setRedF(static_cast<double>(entry.value[0]));
            c.setGreenF(static_cast<double>(entry.value[1]));
            c.setBlueF(static_cast<double>(entry.value[2]));
            c.setAlphaF(static_cast<double>(entry.value[3]));
            result[paramName] = c.name(QColor::HexArgb);
        } else {
            result[paramName] = static_cast<double>(entry.value[0]);
        }
    }

    return result;
}

void ECSRenderBridge::notifyFrameReady() {
    m_dirty = true;
    emit renderStatesChanged();
}

} // namespace AviQtl::UI
