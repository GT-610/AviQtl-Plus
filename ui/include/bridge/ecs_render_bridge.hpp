#pragma once
#include "engine/timeline/ecs.hpp"
#include <QHash>
#include <QObject>
#include <QVariantList>
#include <QVariantMap>
#include <QVector>

namespace AviQtl::UI {

class ECSRenderBridge : public QObject {
    Q_OBJECT

    Q_PROPERTY(QVariantList renderStates READ renderStates NOTIFY renderStatesChanged)
    Q_PROPERTY(QVariantMap renderStateMap READ renderStateMap NOTIFY renderStatesChanged)
    Q_PROPERTY(quint64 renderRevision READ renderRevision NOTIFY renderStatesChanged)

  public:
    static ECSRenderBridge &instance();

    QVariantList renderStates() const;
    QVariantMap renderStateMap() const;
    quint64 renderRevision() const { return m_renderRevision; }

    Q_INVOKABLE QVariantMap getRenderState(int clipId) const;
    Q_INVOKABLE QVariantMap getEffectParams(int clipId) const;

    void notifyFrameReady();

  signals:
    void renderStatesChanged();

  private:
    ECSRenderBridge() = default;

    bool syncSnapshot() const;

    mutable QVariantList m_cachedStates;
    mutable QVariantMap m_cachedStateMap;
    mutable QVector<int> m_cachedClipOrder;
    mutable QHash<int, AviQtl::Engine::Timeline::RenderComponent> m_cachedComponents;
    mutable QHash<int, QVariantMap> m_cachedStateValues;
    mutable QHash<int, QVector<AviQtl::Engine::Timeline::EffectParamEntry>> m_cachedParamEntries;
    mutable QHash<int, QVariantMap> m_cachedParamValues;
    mutable bool m_dirty = true;
    mutable bool m_pendingChange = false;
    mutable quint64 m_renderRevision = 0;
};

} // namespace AviQtl::UI
