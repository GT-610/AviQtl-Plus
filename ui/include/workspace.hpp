#pragma once
#include "timeline_controller.hpp"
#include "video_frame_store.hpp"
#include <QList>
#include <QObject>
#include <QVariantList>
#include <QSet>

namespace AviQtl::UI {

class Workspace : public QObject {
    Q_OBJECT
    Q_PROPERTY(int currentIndex READ currentIndex WRITE setCurrentIndex NOTIFY currentIndexChanged)
    Q_PROPERTY(AviQtl::UI::TimelineController *currentTimeline READ currentTimeline NOTIFY currentTimelineChanged)
    Q_PROPERTY(QVariantList tabs READ tabs NOTIFY tabsChanged)
    Q_PROPERTY(QVariantList recoveries READ recoveries NOTIFY recoveriesChanged)

  public:
    explicit Workspace(QObject *parent = nullptr);

    void setVideoFrameStore(AviQtl::Core::VideoFrameStore *store);
    AviQtl::Core::VideoFrameStore *videoFrameStore() const { return m_videoFrameStore; }

    int currentIndex() const { return m_currentIndex; }
    void setCurrentIndex(int index);

    TimelineController *currentTimeline() const;

    QVariantList tabs() const;
    QVariantList recoveries() const;

    Q_INVOKABLE void newProject();
    Q_INVOKABLE void closeProject(int index);
    Q_INVOKABLE void loadProject(const QString &fileUrl);
    Q_INVOKABLE bool recoverProject(const QString &recoveryId);
    Q_INVOKABLE void discardRecovery(const QString &recoveryId);
    Q_INVOKABLE void discardAllRecoveries();

  signals:
    void currentIndexChanged();
    void currentTimelineChanged();
    void tabsChanged();
    void recoveriesChanged();

  private slots:
    void onTabStateChanged();

  private:
    QList<TimelineController *> m_timelines;
    int m_currentIndex = -1;
    int m_untitledCounter = 1;
    AviQtl::Core::VideoFrameStore *m_videoFrameStore = nullptr;
    QSet<QString> m_claimedRecoveryIds;
};

} // namespace AviQtl::UI
