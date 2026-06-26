#pragma once

#include <rhi/qshader.h>
#include <QString>

namespace AviQtl::Core {

class ShaderCompiler {
public:
    static bool compileToFile(const QString &sourcePath,
                              const QString &outputQsbPath,
                              QString *errorMessage = nullptr);

    static QShader compileSource(const QByteArray &glslSource,
                                 QShader::Stage stage,
                                 QString *errorMessage = nullptr);

    static bool needsRecompile(const QString &sourcePath,
                               const QString &cachedQsbPath);

    static QString ensureCompiled(const QString &sourcePath,
                                  const QString &cacheDir);
};

} // namespace AviQtl::Core
