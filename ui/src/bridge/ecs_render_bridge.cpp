#include "bridge/ecs_render_bridge.hpp"
#include "engine/timeline/ecs.hpp"

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
        m[QStringLiteral("visible")] = rc.visible;
        m[QStringLiteral("clipByUpperObject")] = rc.clipByUpperObject;
        m[QStringLiteral("typeIndex")] = rc.typeIndex;
        m[QStringLiteral("effectCount")] = rc.effectCount;
        m[QStringLiteral("videoFrameKey")] = rc.videoFrameKey;
        states.append(m);
    });

    m_cachedStates = states;

    QVariantMap epMap;
    const auto &entries = snapshot->effectParams.entries;
    for (const auto &entry : entries) {
        const QString key = QString::number(entry.clipId) + QStringLiteral(":") + QString::number(entry.effectIndex);
        QVariantMap params = epMap.value(key).toMap();

        const QString paramName = QString::fromUtf8(entry.paramName);
        if (entry.paramType == AviQtl::Engine::Timeline::ParamType::Color) {
            QColor c;
            c.setRedF(static_cast<double>(entry.value[0]));
            c.setGreenF(static_cast<double>(entry.value[1]));
            c.setBlueF(static_cast<double>(entry.value[2]));
            c.setAlphaF(static_cast<double>(entry.value[3]));
            params[paramName] = c.name(QColor::HexArgb);
        } else {
            params[paramName] = static_cast<double>(entry.value[0]);
        }

        epMap[key] = params;
    }
    m_cachedEffectParams = epMap;

    m_dirty = false;
    return m_cachedStates;
}

QVariantMap ECSRenderBridge::getEffectParams(int clipId) const {
    if (m_dirty)
        renderStates();

    QVariantMap result;
    const QString prefix = QString::number(clipId) + QStringLiteral(":");
    for (auto it = m_cachedEffectParams.constBegin(); it != m_cachedEffectParams.constEnd(); ++it) {
        if (it.key().startsWith(prefix)) {
            const QVariantMap params = it.value().toMap();
            for (auto pit = params.constBegin(); pit != params.constEnd(); ++pit) {
                result[pit.key()] = pit.value();
            }
        }
    }
    return result;
}

void ECSRenderBridge::notifyFrameReady() {
    m_dirty = true;
    emit renderStatesChanged();
}

} // namespace AviQtl::UI
