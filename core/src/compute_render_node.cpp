#include "compute_render_node.hpp"
#include <QColor>
#include <QCoreApplication>
#include <QFile>
#include <QLoggingCategory>
#include <QPointF>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <cstring>
#include <mutex>
#include <rhi/qrhi.h>
#include <rhi/qshaderdescription.h>

Q_LOGGING_CATEGORY(lcComputeRenderNode, "aviqtl.compute_render_node")

namespace AviQtl::UI::Effects {

namespace {
static constexpr int kOutputBinding = 0;
static constexpr int kInputBinding = 1;
static constexpr int kParamsBinding = 2;
static constexpr int kExtraBindingBase = 3;
static constexpr int kParamsBlockSize = 32;

static constexpr float kQuadData[] = {
    0.0f, 0.0f, 0.0f, 0.0f, 0.0f, 1.0f, 0.0f, 1.0f, 1.0f, 0.0f, 1.0f, 0.0f, 1.0f, 1.0f, 1.0f, 1.0f,
};

static QShader s_cachedBlitVert;
static QShader s_cachedBlitFrag;
static std::once_flag s_blitInitFlag;

static void loadBlitShaders() {
    QString baseDir = QCoreApplication::applicationDirPath();
    if (baseDir.endsWith("/bin"))
        baseDir.chop(4);
    const QString shaderDir = baseDir + "/common/shaders/";

    QFile vfile(shaderDir + "blit.vert.qsb");
    if (vfile.open(QIODevice::ReadOnly))
        s_cachedBlitVert = QShader::fromSerialized(vfile.readAll());

    QFile ffile(shaderDir + "blit.frag.qsb");
    if (ffile.open(QIODevice::ReadOnly))
        s_cachedBlitFrag = QShader::fromSerialized(ffile.readAll());
}

static bool ensureBlitShaders() {
    std::call_once(s_blitInitFlag, loadBlitShaders);
    return s_cachedBlitVert.isValid() && s_cachedBlitFrag.isValid();
}
} // namespace

ComputeRenderNode::ComputeRenderNode(QQuickWindow *window) : m_window(window) {}

ComputeRenderNode::~ComputeRenderNode() { destroyResources(); }

void ComputeRenderNode::syncParams(const QVariantMap &params) {
    if (m_params == params)
        return;
    m_params = params;
    m_paramsDirty = true;
}

void ComputeRenderNode::syncShaderPath(const QString &path) {
    if (m_shaderPath == path)
        return;
    m_shaderPath = path;
    m_shaderDirty = true;
    m_bufferLayoutDirty = true;
}

void ComputeRenderNode::syncSize(float w, float h) {
    if (qFuzzyCompare(m_width, w) && qFuzzyCompare(m_height, h))
        return;
    qCDebug(lcComputeRenderNode) << "ComputeRenderNode: Size changed to" << w << "x" << h;
    m_width = w;
    m_height = h;
}

void ComputeRenderNode::syncInputTexture(QSGTexture *tex) {
    if (m_inputTexture == tex)
        return;
    qCDebug(lcComputeRenderNode) << "ComputeRenderNode: inputTexture updated to" << tex;
    m_inputTexture = tex;
    m_bufferLayoutDirty = true;
    m_renderTargetDirty = true;
}

void ComputeRenderNode::syncWorkGroupSize(int x, int y, int z) {
    m_workGroupX = qMax(1, x);
    m_workGroupY = qMax(1, y);
    m_workGroupZ = qMax(1, z);
}

void ComputeRenderNode::syncHdrOutput(bool hdr) {
    if (m_hdrOutput == hdr)
        return;
    m_hdrOutput = hdr;
    m_renderTargetDirty = true;
    m_texturesDirty = true;
}

void ComputeRenderNode::syncOpacity(qreal opacity) {
    m_opacity = qBound(0.0, opacity, 1.0);
}

void ComputeRenderNode::syncExtraTextures(const QList<QSGTexture *> &textures) {
    if (m_extraTextures == textures)
        return;
    m_extraTextures = textures;
    m_bufferLayoutDirty = true;
    m_passSrbDirty = true;
}

void ComputeRenderNode::syncDispatchCount(int count) {
    const int clamped = qMax(1, count);
    if (m_dispatchCount == clamped)
        return;
    m_dispatchCount = clamped;
    m_texturesDirty = true;
    m_passSrbDirty = true;
}

QRectF ComputeRenderNode::rect() const { return QRectF(0, 0, m_width, m_height); }

QRhi *ComputeRenderNode::resolveRhi() const {
    if (!m_window)
        return nullptr;
    auto *ri = m_window->rendererInterface();
    if (!ri)
        return nullptr;
    return static_cast<QRhi *>(ri->getResource(m_window, QSGRendererInterface::RhiResource));
}

QRhiCommandBuffer *ComputeRenderNode::resolveCommandBuffer() const {
    if (!m_window)
        return nullptr;
    auto *ri = m_window->rendererInterface();
    if (!ri)
        return nullptr;
    auto *cb = static_cast<QRhiCommandBuffer *>(ri->getResource(m_window, QSGRendererInterface::RhiRedirectCommandBuffer));
    if (!cb) {
        cb = static_cast<QRhiCommandBuffer *>(ri->getResource(m_window, QSGRendererInterface::CommandListResource));
    }
    return cb;
}

bool ComputeRenderNode::ensureBuffers(QRhi *rhi) {
    if (!rhi) {
        return false;
    }
    const bool computeSupported = rhi->isFeatureSupported(QRhi::Compute);
    QRhiTexture *currentInputRhiTexture = m_inputTexture ? m_inputTexture->rhiTexture() : nullptr;
    if (m_inputRhiTexture != currentInputRhiTexture) {
        m_inputRhiTexture = currentInputRhiTexture;
        m_bufferLayoutDirty = true;
        m_renderTargetDirty = true;
    }

    // Track extra texture rhi changes
    QList<QRhiTexture *> currentExtraRhi;
    for (auto *tex : std::as_const(m_extraTextures))
        currentExtraRhi.append(tex ? tex->rhiTexture() : nullptr);
    if (m_extraRhiTextures != currentExtraRhi) {
        m_extraRhiTextures = currentExtraRhi;
        m_passSrbDirty = true;
    }

    if (!m_shaderPath.isEmpty() && m_shaderDirty) {
        QFile f(m_shaderPath);
        if (f.open(QIODevice::ReadOnly)) {
            QShader nextShader = QShader::fromSerialized(f.readAll());
            if (nextShader.isValid()) {
                m_shader = nextShader;
                m_shaderDirty = false;
            } else {
                m_error = QStringLiteral("Compute shader file is invalid: %1").arg(m_shaderPath);
            }
        } else {
            m_error = QStringLiteral("Compute shader file cannot be opened: %1").arg(m_shaderPath);
        }
    }

    bool textureSizeChanged = false;
    if (computeSupported && m_inputTexture) {
        QSize sz = m_inputTexture->textureSize();
        if (!m_outputTexture || m_outputTexture->pixelSize() != sz) {
            qCDebug(lcComputeRenderNode) << "ComputeRenderNode: Resizing output texture to" << sz;
            textureSizeChanged = true;
            m_texturesDirty = true;
        }
    }

    // Full rebuild: shader change, input change, or first init
    if (m_bufferLayoutDirty) {
        destroyResources();
        m_bufferLayoutDirty = false;
        m_texturesDirty = false;
        m_passSrbDirty = false;

        QSize sz(qMax(1, static_cast<int>(m_width)), qMax(1, static_cast<int>(m_height)));
        if (m_inputTexture) {
            QSize ts = m_inputTexture->textureSize();
            if (ts.isValid() && ts.width() > 0)
                sz = ts;
        }

        if (computeSupported)
            m_srb = rhi->newShaderResourceBindings();
        QList<QRhiShaderResourceBinding> bindings;

        if (!m_sampler) {
            m_sampler = rhi->newSampler(QRhiSampler::Linear, QRhiSampler::Linear, QRhiSampler::None, QRhiSampler::ClampToEdge, QRhiSampler::ClampToEdge);
            m_sampler->create();
        }

        if (computeSupported && m_srb && m_inputRhiTexture) {
            bindings.append(QRhiShaderResourceBinding::sampledTexture(kInputBinding, QRhiShaderResourceBinding::ComputeStage, m_inputRhiTexture, m_sampler));
        }

        // Extra textures at binding 3, 4, 5, ...
        for (int i = 0; i < m_extraTextures.size(); ++i) {
            QRhiTexture *extraRhi = m_extraTextures[i] ? m_extraTextures[i]->rhiTexture() : nullptr;
            if (extraRhi) {
                bindings.append(QRhiShaderResourceBinding::sampledTexture(kExtraBindingBase + i, QRhiShaderResourceBinding::ComputeStage, extraRhi, m_sampler));
            }
        }

        if (computeSupported && m_srb) {
            const QRhiTexture::Format outputFormat = m_hdrOutput ? QRhiTexture::RGBA16F : QRhiTexture::RGBA8;
            m_outputTexture = rhi->newTexture(outputFormat, sz, 1, QRhiTexture::UsedWithLoadStore | QRhiTexture::RenderTarget);
            if (!m_outputTexture->create()) {
                delete m_outputTexture;
                m_outputTexture = nullptr;
                delete m_srb;
                m_srb = nullptr;
                m_error = QStringLiteral("Compute output texture creation failed.");
            } else {
                bindings.append(QRhiShaderResourceBinding::imageLoadStore(kOutputBinding, QRhiShaderResourceBinding::ComputeStage, m_outputTexture, 0));

                // Second output texture for ping-pong (multi-pass)
                if (m_dispatchCount > 1) {
                    m_outputTextureB = rhi->newTexture(outputFormat, sz, 1, QRhiTexture::UsedWithLoadStore | QRhiTexture::RenderTarget);
                    if (!m_outputTextureB->create()) {
                        delete m_outputTextureB;
                        m_outputTextureB = nullptr;
                    }
                }
            }
        }

        m_vbuf = rhi->newBuffer(QRhiBuffer::Immutable, QRhiBuffer::VertexBuffer, sizeof(kQuadData));
        if (!m_vbuf->create()) {
            m_error = QStringLiteral("Compute blit vertex buffer creation failed.");
            delete m_vbuf; m_vbuf = nullptr;
            delete m_sampler; m_sampler = nullptr;
            delete m_outputTexture; m_outputTexture = nullptr;
            delete m_srb; m_srb = nullptr;
            return false;
        }

        m_ubuf = rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, 80);
        if (!m_ubuf->create()) {
            m_error = QStringLiteral("Compute blit uniform buffer creation failed.");
            delete m_ubuf; m_ubuf = nullptr;
            delete m_vbuf; m_vbuf = nullptr;
            delete m_sampler; m_sampler = nullptr;
            delete m_outputTexture; m_outputTexture = nullptr;
            delete m_srb; m_srb = nullptr;
            return false;
        }

        if (m_inputTexture && !m_inputRhiTexture) {
            m_bufferLayoutDirty = true;
        }

        if (computeSupported && m_srb) {
            quint32 ubufSize = kParamsBlockSize;
            if (m_shader.isValid()) {
                const QShaderDescription desc = m_shader.description();
                const QList<QShaderDescription::UniformBlock> blocks = desc.uniformBlocks();
                for (const auto &block : blocks) {
                    if (block.binding == kParamsBinding) {
                        ubufSize = block.size;
                        break;
                    }
                }
            }
            ubufSize = qMax<quint32>(32, ubufSize);

            m_paramUbuf = rhi->newBuffer(QRhiBuffer::Dynamic, QRhiBuffer::UniformBuffer, ubufSize);
            if (!m_paramUbuf->create()) {
                delete m_paramUbuf;
                m_paramUbuf = nullptr;
                delete m_srb;
                m_srb = nullptr;
                if (m_error.isEmpty())
                    m_error = QStringLiteral("Compute parameter uniform buffer creation failed.");
            } else {
                bindings.append(QRhiShaderResourceBinding::uniformBuffer(kParamsBinding, QRhiShaderResourceBinding::ComputeStage, m_paramUbuf));
                m_srb->setBindings(bindings.cbegin(), bindings.cend());
                if (!m_srb->create()) {
                    delete m_srb;
                    m_srb = nullptr;
                    if (m_error.isEmpty())
                        m_error = QStringLiteral("Compute shader resource bindings creation failed.");
                    qCWarning(lcComputeRenderNode) << m_error;
                } else {
                    m_paramsDirty = true;
                }
            }
        }

        m_shaderDirty = true;
        m_renderTargetDirty = true;
        return true;
    }

