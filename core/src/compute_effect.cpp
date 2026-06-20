#include "compute_effect.hpp"
#include "compute_render_node.hpp"
#include <QDebug>
#include <QLoggingCategory>
#include <QMetaObject>
#include <QPointer>
#include <QSGNode>
#include <QSGTexture>
#include <QSGTextureProvider>

namespace AviQtl::UI::Effects {

ComputeEffect::ComputeEffect(QQuickItem *parent) : QQuickItem(parent) { setFlag(ItemHasContents, true); }

ComputeEffect::~ComputeEffect() = default;

void ComputeEffect::setSource(QQuickItem *s) {
    if (m_source == s)
        return;
    qCDebug(lcComputeRenderNode) << "ComputeEffect: setSource called with" << s << (s ? s->objectName() : "null");
    m_source = s;
    m_dirty = true;
    emit sourceChanged();
    update();
}

void ComputeEffect::setParams(const QVariantMap &params) {
    if (m_params == params)
        return;
    m_params = params;
    m_dirty = true;
    emit paramsChanged();
    update();
}

void ComputeEffect::setShaderEnabled(bool enabled) {
    if (m_enabled == enabled)
        return;
    m_enabled = enabled;
    m_dirty = true;
    emit shaderEnabledChanged();
    update();
}

void ComputeEffect::setShaderPath(const QUrl &path) {
    if (m_shaderPath == path)
        return;
    m_shaderPath = path;
    m_dirty = true;
    emit shaderPathChanged();
    update();
}

void ComputeEffect::setWorkGroupSizeX(int x) {
    const int clamped = qMax(1, x);
    if (m_workGroupX == clamped)
        return;
    m_workGroupX = clamped;
    m_dirty = true;
    emit workGroupSizeXChanged();
    update();
}

void ComputeEffect::setWorkGroupSizeY(int y) {
    const int clamped = qMax(1, y);
    if (m_workGroupY == clamped)
        return;
    m_workGroupY = clamped;
    m_dirty = true;
    emit workGroupSizeYChanged();
    update();
}

void ComputeEffect::setAutoWorkGroup(bool autoWG) {
    if (m_autoWorkGroup == autoWG)
        return;
    m_autoWorkGroup = autoWG;
    if (m_autoWorkGroup)
        recalcAutoWorkGroup();
    m_dirty = true;
    emit autoWorkGroupChanged();
    update();
}

void ComputeEffect::setHdrOutput(bool hdr) {
    if (m_hdrOutput == hdr)
        return;
    m_hdrOutput = hdr;
    m_dirty = true;
    emit hdrOutputChanged();
    update();
}

void ComputeEffect::setOpacity(qreal o) {
    const qreal clamped = qBound(0.0, o, 1.0);
    if (qFuzzyCompare(m_opacity, clamped))
        return;
    m_opacity = clamped;
    m_dirty = true;
    emit opacityChanged();
    update();
}

void ComputeEffect::setExtraTextures(const QVariantList &textures) {
    if (m_extraTextures == textures)
        return;
    m_extraTextures = textures;
    m_dirty = true;
    emit extraTexturesChanged();
    update();
}

void ComputeEffect::setErrorFromRenderThread(const QString &error) {
    if (m_error == error)
        return;
    m_error = error;
    emit errorChanged();
}

void ComputeEffect::geometryChange(const QRectF &newGeometry, const QRectF &oldGeometry) {
    QQuickItem::geometryChange(newGeometry, oldGeometry);
    if (m_autoWorkGroup)
        recalcAutoWorkGroup();
}

void ComputeEffect::recalcAutoWorkGroup() {
    if (width() > 0.0) {
        const int newX = qMax(1, static_cast<int>(std::ceil(width() / 16.0)));
        if (m_workGroupX != newX) {
            m_workGroupX = newX;
            emit workGroupSizeXChanged();
        }
    }
    if (height() > 0.0) {
        const int newY = qMax(1, static_cast<int>(std::ceil(height() / 16.0)));
        if (m_workGroupY != newY) {
            m_workGroupY = newY;
            emit workGroupSizeYChanged();
        }
    }
}

auto ComputeEffect::updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *) -> QSGNode * {
    if (!m_enabled) {
        delete oldNode;
        return nullptr;
    }

    auto *node = static_cast<ComputeRenderNode *>(oldNode);
    if (!node) {
        node = new ComputeRenderNode(window());
        m_dirty = true;
    }

    if (m_dirty || m_source) {
        node->syncParams(m_params);

        // ソーステクスチャの同期
        if (m_source) {
            QSGTextureProvider *provider = m_source->textureProvider();
            if (!provider) {
                static int pWarn = 0;
                if (pWarn++ % 60 == 0)
                    qCWarning(lcComputeRenderNode) << "ComputeEffect: Source item" << m_source << "has NO texture provider!";
            }
            QSGTexture *tex = provider ? provider->texture() : nullptr;

            qCDebug(lcComputeRenderNode) << "ComputeEffect: Syncing texture" << tex << "from source" << (m_source ? m_source->objectName() : "deleted") << "to node";
            node->syncInputTexture(tex);

            if (!tex) {
                static int warnCount = 0;
                if (warnCount++ % 60 == 0)
                    qCWarning(lcComputeRenderNode) << "ComputeEffect: tex is NULL. Item exists but has no GPU texture. Is layer.enabled: true?";
            }
        } else {
            node->syncInputTexture(nullptr);
        }

        QString pathStr = m_shaderPath.isLocalFile() ? m_shaderPath.toLocalFile() : m_shaderPath.toString();
        node->syncShaderPath(pathStr);
        node->syncSize(width(), height());
        node->syncWorkGroupSize(m_workGroupX, m_workGroupY);
        node->syncHdrOutput(m_hdrOutput);
        node->syncOpacity(m_opacity);

        // Collect extra textures from QQuickItem sources
        QList<QSGTexture *> extraTexList;
        for (const QVariant &v : std::as_const(m_extraTextures)) {
            auto *item = v.value<QQuickItem *>();
            if (!item)
                continue;
            QSGTextureProvider *tp = item->textureProvider();
            QSGTexture *tex = tp ? tp->texture() : nullptr;
            if (tex)
                extraTexList.append(tex);
        }
        node->syncExtraTextures(extraTexList);

        m_dirty = false;
    }

    const QString nodeErr = node->errorMessage();
    if (m_error != nodeErr) {
        QPointer<ComputeEffect> self(this);
        QMetaObject::invokeMethod(
            this,
            [self, nodeErr]() {
                if (self)
                    self->setErrorFromRenderThread(nodeErr);
            },
            Qt::QueuedConnection);
    }

    node->markDirty(QSGNode::DirtyMaterial);
    return node;
}

} // namespace AviQtl::UI::Effects
