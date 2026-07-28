#include "shader_compiler.hpp"
#include <QDir>
#include <QFile>
#include <QTemporaryDir>
#include <QTest>
#include <QThread>

using namespace AviQtl::Core;

class TestShaderCompiler : public QObject {
    Q_OBJECT

  private slots:
    void compileFragmentSource();
    void compileComputeSource();
    void compileInvalidSource();
    void compileToFile();
    void failedCompilePreservesExistingOutput();
    void compileToFileInvalidPath();
    void needsRecompileWhenMissing();
    void needsRecompileWhenStale();
    void needsRecompileWhenFresh();
    void ensureCompiledCaches();
    void ensureCompiledRecompiles();

  private:
    static QByteArray simpleFragmentShader();
    static QByteArray simpleComputeShader();
    static QByteArray invalidShader();
};

QByteArray TestShaderCompiler::simpleFragmentShader() {
    return QByteArrayLiteral(R"(#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
};
layout(binding=1) uniform sampler2D source;
void main() {
    fragColor = texture(source, qt_TexCoord0) * qt_Opacity;
}
)");
}

QByteArray TestShaderCompiler::simpleComputeShader() {
    return QByteArrayLiteral(R"(#version 430
layout(local_size_x = 8, local_size_y = 8) in;
layout(binding = 0, rgba8) uniform coherent image2D outImage;
layout(binding = 1) uniform sampler2D inTex;
void main() {
    ivec2 gid = ivec2(gl_GlobalInvocationID.xy);
    ivec2 sz = imageSize(outImage);
    if (gid.x >= sz.x || gid.y >= sz.y) return;
    vec2 uv = (vec2(gid) + 0.5) / vec2(sz);
    imageStore(outImage, gid, texture(inTex, uv));
}
)");
}

QByteArray TestShaderCompiler::invalidShader() {
    return QByteArrayLiteral("this is not valid GLSL");
}

void TestShaderCompiler::compileFragmentSource() {
    QString error;
    QShader shader = ShaderCompiler::compileSource(simpleFragmentShader(), QShader::FragmentStage, &error);
    QVERIFY2(shader.isValid(), qPrintable(error));
    QVERIFY(!shader.serialized().isEmpty());
}

void TestShaderCompiler::compileComputeSource() {
    QString error;
    QShader shader = ShaderCompiler::compileSource(simpleComputeShader(), QShader::ComputeStage, &error);
    QVERIFY2(shader.isValid(), qPrintable(error));
    QVERIFY(!shader.serialized().isEmpty());
}

void TestShaderCompiler::compileInvalidSource() {
    QString error;
    QShader shader = ShaderCompiler::compileSource(invalidShader(), QShader::FragmentStage, &error);
    QVERIFY(!shader.isValid());
    QVERIFY(!error.isEmpty());
}

void TestShaderCompiler::compileToFile() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QString srcPath = dir.path() + "/test.frag";
    QString outPath = dir.path() + "/test.frag.qsb";

    QFile src(srcPath);
    QVERIFY(src.open(QIODevice::WriteOnly));
    src.write(simpleFragmentShader());
    src.close();

    QFile previous(outPath);
    QVERIFY(previous.open(QIODevice::WriteOnly));
    QCOMPARE(previous.write("previous-cache"), qint64(14));
    previous.close();

    QString error;
    bool ok = ShaderCompiler::compileToFile(srcPath, outPath, &error);
    QVERIFY2(ok, qPrintable(error));
    QVERIFY(QFile::exists(outPath));

    QFile out(outPath);
    QVERIFY(out.open(QIODevice::ReadOnly));
    QVERIFY(out.size() > 0);
    QVERIFY(out.readAll() != QByteArrayLiteral("previous-cache"));
}

