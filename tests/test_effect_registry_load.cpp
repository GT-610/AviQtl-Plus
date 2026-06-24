#include "effect_registry.hpp"
#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::Core;

class TestEffectRegistryLoad : public QObject {
    Q_OBJECT

  private slots:
    void init();
    void loadValidEffect();
    void loadValidObject();
    void loadValidTransition();
    void skipMissingId();
    void skipMissingName();
    void skipMissingQml();
    void skipMissingControls();
    void skipInvalidVersion();
    void skipInvalidKind();
    void skipEmptyCategories();
    void skipMissingQmlFile();
    void loadFromSubdirectory();
    void loadFromNonexistentDir();
    void colorFieldPreserved();
    void defaultParamsPreserved();

  private:
    QTemporaryDir m_dir;
    bool writeJson(const QString &relativePath, const QByteArray &content);
    bool writeQml(const QString &relativePath);
};

void TestEffectRegistryLoad::init() {
    // Clean up all files in temp dir between tests
    QDir dir(m_dir.path());
    for (const auto &entry : dir.entryList(QDir::Files)) {
        dir.remove(entry);
    }
    for (const auto &entry : dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot)) {
        QDir(dir.filePath(entry)).removeRecursively();
    }
}

bool TestEffectRegistryLoad::writeJson(const QString &relativePath, const QByteArray &content) {
    QFile f(m_dir.path() + QLatin1Char('/') + relativePath);
    if (!f.open(QIODevice::WriteOnly))
        return false;
    f.write(content);
    f.close();
    return true;
}

bool TestEffectRegistryLoad::writeQml(const QString &relativePath) {
    QFile f(m_dir.path() + QLatin1Char('/') + relativePath);
    if (!f.open(QIODevice::WriteOnly))
        return false;
    f.write("import QtQuick; Item {}");
    f.close();
    return true;
}

void TestEffectRegistryLoad::loadValidEffect() {
    writeJson("test_effect.json", R"({
        "id": "load.test.effect",
        "name": "Test Effect",
        "qml": "TestEffect.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {"intensity": 0.5},
        "ui": {"controls": [{"type": "slider", "param": "intensity", "label": "Intensity"}]}
    })");
    writeQml("TestEffect.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("load.test.effect");
    QCOMPARE(meta.id, QStringLiteral("load.test.effect"));
    QCOMPARE(meta.name, QStringLiteral("Test Effect"));
    QCOMPARE(meta.version, QStringLiteral("1.0.0"));
    QCOMPARE(meta.kind, QStringLiteral("effect"));
    QCOMPARE(meta.categories.size(), 1);
    QCOMPARE(meta.categories[0], QStringLiteral("Test"));
    QVERIFY(!meta.qmlSource.isEmpty());
}

void TestEffectRegistryLoad::loadValidObject() {
    writeJson("test_object.json", R"({
        "id": "load.test.object",
        "name": "Test Object",
        "qml": "TestObject.qml",
        "version": "2.0.0",
        "kind": "object",
        "categories": ["Custom"],
        "params": {"sizeW": 200},
        "ui": {"controls": [{"type": "float", "param": "sizeW", "label": "Width"}]}
    })");
    writeQml("TestObject.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("load.test.object");
    QCOMPARE(meta.kind, QStringLiteral("object"));
}

void TestEffectRegistryLoad::loadValidTransition() {
    writeJson("test_transition.json", R"({
        "id": "load.test.transition",
        "name": "Test Transition",
        "qml": "TestTransition.qml",
        "version": "1.0.0",
        "kind": "transition",
        "categories": ["Basic"],
        "params": {"duration": 30},
        "ui": {"controls": [{"type": "spinner", "param": "duration", "label": "Duration"}]}
    })");
    writeQml("TestTransition.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("load.test.transition");
    QCOMPARE(meta.kind, QStringLiteral("transition"));
}

void TestEffectRegistryLoad::skipMissingId() {
    writeJson("no_id.json", R"({
        "name": "No ID",
        "qml": "NoId.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    writeQml("NoId.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    int after = reg.getAllEffects().size();
    QCOMPARE(before, after);
}

void TestEffectRegistryLoad::skipMissingName() {
    writeJson("no_name.json", R"({
        "id": "noname",
        "qml": "NoName.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    writeQml("NoName.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::skipMissingQml() {
    writeJson("no_qml.json", R"({
        "id": "noqml",
        "name": "No QML",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::skipMissingControls() {
    writeJson("no_controls.json", R"({
        "id": "nocontrols",
        "name": "No Controls",
        "qml": "NoControls.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {}
    })");
    writeQml("NoControls.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::skipInvalidVersion() {
    writeJson("bad_version.json", R"({
        "id": "badversion",
        "name": "Bad Version",
        "qml": "BadVersion.qml",
        "version": "1.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    writeQml("BadVersion.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::skipInvalidKind() {
    writeJson("bad_kind.json", R"({
        "id": "badkind",
        "name": "Bad Kind",
        "qml": "BadKind.qml",
        "version": "1.0.0",
        "kind": "invalid",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    writeQml("BadKind.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::skipEmptyCategories() {
    writeJson("no_cat.json", R"({
        "id": "nocat",
        "name": "No Categories",
        "qml": "NoCat.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": [],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    writeQml("NoCat.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::skipMissingQmlFile() {
    writeJson("missing_qml.json", R"({
        "id": "missingqml",
        "name": "Missing QML",
        "qml": "DoesNotExist.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    // Don't create DoesNotExist.qml

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::loadFromSubdirectory() {
    QDir sub(m_dir.path());
    sub.mkdir("sub");
    QString subPath = sub.filePath("sub");

    writeJson("sub/nested_effect.json", R"({
        "id": "load.nested",
        "name": "Nested",
        "qml": "Nested.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");

    QFile qmlFile(subPath + "/Nested.qml");
    QVERIFY(qmlFile.open(QIODevice::WriteOnly));
    qmlFile.write("import QtQuick; Item {}");
    qmlFile.close();

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("load.nested");
    QCOMPARE(meta.id, QStringLiteral("load.nested"));
}

void TestEffectRegistryLoad::loadFromNonexistentDir() {
    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory("/nonexistent/path/that/does/not/exist");
    QCOMPARE(reg.getAllEffects().size(), before);
}

void TestEffectRegistryLoad::colorFieldPreserved() {
    writeJson("colored.json", R"({
        "id": "load.colored",
        "name": "Colored",
        "qml": "Colored.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]},
        "color": "#ff0000"
    })");
    writeQml("Colored.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("load.colored");
    QCOMPARE(meta.color, QStringLiteral("#ff0000"));
}

void TestEffectRegistryLoad::defaultParamsPreserved() {
    writeJson("params.json", R"({
        "id": "load.params",
        "name": "With Params",
        "qml": "Params.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {"alpha": 0.8, "count": 5, "enabled": true},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    writeQml("Params.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("load.params");
    QCOMPARE(meta.defaultParams["alpha"].toDouble(), 0.8);
    QCOMPARE(meta.defaultParams["count"].toInt(), 5);
    QCOMPARE(meta.defaultParams["enabled"].toBool(), true);
}

QTEST_MAIN(TestEffectRegistryLoad)
#include "test_effect_registry_load.moc"
