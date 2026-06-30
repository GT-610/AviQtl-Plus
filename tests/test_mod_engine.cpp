#include "mod_engine.hpp"
#include <QSignalSpy>
#include <QTest>

using namespace AviQtl::Scripting;

class TestModEngine : public QObject {
    Q_OBJECT

  private slots:
    void pluginManifestValid();
    void pluginManifestInvalidEmptyId();
    void pluginManifestInvalidEmptyName();
    void pluginManifestInvalidEmptyVersion();
    void pluginFileWatcherWatchPath();
    void pluginFileWatcherClearPaths();
};

void TestModEngine::pluginManifestValid() {
    PluginManifest manifest;
    manifest.id = "com.example.plugin";
    manifest.name = "Test Plugin";
    manifest.version = "1.0.0";
    QVERIFY(manifest.isValid());
}

void TestModEngine::pluginManifestInvalidEmptyId() {
    PluginManifest manifest;
    manifest.id = "";
    manifest.name = "Test Plugin";
    manifest.version = "1.0.0";
    QVERIFY(!manifest.isValid());
}

void TestModEngine::pluginManifestInvalidEmptyName() {
    PluginManifest manifest;
    manifest.id = "com.example.plugin";
    manifest.name = "";
    manifest.version = "1.0.0";
    QVERIFY(!manifest.isValid());
}

void TestModEngine::pluginManifestInvalidEmptyVersion() {
    PluginManifest manifest;
    manifest.id = "com.example.plugin";
    manifest.name = "Test Plugin";
    manifest.version = "";
    QVERIFY(!manifest.isValid());
}

void TestModEngine::pluginFileWatcherWatchPath() {
    PluginFileWatcher watcher;
    QSignalSpy spy(&watcher, &PluginFileWatcher::directoryChanged);

    watcher.watchPath("/tmp/test_path");
    QVERIFY(spy.isValid());
}

void TestModEngine::pluginFileWatcherClearPaths() {
    PluginFileWatcher watcher;
    watcher.watchPath("/tmp/path1");
    watcher.watchPath("/tmp/path2");
    watcher.clearPaths();
}

#include "test_mod_engine.moc"
QTEST_MAIN(TestModEngine)