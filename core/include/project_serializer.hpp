#pragma once
#include <QString>
#include <QVariantMap>

namespace AviQtl::UI {
class TimelineService;
class ProjectService;
} // namespace AviQtl::UI

namespace AviQtl::Core {

class ProjectSerializer {
  public:
    static QVariantMap captureSnapshot(const UI::TimelineService *timeline, const UI::ProjectService *project);
    static auto saveSnapshot(const QString &fileUrl, const QVariantMap &snapshot, QString *errorMessage = nullptr) -> bool;
    static auto save(const QString &fileUrl, const UI::TimelineService *timeline, const UI::ProjectService *project, QString *errorMessage = nullptr) -> bool;
    static auto load(const QString &fileUrl, UI::TimelineService *timeline, UI::ProjectService *project, QString *errorMessage = nullptr) -> bool;
};

} // namespace AviQtl::Core