void TestShaderCompiler::failedCompilePreservesExistingOutput() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    const QString srcPath = dir.path() + QStringLiteral("/invalid.frag");
    const QString outPath = dir.path() + QStringLiteral("/invalid.frag.qsb");
    QFile src(srcPath);
    QVERIFY(src.open(QIODevice::WriteOnly));
    QCOMPARE(src.write(invalidShader()), invalidShader().size());
    src.close();

    const QByteArray previousCache = QByteArrayLiteral("known-good-cache");
    QFile out(outPath);
    QVERIFY(out.open(QIODevice::WriteOnly));
    QCOMPARE(out.write(previousCache), previousCache.size());
    out.close();

    QString error;
    QVERIFY(!ShaderCompiler::compileToFile(srcPath, outPath, &error));
    QVERIFY(!error.isEmpty());
    QVERIFY(out.open(QIODevice::ReadOnly));
    QCOMPARE(out.readAll(), previousCache);
}

void TestShaderCompiler::compileToFileInvalidPath() {
    QString error;
    bool ok = ShaderCompiler::compileToFile("/nonexistent/test.frag", "/tmp/out.qsb", &error);
    QVERIFY(!ok);
    QVERIFY(!error.isEmpty());
}

void TestShaderCompiler::needsRecompileWhenMissing() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    QVERIFY(ShaderCompiler::needsRecompile(dir.path() + "/src.frag", dir.path() + "/out.frag.qsb"));
}

void TestShaderCompiler::needsRecompileWhenStale() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QString srcPath = dir.path() + "/src.frag";
    QString qsbPath = dir.path() + "/src.frag.qsb";

    // Create .qsb file first
    QFile qsb(qsbPath);
    QVERIFY(qsb.open(QIODevice::WriteOnly));
    qsb.write("old_qsb");
    qsb.close();

    // Ensure timestamp separation on coarse filesystems
    QThread::msleep(50);

    // Create source file (newer than .qsb)
    QFile src(srcPath);
    QVERIFY(src.open(QIODevice::WriteOnly));
    src.write("new");
    src.close();

    QVERIFY(ShaderCompiler::needsRecompile(srcPath, qsbPath));
}

void TestShaderCompiler::needsRecompileWhenFresh() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QString srcPath = dir.path() + "/src.frag";
    QString qsbPath = dir.path() + "/src.frag.qsb";

    // Create source first
    QFile src(srcPath);
    QVERIFY(src.open(QIODevice::WriteOnly));
    src.write("src");
    src.close();

    // Create .qsb (newer)
    QFile qsb(qsbPath);
    QVERIFY(qsb.open(QIODevice::WriteOnly));
    qsb.write("qsb");
    qsb.close();

    QVERIFY(!ShaderCompiler::needsRecompile(srcPath, qsbPath));
}

void TestShaderCompiler::ensureCompiledCaches() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QString srcPath = dir.path() + "/test.frag";
    QFile src(srcPath);
    QVERIFY(src.open(QIODevice::WriteOnly));
    src.write(simpleFragmentShader());
    src.close();

    // First call should compile
    QString result1 = ShaderCompiler::ensureCompiled(srcPath, dir.path());
    QVERIFY(!result1.isEmpty());
    QVERIFY(QFile::exists(result1));

    // Second call should use cache (no recompile needed)
    QString result2 = ShaderCompiler::ensureCompiled(srcPath, dir.path());
    QCOMPARE(result1, result2);
}

void TestShaderCompiler::ensureCompiledRecompiles() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());

    QString srcPath = dir.path() + "/test.comp";
    QFile src(srcPath);
    QVERIFY(src.open(QIODevice::WriteOnly));
    src.write(simpleComputeShader());
    src.close();

    QString result1 = ShaderCompiler::ensureCompiled(srcPath, dir.path());
    QVERIFY(!result1.isEmpty());
    QDateTime mtime1 = QFileInfo(result1).lastModified();

    // Modify source to trigger recompile
    QThread::msleep(50);
    QFile src2(srcPath);
    QVERIFY(src2.open(QIODevice::WriteOnly));
    src2.write(simpleComputeShader());
    src2.close();

    QString result2 = ShaderCompiler::ensureCompiled(srcPath, dir.path());
    QVERIFY(!result2.isEmpty());
    QCOMPARE(result1, result2); // Same output path

    // Verify recompilation actually happened (file was regenerated)
    QDateTime mtime2 = QFileInfo(result2).lastModified();
    QVERIFY(mtime2 > mtime1);
}

QTEST_MAIN(TestShaderCompiler)
#include "test_shader_compiler.moc"
