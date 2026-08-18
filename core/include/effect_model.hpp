#pragma once
#include "../../scripting/lua_host.hpp"
#include "constants.hpp"
#include "keyframe_utils.hpp"
#include "rust_keyframe_adapter.hpp"
#include <QColor>
#include <QHash>
#include <QObject>
#include <QQmlEngine>
#include <QVariant>
#include <QVariantList>
#include <utility>
namespace AviQtl::UI {

class EffectModel : public QObject {
    Q_OBJECT

  private:
    static QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback) { return Core::KeyframeUtils::evaluateTrack(track, frame, fallback); }

  public:
    Q_PROPERTY(QString id READ id CONSTANT)
    Q_PROPERTY(QString name READ name CONSTANT)
    Q_PROPERTY(QString kind READ kind CONSTANT)
    Q_PROPERTY(QStringList categories READ categories CONSTANT)
    Q_PROPERTY(bool enabled READ isEnabled WRITE setEnabled NOTIFY enabledChanged)
    Q_PROPERTY(QVariantMap params READ params NOTIFY paramsChanged)
    Q_PROPERTY(QString qmlSource READ qmlSource CONSTANT)
    Q_PROPERTY(QVariantMap keyframeTracks READ keyframeTracks NOTIFY keyframeTracksChanged)
    Q_PROPERTY(QVariantMap uiDefinition READ uiDefinition CONSTANT)

    explicit EffectModel(const QString &id, const QString &name, const QString &kind, const QStringList &categories, const QVariantMap &params = {}, const QString &qmlSource = "", const QVariantMap &uiDef = {}, QObject *parent = nullptr)
        : QObject(parent), m_id(id), m_name(name), m_kind(kind), m_categories(categories), m_enabled(true), m_params(params), m_qmlSource(qmlSource), m_uiDefinition(uiDef) {
        for (auto it = m_params.begin(); it != m_params.end(); ++it) {
            const auto result = Core::RustKeyframeDocument::normalize(
                QVariant(), it.value(), 1);
            if (result) {
                m_keyframeTracks[it.key()] = result->track;
            }
        }
    }

    QString id() const { return m_id; }
    QString name() const { return m_name; }
    QString kind() const { return m_kind; }
    QStringList categories() const { return m_categories; }
    bool isEnabled() const { return m_enabled; }
    QVariantMap params() const { return m_params; }
    QString qmlSource() const { return m_qmlSource; }
    QVariantMap keyframeTracks() const { return m_keyframeTracks; }
    QVariantMap uiDefinition() const { return m_uiDefinition; }

    EffectModel *clone() const {
        auto *copy = new EffectModel(m_id, m_name, m_kind, m_categories, m_params, m_qmlSource, m_uiDefinition);
        copy->m_enabled = m_enabled;
        copy->m_keyframeTracks = m_keyframeTracks;
        copy->m_lastDuration = m_lastDuration;
        return copy;
    }

    Q_INVOKABLE QVariantList keyframeListForUi(const QString &paramName) const {
        const QVariant raw = m_keyframeTracks.value(paramName);
        const auto result = Core::RustKeyframeDocument::inspect(
            raw, m_params.value(paramName), m_lastDuration);
        return result ? result->flat : QVariantList();
    }

    Q_INVOKABLE bool isEndpointFrame(const QString &paramName, int frame) const {
        Q_UNUSED(paramName)
        return frame == 0;
    }

    Q_INVOKABLE void syncTrackEndpoints(int durationFrames) {
        invalidateCache({});
        const int oldDuration = m_lastDuration;
        m_lastDuration = durationFrames;
        for (auto it = m_params.begin(); it != m_params.end(); ++it) {
            const QString &key = it.key();
            const auto result = Core::RustKeyframeDocument::sync(
                m_keyframeTracks.value(key), it.value(), oldDuration, durationFrames);
            if (result) {
                m_keyframeTracks[key] = result->track;
            }
        }
        emit keyframeTracksChanged();
    }

