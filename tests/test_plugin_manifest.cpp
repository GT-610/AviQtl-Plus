#include "mod_engine.hpp"
#include <QDir>
#include <QTemporaryDir>
#include <QTest>
#include <QTextStream>

using namespace AviQtl::Scripting;

class TestPluginManifest : public QObject {
    Q_OBJECT

  private slots:
    void validManifest();
    void missingId();
    void missingName();
    void missingVersion();
    void emptyFields();
    void noManifestFile();
    void invalidLua();
    void manifestValidity();
    void scriptParamsAllowBlankLinesAndTypedSelect();
};

void TestPluginManifest::validManifest() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile manifestFile(dir.path() + QStringLiteral("/manifest.lua"));
    QVERIFY(manifestFile.open(QIODevice::WriteOnly));
    QTextStream out(&manifestFile);
    out << R"(
return {
    id = "com.test.valid",
    name = "Valid Plugin",
    version = "1.0.0",
    author = "Test Author",
    description = "A valid test plugin",
    min_app_version = "0.2.0"
}
)";
    manifestFile.close();

    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(manifest.isValid());
    QCOMPARE(manifest.id, QStringLiteral("com.test.valid"));
    QCOMPARE(manifest.name, QStringLiteral("Valid Plugin"));
    QCOMPARE(manifest.version, QStringLiteral("1.0.0"));
    QCOMPARE(manifest.author, QStringLiteral("Test Author"));
    QCOMPARE(manifest.description, QStringLiteral("A valid test plugin"));
    QCOMPARE(manifest.minAppVersion, QStringLiteral("0.2.0"));
}

void TestPluginManifest::missingId() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile manifestFile(dir.path() + QStringLiteral("/manifest.lua"));
    QVERIFY(manifestFile.open(QIODevice::WriteOnly));
    QTextStream out(&manifestFile);
    out << R"(
return {
    name = "No ID Plugin",
    version = "1.0.0"
}
)";
    manifestFile.close();

    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(!manifest.isValid());
}

void TestPluginManifest::missingName() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile manifestFile(dir.path() + QStringLiteral("/manifest.lua"));
    QVERIFY(manifestFile.open(QIODevice::WriteOnly));
    QTextStream out(&manifestFile);
    out << R"(
return {
    id = "com.test.noname",
    version = "1.0.0"
}
)";
    manifestFile.close();

    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(!manifest.isValid());
}

void TestPluginManifest::missingVersion() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile manifestFile(dir.path() + QStringLiteral("/manifest.lua"));
    QVERIFY(manifestFile.open(QIODevice::WriteOnly));
    QTextStream out(&manifestFile);
    out << R"(
return {
    id = "com.test.noversion",
    name = "No Version Plugin"
}
)";
    manifestFile.close();

    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(!manifest.isValid());
}

void TestPluginManifest::emptyFields() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile manifestFile(dir.path() + QStringLiteral("/manifest.lua"));
    QVERIFY(manifestFile.open(QIODevice::WriteOnly));
    QTextStream out(&manifestFile);
    out << R"(
return {
    id = "",
    name = "",
    version = ""
}
)";
    manifestFile.close();

    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(!manifest.isValid());
}

void TestPluginManifest::noManifestFile() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    // No manifest.lua file created
    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(!manifest.isValid());
    QVERIFY(manifest.id.isEmpty());
}

void TestPluginManifest::invalidLua() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile manifestFile(dir.path() + QStringLiteral("/manifest.lua"));
    QVERIFY(manifestFile.open(QIODevice::WriteOnly));
    QTextStream out(&manifestFile);
    out << "this is not valid lua code {{{{";
    manifestFile.close();

    ModEngine &engine = ModEngine::instance();
    PluginManifest manifest = engine.loadManifest(dir.path());

    QVERIFY(!manifest.isValid());
}

void TestPluginManifest::manifestValidity() {
    PluginManifest empty;
    QVERIFY(!empty.isValid());

    PluginManifest partial;
    partial.id = QStringLiteral("com.test");
    QVERIFY(!partial.isValid());

    partial.name = QStringLiteral("Test");
    QVERIFY(!partial.isValid());

    partial.version = QStringLiteral("1.0.0");
    QVERIFY(partial.isValid());
}

void TestPluginManifest::scriptParamsAllowBlankLinesAndTypedSelect() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QFile scriptFile(dir.path() + QStringLiteral("/main.lua"));
    QVERIFY(scriptFile.open(QIODevice::WriteOnly));
    QTextStream out(&scriptFile);
    out << R"(-- Example plugin
-- Description line

--track@monitor_interval:Monitor Interval,1,120,60
--check@auto_play:Auto Play on Load,false
--select@log_level:Log Level=info,debug=0,info=1,warning=2,error=3

local loaded = true
)";
    scriptFile.close();

    ModEngine &engine = ModEngine::instance();
    ScriptMetadata meta = engine.loadScriptParams(scriptFile.fileName());

    QCOMPARE(meta.params.size(), 3);
    QCOMPARE(meta.params.at(0).varName, QStringLiteral("monitor_interval"));
    QCOMPARE(meta.params.at(0).defaultValue.toDouble(), AviQtl::kDefaultFps);
    QCOMPARE(meta.params.at(1).varName, QStringLiteral("auto_play"));
    QCOMPARE(meta.params.at(1).defaultValue.toBool(), false);
    QCOMPARE(meta.params.at(2).varName, QStringLiteral("log_level"));
    QCOMPARE(meta.params.at(2).defaultValue.toInt(), 1);
    QCOMPARE(meta.params.at(2).options.size(), 4);
    QCOMPARE(meta.params.at(2).options.at(0).value.toInt(), 0);
    QCOMPARE(meta.params.at(2).options.at(1).value.toInt(), 1);
    QCOMPARE(meta.params.at(2).options.at(2).value.toInt(), 2);
    QCOMPARE(meta.params.at(2).options.at(3).value.toInt(), 3);
}

QTEST_MAIN(TestPluginManifest)
#include "test_plugin_manifest.moc"
