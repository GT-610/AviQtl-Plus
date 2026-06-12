#pragma once
#include "engine/timeline/ecs.hpp"
#include <QObject>
#include <QVariantList>
#include <QVariantMap>
#include <unordered_map>

namespace AviQtl::UI {

class ECSRenderBridge : public QObject {
    Q_OBJECT

    Q_PROPERTY(QVariantList renderStates READ renderStates NOTIFY renderStatesChanged)

  public:
    static ECSRenderBridge &instance();

    QVariantList renderStates() const;

    Q_INVOKABLE QVariantMap getEffectParams(int clipId) const;

    void notifyFrameReady();

  signals:
    void renderStatesChanged();

  private:
    ECSRenderBridge() = default;

    struct ClipParamIndex {
        uint32_t start = 0;
        uint32_t count = 0;
    };

    mutable QVariantList m_cachedStates;
    mutable std::vector<AviQtl::Engine::Timeline::EffectParamEntry> m_cachedEntries;
    mutable std::unordered_map<int, ClipParamIndex> m_clipParamIndex;
    mutable bool m_dirty = true;
};

} // namespace AviQtl::UI
