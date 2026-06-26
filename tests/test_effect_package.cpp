#include "effect_registry.hpp"
#include "shader_compiler.hpp"
#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QTest>

using namespace AviQtl::Core;

class TestEffectPackage : public QObject {
    Q_OBJECT

  private slots:
    void init();
    void loadEffectFromSubdirectory();
    void loadMultipleEffectsFromPackage();
    void skipEffectWithMissingQml();
    void shaderCompiledOnLoad();

  private:
    QTemporaryDir m_dir;
    bool writeJson(const QString &relativePath, const QByteArray &content);
    bool writeQml(const QString &relativePath);
    bool writeFrag(const QString &relativePath);
};

void TestEffectPackage::init() {
    QVERIFY(m_dir.isValid());
    QDir dir(m_dir.path());
    for (const auto &entry : dir.entryList(QDir::Files)) {
        dir.remove(entry);
    }
    for (const auto &entry : dir.entryList(QDir::Dirs | QDir::NoDotAndDotDot)) {
        QDir(dir.filePath(entry)).removeRecursively();
    }
}

bool TestEffectPackage::writeJson(const QString &relativePath, const QByteArray &content) {
    QFile f(m_dir.path() + QLatin1Char('/') + relativePath);
    if (!f.open(QIODevice::WriteOnly))
        return false;
    f.write(content);
    f.close();
    return true;
}

bool TestEffectPackage::writeQml(const QString &relativePath) {
    QFile f(m_dir.path() + QLatin1Char('/') + relativePath);
    if (!f.open(QIODevice::WriteOnly))
        return false;
    f.write("import QtQuick; Item {}");
    f.close();
    return true;
}

bool TestEffectPackage::writeFrag(const QString &relativePath) {
    QFile f(m_dir.path() + QLatin1Char('/') + relativePath);
    if (!f.open(QIODevice::WriteOnly))
        return false;
    f.write(QByteArrayLiteral(R"(#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf { mat4 qt_Matrix; float qt_Opacity; };
layout(binding=1) uniform sampler2D source;
void main() { fragColor = texture(source, qt_TexCoord0) * qt_Opacity; }
)"));
    f.close();
    return true;
}

void TestEffectPackage::loadEffectFromSubdirectory() {
    // Simulate effect-packages/stylize-effects/glitch/ structure
    QDir(m_dir.path()).mkpath("stylize-effects/glitch");
    writeJson("stylize-effects/glitch/glitch.json", R"({
        "id": "pkg.test.glitch",
        "name": "Test Glitch",
        "qml": "Glitch.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Stylize"],
        "params": {"intensity": 0.5},
        "ui": {"controls": [{"type": "slider", "param": "intensity", "label": "Intensity"}]}
    })");
    writeQml("stylize-effects/glitch/Glitch.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    const auto &meta = reg.getEffect("pkg.test.glitch");
    QCOMPARE(meta.id, QStringLiteral("pkg.test.glitch"));
    QCOMPARE(meta.kind, QStringLiteral("effect"));
    QVERIFY(!meta.qmlSource.isEmpty());
}

void TestEffectPackage::loadMultipleEffectsFromPackage() {
    // Simulate a package with multiple effects in subdirectories
    QDir(m_dir.path()).mkpath("advanced-blur/lens_blur");
    QDir(m_dir.path()).mkpath("advanced-blur/radial_blur");

    writeJson("advanced-blur/lens_blur/lens_blur.json", R"({
        "id": "pkg.test.lens_blur",
        "name": "Test Lens Blur",
        "qml": "LensBlur.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Blur"],
        "params": {"radius": 10},
        "ui": {"controls": [{"type": "float", "param": "radius", "label": "Radius"}]}
    })");
    writeQml("advanced-blur/lens_blur/LensBlur.qml");

    writeJson("advanced-blur/radial_blur/radial_blur.json", R"({
        "id": "pkg.test.radial_blur",
        "name": "Test Radial Blur",
        "qml": "RadialBlur.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Blur"],
        "params": {"strength": 0.1},
        "ui": {"controls": [{"type": "float", "param": "strength", "label": "Strength"}]}
    })");
    writeQml("advanced-blur/radial_blur/RadialBlur.qml");

    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    QVERIFY(reg.getEffect("pkg.test.lens_blur").id == "pkg.test.lens_blur");
    QVERIFY(reg.getEffect("pkg.test.radial_blur").id == "pkg.test.radial_blur");
}

void TestEffectPackage::skipEffectWithMissingQml() {
    QDir(m_dir.path()).mkpath("bad_pkg");
    writeJson("bad_pkg/bad.json", R"({
        "id": "pkg.test.bad",
        "name": "Bad Effect",
        "qml": "Nonexistent.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {},
        "ui": {"controls": [{"type": "header", "label": "X"}]}
    })");
    // No QML file written

    EffectRegistry &reg = EffectRegistry::instance();
    int before = reg.getAllEffects().size();
    reg.loadEffectsFromDirectory(m_dir.path());
    int after = reg.getAllEffects().size();
    QCOMPARE(before, after);
}

void TestEffectPackage::shaderCompiledOnLoad() {
    QDir(m_dir.path()).mkpath("test_pkg");
    writeJson("test_pkg/test.json", R"({
        "id": "pkg.test.shader",
        "name": "Shader Test",
        "qml": "ShaderTest.qml",
        "version": "1.0.0",
        "kind": "effect",
        "categories": ["Test"],
        "params": {"intensity": 0.5},
        "ui": {"controls": [{"type": "slider", "param": "intensity", "label": "Intensity"}]}
    })");
    writeQml("test_pkg/ShaderTest.qml");
    writeFrag("test_pkg/test.frag");

    // Before loading, no .qsb should exist
    QVERIFY(!QFile::exists(m_dir.path() + "/test_pkg/test.frag.qsb"));

    // Loading triggers shader compilation via effect_registry
    EffectRegistry &reg = EffectRegistry::instance();
    reg.loadEffectsFromDirectory(m_dir.path());

    // After loading, .qsb should be compiled
    QVERIFY(QFile::exists(m_dir.path() + "/test_pkg/test.frag.qsb"));
}

QTEST_MAIN(TestEffectPackage)
#include "test_effect_package.moc"