    // Partial rebuild: only output textures changed (HDR format, texture size)
    if (m_texturesDirty && computeSupported && m_srb) {
        m_texturesDirty = false;
        m_passSrbDirty = true;

        delete m_outputTexture;
        m_outputTexture = nullptr;
        delete m_outputTextureB;
        m_outputTextureB = nullptr;

        QSize sz(qMax(1, static_cast<int>(m_width)), qMax(1, static_cast<int>(m_height)));
        if (m_inputTexture) {
            QSize ts = m_inputTexture->textureSize();
            if (ts.isValid() && ts.width() > 0)
                sz = ts;
        }

        const QRhiTexture::Format outputFormat = m_hdrOutput ? QRhiTexture::RGBA16F : QRhiTexture::RGBA8;
        m_outputTexture = rhi->newTexture(outputFormat, sz, 1, QRhiTexture::UsedWithLoadStore | QRhiTexture::RenderTarget);
        if (!m_outputTexture->create()) {
            delete m_outputTexture;
            m_outputTexture = nullptr;
            m_error = QStringLiteral("Compute output texture creation failed.");
            return false;
        }

        if (m_dispatchCount > 1) {
            m_outputTextureB = rhi->newTexture(outputFormat, sz, 1, QRhiTexture::UsedWithLoadStore | QRhiTexture::RenderTarget);
            if (!m_outputTextureB->create()) {
                delete m_outputTextureB;
                m_outputTextureB = nullptr;
            }
        }

        // Rebuild main SRB with new output texture
        delete m_srb;
        m_srb = rhi->newShaderResourceBindings();
        QList<QRhiShaderResourceBinding> bindings;
        if (m_inputRhiTexture)
            bindings.append(QRhiShaderResourceBinding::sampledTexture(kInputBinding, QRhiShaderResourceBinding::ComputeStage, m_inputRhiTexture, m_sampler));
        for (int i = 0; i < m_extraRhiTextures.size(); ++i) {
            if (m_extraRhiTextures[i])
                bindings.append(QRhiShaderResourceBinding::sampledTexture(kExtraBindingBase + i, QRhiShaderResourceBinding::ComputeStage, m_extraRhiTextures[i], m_sampler));
        }
        bindings.append(QRhiShaderResourceBinding::imageLoadStore(kOutputBinding, QRhiShaderResourceBinding::ComputeStage, m_outputTexture, 0));
        if (m_paramUbuf)
            bindings.append(QRhiShaderResourceBinding::uniformBuffer(kParamsBinding, QRhiShaderResourceBinding::ComputeStage, m_paramUbuf));
        m_srb->setBindings(bindings.cbegin(), bindings.cend());
        if (!m_srb->create()) {
            delete m_srb;
            m_srb = nullptr;
            m_error = QStringLiteral("Compute shader resource bindings creation failed.");
            return false;
        }

        m_renderTargetDirty = true;
        m_paramsDirty = true;
    }

