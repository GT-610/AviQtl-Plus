#include "shader_compiler.hpp"
#include <rhi/qshaderbaker.h>
#include <QFile>
#include <QFileInfo>
#include <QLoggingCategory>

// Ensure GuiPrivate headers are available
#include <QtGui/private/qtguiglobal_p.h>

Q_LOGGING_CATEGORY(lcShaderCompiler, "aviqtl.shader_compiler")

namespace AviQtl::Core {

QShader ShaderCompiler::compileSource(const QByteArray &glslSource,
                                       QShader::Stage stage,
                                       QString *errorMessage) {
    QShaderBaker baker;
    baker.setSourceString(glslSource, stage);
    baker.setGeneratedShaderVariants({QShader::StandardShader});

    QList<QShaderBaker::GeneratedShader> targets;
    targets.append({QShader::SpirvShader, QShaderVersion(100)});

    if (stage == QShader::ComputeStage) {
        // Compute shaders require GLSL 310es/430 (matching CMake qsb settings)
        targets.append({QShader::GlslShader, QShaderVersion(310, QShaderVersion::GlslEs)});
        targets.append({QShader::GlslShader, QShaderVersion(430)});
    } else {
        targets.append({QShader::GlslShader, QShaderVersion(100, QShaderVersion::GlslEs)});
        targets.append({QShader::GlslShader, QShaderVersion(120)});
        targets.append({QShader::GlslShader, QShaderVersion(150)});
    }

    targets.append({QShader::HlslShader, QShaderVersion(50)});
    targets.append({QShader::MslShader, QShaderVersion(12)});
    baker.setGeneratedShaders(targets);

    QShader shader = baker.bake();
    if (!shader.isValid() && errorMessage) {
        *errorMessage = baker.errorMessage();
    }
    return shader;
}

bool ShaderCompiler::compileToFile(const QString &sourcePath,
                                    const QString &outputQsbPath,
                                    QString *errorMessage) {
    QFile sourceFile(sourcePath);
    if (!sourceFile.open(QIODevice::ReadOnly)) {
        if (errorMessage) {
            *errorMessage = QStringLiteral("Cannot open source file: ") + sourcePath;
        }
        return false;
    }

    QShader::Stage stage = QShader::FragmentStage;
    if (sourcePath.endsWith(QStringLiteral(".vert"))) {
        stage = QShader::VertexStage;
    } else if (sourcePath.endsWith(QStringLiteral(".comp"))) {
        stage = QShader::ComputeStage;
    }

    QShader shader = compileSource(sourceFile.readAll(), stage, errorMessage);
    if (!shader.isValid()) {
        return false;
    }

    QFile outFile(outputQsbPath);
    if (!outFile.open(QIODevice::WriteOnly)) {
        if (errorMessage) {
            *errorMessage = QStringLiteral("Cannot write output: ") + outputQsbPath;
        }
        return false;
    }
    outFile.write(shader.serialized());

    qCDebug(lcShaderCompiler).noquote() << "Compiled:" << sourcePath << "->" << outputQsbPath;
    return true;
}

bool ShaderCompiler::needsRecompile(const QString &sourcePath,
                                     const QString &cachedQsbPath) {
    QFileInfo sourceInfo(sourcePath);
    QFileInfo cachedInfo(cachedQsbPath);

    if (!cachedInfo.exists()) {
        return true;
    }
    return sourceInfo.lastModified() > cachedInfo.lastModified();
}

QString ShaderCompiler::ensureCompiled(const QString &sourcePath,
                                        const QString &cacheDir) {
    QString baseName = QFileInfo(sourcePath).fileName() + QStringLiteral(".qsb");
    QString cachedPath = cacheDir + QLatin1Char('/') + baseName;

    if (!needsRecompile(sourcePath, cachedPath)) {
        return cachedPath;
    }

    QString error;
    if (compileToFile(sourcePath, cachedPath, &error)) {
        return cachedPath;
    }

    qCWarning(lcShaderCompiler).noquote() << "Failed to compile" << sourcePath << ":" << error;
    return {};
}

} // namespace AviQtl::Core
