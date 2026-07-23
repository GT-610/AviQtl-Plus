#include "workspace.hpp"
#include "project_recovery_manager.hpp"
#include <QFileInfo>
#include <QUrl>
#include <algorithm>
#include <utility>

namespace AviQtl::UI {

// 起動時はタブなしで開始。プロジェクトランチャーが tabsChanged で自動起動する
Workspace::Workspace(QObject *parent) : QObject(parent) { ProjectRecoveryManager::cleanupStale(); }

void Workspace::setCurrentIndex(int index) {
    if (m_currentIndex == index || index < 0 || index >= m_timelines.size())
        return;
    m_currentIndex = index;
    emit currentIndexChanged();
    emit currentTimelineChanged();
}

TimelineController *Workspace::currentTimeline() const {
    if (m_currentIndex >= 0 && m_currentIndex < m_timelines.size()) {
        return m_timelines[m_currentIndex];
    }
    return nullptr;
}

QVariantList Workspace::tabs() const {
    QVariantList list;
    for (auto *tc : m_timelines) {
        QVariantMap map;
        QString url = tc->currentProjectUrl();
        QString name;
        if (url.isEmpty()) {
            name = tc->property("untitledName").toString();
        } else {
            QUrl qurl(url);
            name = QFileInfo(qurl.toLocalFile().isEmpty() ? url : qurl.toLocalFile()).fileName();
        }
        map["name"] = name;
        map["hasUnsavedChanges"] = tc->hasUnsavedChanges();
        list.append(map);
    }
    return list;
}

QVariantList Workspace::recoveries() const {
    QVariantList result;
    for (const ProjectRecoveryEntry &entry : ProjectRecoveryManager::entries()) {
        if (m_claimedRecoveryIds.contains(entry.id))
            continue;
        QVariantMap item;
        item.insert(QStringLiteral("id"), entry.id);
        item.insert(QStringLiteral("name"), entry.displayName.isEmpty() ? tr("Recovered project") : entry.displayName);
        item.insert(QStringLiteral("originalProjectUrl"), entry.originalProjectUrl);
        item.insert(QStringLiteral("savedAt"), entry.savedAt);
        item.insert(QStringLiteral("valid"), entry.valid);
        item.insert(QStringLiteral("error"), entry.error);
        result.append(item);
    }
    return result;
}

void Workspace::setVideoFrameStore(AviQtl::Core::VideoFrameStore *store) {
    m_videoFrameStore = store;
    for (auto *tc : m_timelines) {
        tc->setVideoFrameStore(store);
    }
}

void Workspace::newProject() {
    auto *tc = new TimelineController(this);
    if (m_videoFrameStore) {
        tc->setVideoFrameStore(m_videoFrameStore);
    }
    tc->setProperty("untitledName", QString("Untitled %1").arg(m_untitledCounter++));

    connect(tc, &TimelineController::currentProjectUrlChanged, this, &Workspace::onTabStateChanged);
    connect(tc, &TimelineController::hasUnsavedChangesChanged, this, &Workspace::onTabStateChanged);

    m_timelines.append(tc);
    emit tabsChanged();

    setCurrentIndex(m_timelines.size() - 1);
}

void Workspace::closeProject(int index) {
    if (index < 0 || index >= m_timelines.size())
        return;

    auto *tc = m_timelines.takeAt(index);
    m_claimedRecoveryIds.remove(tc->recoveryId());
    tc->discardRecovery();
    tc->deleteLater();
    emit recoveriesChanged();

    if (m_timelines.isEmpty()) {
        // タブが 0 になったら自動で newProject() せず tabsChanged を emit して
        // QML 側のランチャー起動に委ねる
        m_currentIndex = -1;
        emit currentTimelineChanged();
        emit tabsChanged();
    } else {
        if (m_currentIndex >= m_timelines.size()) {
            m_currentIndex = m_timelines.size() - 1;
        }
        emit currentIndexChanged();
        emit currentTimelineChanged();
        emit tabsChanged();
    }
}

void Workspace::loadProject(const QString &fileUrl) {
    auto *current = currentTimeline();
    if (current && current->currentProjectUrl().isEmpty() && !current->hasUnsavedChanges()) {
        current->loadProject(fileUrl);
    } else {
        newProject();
        currentTimeline()->loadProject(fileUrl);
    }
}

bool Workspace::recoverProject(const QString &recoveryId) {
    const auto allEntries = ProjectRecoveryManager::entries();
    const auto it = std::find_if(allEntries.cbegin(), allEntries.cend(), [&recoveryId](const ProjectRecoveryEntry &entry) { return entry.id == recoveryId; });
    if (it == allEntries.cend() || !it->valid)
        return false;

    newProject();
    TimelineController *controller = currentTimeline();
    if (!controller->loadRecovery(it->snapshotPath, it->id, it->originalProjectUrl)) {
        closeProject(m_currentIndex);
        return false;
    }
    controller->setProperty("untitledName", tr("Recovered - %1").arg(it->displayName));
    m_claimedRecoveryIds.insert(it->id);
    emit tabsChanged();
    emit recoveriesChanged();
    return true;
}

void Workspace::discardRecovery(const QString &recoveryId) {
    ProjectRecoveryManager::remove(recoveryId);
    m_claimedRecoveryIds.remove(recoveryId);
    emit recoveriesChanged();
}

void Workspace::discardAllRecoveries() {
    for (TimelineController *controller : std::as_const(m_timelines))
        controller->discardRecovery();
    m_claimedRecoveryIds.clear();
    emit recoveriesChanged();
}

void Workspace::onTabStateChanged() { emit tabsChanged(); }

} // namespace AviQtl::UI