    // Rebuild pass SRBs for multi-pass ping-pong
    if (m_passSrbDirty && computeSupported && m_outputTexture && m_outputTextureB && m_sampler && m_paramUbuf && m_dispatchCount > 1) {
        m_passSrbDirty = false;

        delete m_passSrbA;
        m_passSrbA = nullptr;
        delete m_passSrbB;
        m_passSrbB = nullptr;

        QRhiTexture *textures[2] = {m_outputTexture, m_outputTextureB};
        for (int pass = 0; pass < qMin(2, m_dispatchCount); ++pass) {
            QRhiTexture *writeTex = textures[pass % 2];
            QRhiTexture *readTex = (pass == 0) ? m_inputRhiTexture : textures[(pass + 1) % 2];

            QList<QRhiShaderResourceBinding> passBindings;
            passBindings.append(QRhiShaderResourceBinding::sampledTexture(kInputBinding, QRhiShaderResourceBinding::ComputeStage, readTex, m_sampler));
            passBindings.append(QRhiShaderResourceBinding::imageLoadStore(kOutputBinding, QRhiShaderResourceBinding::ComputeStage, writeTex, 0));
            passBindings.append(QRhiShaderResourceBinding::uniformBuffer(kParamsBinding, QRhiShaderResourceBinding::ComputeStage, m_paramUbuf));
            for (int i = 0; i < m_extraRhiTextures.size(); ++i) {
                if (m_extraRhiTextures[i])
                    passBindings.append(QRhiShaderResourceBinding::sampledTexture(kExtraBindingBase + i, QRhiShaderResourceBinding::ComputeStage, m_extraRhiTextures[i], m_sampler));
            }

            auto *&targetSrb = (pass == 0) ? m_passSrbA : m_passSrbB;
            targetSrb = rhi->newShaderResourceBindings();
            targetSrb->setBindings(passBindings.cbegin(), passBindings.cend());
            if (!targetSrb->create()) {
                delete targetSrb;
                targetSrb = nullptr;
                m_error = QStringLiteral("Multi-pass SRB creation failed at pass %1").arg(pass);
                break;
            }
        }
    } else if (m_dispatchCount <= 1) {
        m_passSrbDirty = false;
        delete m_passSrbA;
        m_passSrbA = nullptr;
        delete m_passSrbB;
        m_passSrbB = nullptr;
    }

