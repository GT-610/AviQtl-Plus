#pragma once

#include <QDateTime>
#include <QFuture>
#include <QList>
#include <QString>
#include <functional>

namespace AviQtl::UI {
class ProjectService;
class TimelineService;

struct ProjectRecoveryEntry {
    QString id;
    QString snapshotPath;
    QString originalProjectUrl;
    QString displayName;
    QDateTime savedAt;
    bool valid = false;
    QString error;
};

struct ProjectRecoveryWriteResult {
    bool success = false;
    QString error;
};

enum class ProjectRecoveryWriteBarrierPoint { AsyncWriteStarted, SynchronousWaitStarted };

class ProjectRecoveryManager {
  public:
    static QString recoveryRoot();
    static void setRecoveryRootForTests(const QString &path);
    static void setWriteBarrierForTests(std::function<void(ProjectRecoveryWriteBarrierPoint)> barrier);

    static bool write(const QString &id, const QString &originalProjectUrl, const QString &displayName, const TimelineService *timeline, const ProjectService *project, QString *errorMessage = nullptr);
    static QFuture<ProjectRecoveryWriteResult> writeAsync(const QString &id, const QString &originalProjectUrl, const QString &displayName, const TimelineService *timeline, const ProjectService *project);
    static bool remove(const QString &id);
    static QList<ProjectRecoveryEntry> entries();
    static void cleanupStale(int maximumAgeDays = 30);

  private:
    friend class TimelineController;
    static void notifySynchronousWaitForTests();
};

} // namespace AviQtl::UI
