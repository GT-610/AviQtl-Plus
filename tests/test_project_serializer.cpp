#include "project_serializer.hpp"
#include "project_service.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::Core;
using namespace AviQtl::UI;

class TestProjectSerializer : public QObject {
    Q_OBJECT

  private slots:
    void atomicSaveReplacesAnExistingProject();
    void saveFailureLeavesAnInvalidTargetUntouched();
};

void TestProjectSerializer::atomicSaveReplacesAnExistingProject() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString path = dir.filePath(QStringLiteral("project.aviqtl"));
    QFile original(path);
    QVERIFY(original.open(QIODevice::WriteOnly));
    QCOMPARE(original.write("not a project"), qint64(13));
    original.close();

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY2(ProjectSerializer::save(path, &timeline, &project, &error), qPrintable(error));

    QFile saved(path);
    QVERIFY(saved.open(QIODevice::ReadOnly));
    const QJsonDocument document = QJsonDocument::fromJson(saved.readAll());
    QVERIFY(document.isObject());
    QCOMPARE(document.object().value(QStringLiteral("version")).toInt(), 2);
}

void TestProjectSerializer::saveFailureLeavesAnInvalidTargetUntouched() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString directoryTarget = dir.filePath(QStringLiteral("not-a-project-file"));
    QVERIFY(QDir().mkpath(directoryTarget));

    SelectionService selection;
    TimelineService timeline(&selection);
    ProjectService project;
    QString error;
    QVERIFY(!ProjectSerializer::save(directoryTarget, &timeline, &project, &error));
    QVERIFY(!error.isEmpty());
    QVERIFY(QFileInfo(directoryTarget).isDir());
}

QTEST_MAIN(TestProjectSerializer)
#include "test_project_serializer.moc"