    Q_INVOKABLE QVariantMap splitTracks(int firstHalfDuration, int originalDuration) {
        invalidateCache({});
        QVariantMap secondHalfTracks;
        if (originalDuration < 1)
            return secondHalfTracks;
        QVariantMap currentTracks = m_keyframeTracks;

        for (auto it = m_params.begin(); it != m_params.end(); ++it) {
            const QString key = it.key();
            const QVariant fallback = it.value();
            const auto result = Core::RustKeyframeDocument::split(
                currentTracks.value(key), fallback, firstHalfDuration, originalDuration);
            if (result) {
                currentTracks[key] = result->track;
                if (result->secondaryTrack) {
                    secondHalfTracks[key] = *result->secondaryTrack;
                }
            }
        }
        m_keyframeTracks = currentTracks;
        emit keyframeTracksChanged();
        return secondHalfTracks;
    }

    // Must be public to be invokable from QML
    Q_INVOKABLE QStringList availableEasings() const {
        QStringList keys;
        keys << QStringLiteral("none");
        keys.append(AviQtl::RustCore::easingNames());
        keys << QStringLiteral("random") << QStringLiteral("alternate");
        return keys;
    }

    void setEnabled(bool e) {
        if (m_enabled != e) {
            m_enabled = e;
            emit enabledChanged();
        }
    }

    Q_INVOKABLE void setParam(const QString &key, const QVariant &val) {
        invalidateCache(key);
        if (m_params[key] != val) {
            const QVariant previous = m_params.value(key);
            m_params[key] = val;
            m_expressionParamsBuilt = false; // Rebuild on next access

            auto ktIt = m_keyframeTracks.find(key);
            if (ktIt != m_keyframeTracks.end()) {
                const auto result = Core::RustKeyframeDocument::set(
                    ktIt.value(), previous, m_lastDuration, 0, val, QVariantMap());
                if (result && result->accepted) {
                    ktIt.value() = result->track;
                    emit keyframeTracksChanged();
                }
            }

            emit paramsChanged();
            emit paramChanged(key, val);
        }
    }

    void setParams(const QVariantMap &params) {
        if (m_params == params) {
            return;
        }
        QVariantMap updatedTracks = m_keyframeTracks;
        for (auto it = updatedTracks.begin(); it != updatedTracks.end();) {
            const auto paramIt = params.constFind(it.key());
            if (paramIt == params.cend()) {
                it = updatedTracks.erase(it);
                continue;
            }
            const auto result = Core::RustKeyframeDocument::set(
                it.value(), m_params.value(it.key()), m_lastDuration, 0, paramIt.value(),
                QVariantMap());
            if (result && result->accepted) {
                it.value() = result->track;
            }
            ++it;
        }
        const bool tracksChanged = updatedTracks != m_keyframeTracks;
        m_params = params;
        m_keyframeTracks = std::move(updatedTracks);
        m_expressionParamsBuilt = false;
        invalidateCache({});
        if (tracksChanged) {
            emit keyframeTracksChanged();
        }
        emit paramsChanged();
    }

    Q_INVOKABLE void setKeyframe(const QString &paramName, int frame, const QVariant &value, const QVariantMap &options) {
        invalidateCache(paramName);
        const QVariant fallback = m_params.value(paramName);
        const auto result = Core::RustKeyframeDocument::set(
            m_keyframeTracks.value(paramName), fallback, 0, frame, value, options);
        if (!result || !result->accepted) {
            return;
        }
        if (result->baseValue) {
            m_params[paramName] = *result->baseValue;
        }
        m_keyframeTracks[paramName] = result->track;
        emit keyframeTracksChanged();
    }

    Q_INVOKABLE void removeKeyframe(const QString &paramName, int frame) {
        invalidateCache(paramName);
        const QVariant fallback = m_params.value(paramName);
        const auto result = Core::RustKeyframeDocument::remove(
            m_keyframeTracks.value(paramName), fallback, 0, frame);
        if (!result || !result->accepted) {
            return;
        }
        m_keyframeTracks[paramName] = result->track;
        emit keyframeTracksChanged();
    }

    Q_INVOKABLE bool moveKeyframe(const QString &paramName, int oldFrame, int newFrame) {
        if (oldFrame == newFrame)
            return true;

        invalidateCache(paramName);
        const QVariant fallback = m_params.value(paramName);
        const auto result = Core::RustKeyframeDocument::move(
            m_keyframeTracks.value(paramName), fallback, 0, oldFrame, newFrame);
        if (!result || !result->accepted) {
            return false;
        }
        if (result->changed) {
            m_keyframeTracks[paramName] = result->track;
            emit keyframeTracksChanged();
        }
        return true;
    }

