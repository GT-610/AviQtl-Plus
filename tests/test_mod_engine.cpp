#include "mod_engine.hpp"
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::Scripting;

class TestModEngine : public QObject {
    Q_OBJECT

  private slots:
    void pluginFileWatcherWatchesAndClearsPaths();
};

void TestModEngine::pluginFileWatcherWatchesAndClearsPaths() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    PluginFileWatcher watcher;
    QSignalSpy spy(&watcher, &PluginFileWatcher::directoryChanged);
    QVERIFY(spy.isValid());
    watcher.watchPath(dir.path());

    QFile firstFile(dir.filePath(QStringLiteral("first.lua")));
    QVERIFY(firstFile.open(QIODevice::WriteOnly));
    QCOMPARE(firstFile.write("return {}"), 9);
    firstFile.close();

    QTRY_VERIFY_WITH_TIMEOUT(spy.count() > 0, 5'000);
    QCOMPARE(spy.last().at(0).toString(), dir.path());

    watcher.clearPaths();
    QTest::qWait(50);
    spy.clear();

    QFile secondFile(dir.filePath(QStringLiteral("second.lua")));
    QVERIFY(secondFile.open(QIODevice::WriteOnly));
    QCOMPARE(secondFile.write("return {}"), 9);
    secondFile.close();

    QTest::qWait(200);
    QCOMPARE(spy.count(), 0);
}

#include "test_mod_engine.moc"
QTEST_MAIN(TestModEngine)