    return m_vbuf != nullptr && m_ubuf != nullptr && m_sampler != nullptr;
}

bool ComputeRenderNode::ensurePipeline(QRhi *rhi) {
    QRhiTexture *wantedRenderTexture = (m_pipeline && m_inputRhiTexture) ? m_outputTexture : m_inputRhiTexture;
    if (!wantedRenderTexture && m_outputTexture)
        wantedRenderTexture = m_outputTexture;

    const bool renderTargetChanged = (m_renderTexture != wantedRenderTexture);
    if (!m_shaderDirty && !m_renderTargetDirty && !renderTargetChanged && m_pipeline && m_renderPipeline)
        return true;

    if (m_pipeline)
        delete m_pipeline;
    m_pipeline = nullptr;

    if (m_renderPipeline)
        delete m_renderPipeline;
    m_renderPipeline = nullptr;

    if (m_renderSrb) {
        delete m_renderSrb;
        m_renderSrb = nullptr;
    }
    m_renderTexture = wantedRenderTexture;
    m_error.clear();

    bool computeOk = false;
    if (!rhi->isFeatureSupported(QRhi::Compute)) {
        m_error = QStringLiteral("Compute shaders are not supported on this hardware/backend.");
    } else if (!m_inputRhiTexture) {
        m_error = QStringLiteral("Compute input texture is not ready.");
    } else if (m_srb && m_shader.isValid()) {
        m_pipeline = rhi->newComputePipeline();
        m_pipeline->setShaderStage({QRhiShaderStage::Compute, m_shader});
        m_pipeline->setShaderResourceBindings(m_srb);
        computeOk = m_pipeline->create();
        if (!computeOk) {
            delete m_pipeline;
            m_pipeline = nullptr;
            m_error = QStringLiteral("Compute pipeline creation failed: %1").arg(m_shaderPath);
        }
    } else {
        m_error = QStringLiteral("Compute shader path or resource bindings are not ready.");
    }

    wantedRenderTexture = computeOk ? m_outputTexture : m_inputRhiTexture;
    if (!wantedRenderTexture && m_outputTexture)
        wantedRenderTexture = m_outputTexture;
    // For multi-pass ping-pong, the final output depends on dispatch count parity
    if (computeOk && m_dispatchCount > 1 && m_outputTextureB) {
        wantedRenderTexture = (m_dispatchCount % 2 == 1) ? m_outputTexture : m_outputTextureB;
    }
    m_renderTexture = wantedRenderTexture;

    bool graphicsOk = false;
    if (m_renderTexture && m_ubuf && m_sampler) {
        m_renderSrb = rhi->newShaderResourceBindings();
        m_renderSrb->setBindings({
            QRhiShaderResourceBinding::uniformBuffer(0, QRhiShaderResourceBinding::VertexStage, m_ubuf),
            QRhiShaderResourceBinding::sampledTexture(1, QRhiShaderResourceBinding::FragmentStage, m_renderTexture, m_sampler),
        });
        if (!m_renderSrb->create()) {
            if (m_error.isEmpty())
                m_error = QStringLiteral("Compute blit shader resource bindings creation failed.");
            return false;
        }

        m_renderPipeline = rhi->newGraphicsPipeline();
        m_renderPipeline->setTopology(QRhiGraphicsPipeline::TriangleStrip);
        m_renderPipeline->setShaderResourceBindings(m_renderSrb);
        QRhiVertexInputLayout inputLayout;
        inputLayout.setBindings({{4 * sizeof(float)}});
        inputLayout.setAttributes({{0, 0, QRhiVertexInputAttribute::Float2, 0}, {0, 1, QRhiVertexInputAttribute::Float2, 2 * sizeof(float)}});
        m_renderPipeline->setVertexInputLayout(inputLayout);

        if (ensureBlitShaders()) {
            m_renderPipeline->setShaderStages({{QRhiShaderStage::Vertex, s_cachedBlitVert}, {QRhiShaderStage::Fragment, s_cachedBlitFrag}});
            auto *rt = renderTarget() ? renderTarget()->renderPassDescriptor() : nullptr;
            m_renderPipeline->setTargetBlends({{}});
            m_renderPipeline->setRenderPassDescriptor(rt);
            graphicsOk = m_renderPipeline->create();
            if (!graphicsOk && m_error.isEmpty())
                m_error = QStringLiteral("Compute blit graphics pipeline creation failed.");
        } else if (m_error.isEmpty()) {
            m_error = QStringLiteral("Compute blit shader files cannot be opened.");
        }
    }

    m_shaderDirty = false;
    m_renderTargetDirty = false;
    qCDebug(lcComputeRenderNode) << (graphicsOk ? (computeOk ? "Compute/Graphics" : "Graphics Only") : "Compute Only") << "Pipeline build complete:" << m_shaderPath;
    return graphicsOk;
}

