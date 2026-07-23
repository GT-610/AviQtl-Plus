#pragma once

#include <QDateTime>
#include <QList>
#include <QString>

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

class ProjectRecoveryManager {
  public:
    static QString recoveryRoot();
    static void setRecoveryRootForTests(const QString &path);

    static bool write(const QString &id, const QString &originalProjectUrl, const QString &displayName, const TimelineService *timeline, const ProjectService *project, QString *errorMessage = nullptr);
    static bool remove(const QString &id);
    static QList<ProjectRecoveryEntry> entries();
    static void cleanupStale(int maximumAgeDays = 30);
};

} // namespace AviQtl::UI