    Q_INVOKABLE QVariantMap evaluatedParams(int frame, double fps = AviQtl::kDefaultFps) const {
        ensureEvaluationCache();
        static_cast<void>(m_numericTrackBatch.evaluate(frame));
        QVariantMap out;
        // 全てのキーを網羅するために m_params から開始 (avoid temporary QList from keys())
        for (auto it = m_params.cbegin(); it != m_params.cend(); ++it) {
            out[it.key()] = evaluatedParam(it.key(), frame, fps);
        }
        return out;
    }

    Q_INVOKABLE QVariant evaluatedParam(const QString &paramName, int frame, double fps = AviQtl::kDefaultFps) const {
        const QVariant fallback = m_params.value(paramName);
        auto ktIt = m_keyframeTracks.find(paramName);
        if (ktIt == m_keyframeTracks.end())
            return fallback;

        ensureEvaluationCache();
        static_cast<void>(m_numericTrackBatch.evaluate(frame));
        QVariant baseValue = fallback;
        const auto resolved = m_resolvedCache.constFind(paramName);
        if (const std::optional<double> numeric = m_numericTrackBatch.value(paramName)) {
            baseValue = resolved == m_resolvedCache.constEnd()
                            ? QVariant(*numeric)
                            : Core::KeyframeUtils::numericResultWithSourceType(*resolved, frame, *numeric);
        } else {
            if (resolved != m_resolvedCache.constEnd())
                baseValue = evaluateTrack(*resolved, frame, fallback);
        }

        // Check expression only if param is known to be an expression
        if (!m_expressionParamsBuilt) {
            rebuildExpressionSet();
            m_expressionParamsBuilt = true;
        }
        if (m_expressionParams.contains(paramName)) {
            std::string expr = m_params.value(paramName).toString().mid(1).toStdString();
            double time = (fps > 0.0) ? frame / fps : 0.0;
            return AviQtl::Scripting::LuaHost::instance().evaluate(expr, time, 0, baseValue.toDouble());
        }

        return baseValue;
    }

    void setKeyframeTracks(const QVariantMap &tracks) {
        m_keyframeTracks = tracks;
        invalidateCache({});
        emit keyframeTracksChanged();
    }

    void setKeyframeTracks(const QVariantMap &tracks, int durationFrames) {
        m_lastDuration = durationFrames;
        setKeyframeTracks(tracks);
    }

    void invalidateCache(const QString &) const {
        m_evaluationCacheDirty = true;
    }

  signals:
    void enabledChanged();
    void paramsChanged();
    void paramChanged(const QString &key, const QVariant &val);
    void keyframeTracksChanged();

  private:
    void ensureEvaluationCache() const {
        if (!m_evaluationCacheDirty)
            return;
        m_resolvedCache = Core::KeyframeUtils::resolveAllTracks(
            m_params, m_keyframeTracks, m_lastDuration);
        m_numericTrackBatch.rebuild(m_params, m_resolvedCache);
        m_evaluationCacheDirty = false;
    }

    void rebuildExpressionSet() const {
        m_expressionParams.clear();
        for (auto it = m_params.constBegin(); it != m_params.constEnd(); ++it) {
            if (it.value().typeId() == QMetaType::QString && it.value().toString().startsWith(QStringLiteral("="))) {
                m_expressionParams.insert(it.key());
            }
        }
    }

    QString m_id;
    QString m_name;
    QString m_kind;
    QStringList m_categories;
    bool m_enabled;
    QVariantMap m_params;
    QString m_qmlSource;
    QVariantMap m_uiDefinition;
    QVariantMap m_keyframeTracks; // パラメータ名 -> QVariantList[{frame,value,interp}]

    mutable int m_lastDuration = -1;
    mutable QHash<QString, QVariantList> m_resolvedCache;
    mutable Core::RustKeyframes::NumericTrackBatch m_numericTrackBatch;
    mutable bool m_evaluationCacheDirty = true;
    mutable QSet<QString> m_expressionParams;
    mutable bool m_expressionParamsBuilt = false;
};
} // namespace AviQtl::UI