void ComputeRenderNode::prepare() {
    m_rhi = resolveRhi();
    if (!m_rhi)
        return;

    if (!ensureBuffers(m_rhi))
        return;
    if (!ensurePipeline(m_rhi))
        return;

    auto *cb = resolveCommandBuffer();
    if (!cb)
        return;

    QRhiResourceUpdateBatch *batch = m_rhi->nextResourceUpdateBatch();

    if (m_vbuf && !m_verticesUploaded) {
        batch->uploadStaticBuffer(m_vbuf, kQuadData);
        m_verticesUploaded = true;
    }

    if (m_shaderDirty) {
        qCDebug(lcComputeRenderNode) << "ComputeRenderNode: Dispatching compute shader" << m_workGroupX << "x" << m_workGroupY;
    }

    if (m_ubuf) {
        QMatrix4x4 mvp;
        if (const QMatrix4x4 *projection = projectionMatrix())
            mvp = *projection;
        if (const QMatrix4x4 *nodeMatrix = matrix())
            mvp *= *nodeMatrix;
        mvp.scale(m_width, m_height, 1.0f);
        batch->updateDynamicBuffer(m_ubuf, 0, 64, mvp.constData());
        const float opacity = static_cast<float>(m_opacity);
        batch->updateDynamicBuffer(m_ubuf, 64, 4, &opacity);
    }

    if (m_paramUbuf && m_shader.isValid() && m_paramsDirty) {
        const QShaderDescription desc = m_shader.description();
        const QList<QShaderDescription::UniformBlock> blocks = desc.uniformBlocks();
        const QShaderDescription::UniformBlock *paramBlock = nullptr;
        for (const auto &block : blocks) {
            if (block.binding == kParamsBinding) {
                paramBlock = &block;
                break;
            }
        }

        if (paramBlock) {
            QByteArray upload(paramBlock->size, 0);
            for (const auto &member : paramBlock->members) {
                const QVariant &val = m_params.value(member.name);
                if (!val.isValid()) {
                    continue;
                }

                int offset = member.offset;
                if (offset + member.size > upload.size())
                    continue;

                switch (member.type) {
                case QShaderDescription::Float: {
                    float f = val.toFloat();
                    std::memcpy(upload.data() + offset, &f, sizeof(float));
                    break;
                }
                case QShaderDescription::Int: {
                    int i = val.toInt();
                    std::memcpy(upload.data() + offset, &i, sizeof(int));
                    break;
                }
                case QShaderDescription::Bool: {
                    int b = val.toBool() ? 1 : 0;
                    std::memcpy(upload.data() + offset, &b, sizeof(int));
                    break;
                }
                case QShaderDescription::Vec2: {
                    float v[2] = {0.0f, 0.0f};
                    if (val.canConvert<QPointF>()) {
                        QPointF p = val.toPointF();
                        v[0] = static_cast<float>(p.x());
                        v[1] = static_cast<float>(p.y());
                    } else if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        if (list.size() >= 2) {
                            v[0] = list[0].toFloat();
                            v[1] = list[1].toFloat();
                        }
                    }
                    std::memcpy(upload.data() + offset, v, 2 * sizeof(float));
                    break;
                }
                case QShaderDescription::Vec3: {
                    float v[3] = {0.0f, 0.0f, 0.0f};
                    if (val.canConvert<QColor>()) {
                        QColor c = val.value<QColor>();
                        v[0] = static_cast<float>(c.redF());
                        v[1] = static_cast<float>(c.greenF());
                        v[2] = static_cast<float>(c.blueF());
                    } else if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        if (list.size() >= 3) {
                            v[0] = list[0].toFloat();
                            v[1] = list[1].toFloat();
                            v[2] = list[2].toFloat();
                        }
                    }
                    std::memcpy(upload.data() + offset, v, 3 * sizeof(float));
                    break;
                }
                case QShaderDescription::Vec4: {
                    float v[4] = {0.0f, 0.0f, 0.0f, 0.0f};
                    if (val.canConvert<QColor>()) {
                        QColor c = val.value<QColor>();
                        v[0] = static_cast<float>(c.redF());
                        v[1] = static_cast<float>(c.greenF());
                        v[2] = static_cast<float>(c.blueF());
                        v[3] = static_cast<float>(c.alphaF());
                    } else if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        if (list.size() >= 4) {
                            v[0] = list[0].toFloat();
                            v[1] = list[1].toFloat();
                            v[2] = list[2].toFloat();
                            v[3] = list[3].toFloat();
                        }
                    }
                    std::memcpy(upload.data() + offset, v, 4 * sizeof(float));
                    break;
                }
                case QShaderDescription::Mat2: {
                    if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        const int stride = member.matrixStride > 0 ? member.matrixStride : static_cast<int>(2 * sizeof(float));
                        const int cols = 2;
                        const int rows = 2;
                        for (int c = 0; c < cols; ++c) {
                            for (int r = 0; r < rows; ++r) {
                                const int srcIdx = member.matrixIsRowMajor ? (r * cols + c) : (c * rows + r);
                                const int dstOff = offset + c * stride + r * static_cast<int>(sizeof(float));
                                if (dstOff + static_cast<int>(sizeof(float)) <= upload.size() && srcIdx < list.size()) {
                                    float f = list[srcIdx].toFloat();
                                    std::memcpy(upload.data() + dstOff, &f, sizeof(float));
                                }
                            }
                        }
                    }
                    break;
                }
                case QShaderDescription::Mat3: {
                    if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        const int stride = member.matrixStride > 0 ? member.matrixStride : static_cast<int>(4 * sizeof(float));
                        const int cols = 3;
                        const int rows = 3;
                        for (int c = 0; c < cols; ++c) {
                            for (int r = 0; r < rows; ++r) {
                                const int srcIdx = member.matrixIsRowMajor ? (r * cols + c) : (c * rows + r);
                                const int dstOff = offset + c * stride + r * static_cast<int>(sizeof(float));
                                if (dstOff + static_cast<int>(sizeof(float)) <= upload.size() && srcIdx < list.size()) {
                                    float f = list[srcIdx].toFloat();
                                    std::memcpy(upload.data() + dstOff, &f, sizeof(float));
                                }
                            }
                        }
                    }
                    break;
                }
                case QShaderDescription::Mat4: {
                    if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        const int stride = member.matrixStride > 0 ? member.matrixStride : static_cast<int>(4 * sizeof(float));
                        const int cols = 4;
                        const int rows = 4;
                        for (int c = 0; c < cols; ++c) {
                            for (int r = 0; r < rows; ++r) {
                                const int srcIdx = member.matrixIsRowMajor ? (r * cols + c) : (c * rows + r);
                                const int dstOff = offset + c * stride + r * static_cast<int>(sizeof(float));
                                if (dstOff + static_cast<int>(sizeof(float)) <= upload.size() && srcIdx < list.size()) {
                                    float f = list[srcIdx].toFloat();
                                    std::memcpy(upload.data() + dstOff, &f, sizeof(float));
                                }
                            }
                        }
                    }
                    break;
                }
                case QShaderDescription::Int2:
                case QShaderDescription::Int3:
                case QShaderDescription::Int4: {
                    const int count = (member.type == QShaderDescription::Int2) ? 2 :
                                      (member.type == QShaderDescription::Int3) ? 3 : 4;
                    int v[4] = {0, 0, 0, 0};
                    if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        for (int i = 0; i < qMin(list.size(), count); ++i)
                            v[i] = list[i].toInt();
                    } else if (count == 1) {
                        v[0] = val.toInt();
                    }
                    std::memcpy(upload.data() + offset, v, count * sizeof(int));
                    break;
                }
                case QShaderDescription::Uint: {
                    int i = val.toInt();
                    std::memcpy(upload.data() + offset, &i, sizeof(int));
                    break;
                }
                case QShaderDescription::Half:
                case QShaderDescription::Half2:
                case QShaderDescription::Half3:
                case QShaderDescription::Half4: {
                    const int count = (member.type == QShaderDescription::Half) ? 1 :
                                      (member.type == QShaderDescription::Half2) ? 2 :
                                      (member.type == QShaderDescription::Half3) ? 3 : 4;
                    float v[4] = {0.0f, 0.0f, 0.0f, 0.0f};
                    if (val.canConvert<QVariantList>()) {
                        QVariantList list = val.toList();
                        for (int i = 0; i < qMin(list.size(), count); ++i)
                            v[i] = list[i].toFloat();
                    } else if (count == 1) {
                        v[0] = val.toFloat();
                    }
                    // half is 2 bytes but std140 may align to 4; use member.size
                    std::memcpy(upload.data() + offset, v, qMin(member.size, static_cast<int>(count * sizeof(float))));
                    break;
                }
                default:
                    break;
                }
            }

            if (upload.size() < m_paramUbuf->size())
                upload.resize(m_paramUbuf->size());
            batch->updateDynamicBuffer(m_paramUbuf, 0, static_cast<quint32>(qMin<qsizetype>(upload.size(), m_paramUbuf->size())), upload.constData());
        }
    }

    if (!m_pipeline || !m_srb || !m_inputRhiTexture) {
        cb->resourceUpdate(batch);
        return;
    }

    cb->beginComputePass(batch);
    cb->setComputePipeline(m_pipeline);

    if (m_dispatchCount <= 1 || !m_outputTextureB) {
        // Single pass: use the pre-built SRB
        cb->setShaderResources(m_srb);
        cb->dispatch(m_workGroupX, m_workGroupY, m_workGroupZ);
    } else {
        // Multi-pass ping-pong: use pre-allocated SRBs
        // Find passIndex member offset for per-pass uniform injection
        int passIndexOffset = -1;
        if (m_shader.isValid()) {
            const QShaderDescription desc = m_shader.description();
            const QList<QShaderDescription::UniformBlock> blocks = desc.uniformBlocks();
            for (const auto &block : blocks) {
                if (block.binding == kParamsBinding) {
                    for (const auto &member : block.members) {
                        if (member.name == "passIndex" && member.type == QShaderDescription::Int) {
                            passIndexOffset = member.offset;
                            break;
                        }
                    }
                    break;
                }
            }
        }

        for (int pass = 0; pass < m_dispatchCount; ++pass) {
            // Inject passIndex into the UBO if the shader declares it
            if (passIndexOffset >= 0 && m_paramUbuf) {
                const int passVal = pass;
                batch->updateDynamicBuffer(m_paramUbuf, static_cast<quint32>(passIndexOffset), sizeof(int), &passVal);
            }

            QRhiShaderResourceBindings *passSrb = (pass % 2 == 0) ? m_passSrbA : m_passSrbB;
            if (!passSrb)
                break;

            cb->setShaderResources(passSrb);
            cb->dispatch(m_workGroupX, m_workGroupY, m_workGroupZ);
        }
    }

    cb->endComputePass();

    m_paramsDirty = false;
}

