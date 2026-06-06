#pragma once
#include <QObject>
#include <QVariantList>
#include <QVariantMap>

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

    mutable QVariantList m_cachedStates;
    mutable QVariantMap m_cachedEffectParams;
    mutable bool m_dirty = true;
};

} // namespace AviQtl::UI