void ComputeRenderNode::render(const RenderState *state) {
    auto *cb = resolveCommandBuffer();
    if (!cb || !m_renderPipeline || !m_renderSrb || !m_vbuf || !m_ubuf || !m_renderTexture)
        return;

    cb->setGraphicsPipeline(m_renderPipeline);

    const float dpr = m_window->devicePixelRatio();
    cb->setViewport(QRhiViewport(0, 0, static_cast<float>(m_window->width()) * dpr, static_cast<float>(m_window->height()) * dpr));

    if (state && state->scissorEnabled()) {
        const QRect s = state->scissorRect();
        cb->setScissor(QRhiScissor(s.x(), s.y(), s.width(), s.height()));
    }

    cb->setShaderResources(m_renderSrb);

    const QRhiCommandBuffer::VertexInput vbufBinding(m_vbuf, 0);
    cb->setVertexInput(0, 1, &vbufBinding);
    cb->draw(4);
}

void ComputeRenderNode::releaseResources() { destroyResources(); }

void ComputeRenderNode::destroyResources() {
    delete m_pipeline;
    m_pipeline = nullptr;
    delete m_srb;
    m_srb = nullptr;
    delete m_renderPipeline;
    m_renderPipeline = nullptr;
    delete m_renderSrb;
    m_renderSrb = nullptr;
    delete m_outputTexture;
    m_outputTexture = nullptr;
    delete m_outputTextureB;
    m_outputTextureB = nullptr;
    delete m_sampler;
    m_sampler = nullptr;
    delete m_vbuf;
    m_vbuf = nullptr;
    delete m_ubuf;
    m_ubuf = nullptr;
    delete m_paramUbuf;
    m_paramUbuf = nullptr;
    delete m_passSrbA;
    m_passSrbA = nullptr;
    delete m_passSrbB;
    m_passSrbB = nullptr;
    m_renderTexture = nullptr;
    m_inputRhiTexture = nullptr;
    m_extraRhiTextures.clear();
    m_verticesUploaded = false;

    m_bufferLayoutDirty = true;
    m_texturesDirty = false;
    m_passSrbDirty = false;
    m_shaderDirty = true;
    m_renderTargetDirty = true;
}

QSGRenderNode::StateFlags ComputeRenderNode::changedStates() const { return ViewportState | ScissorState | ColorState | BlendState | CullState; }

QSGRenderNode::RenderingFlags ComputeRenderNode::flags() const { return BoundedRectRendering; }

} // namespace AviQtl::UI::Effects
