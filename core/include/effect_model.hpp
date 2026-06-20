#pragma once
#include "../../scripting/lua_host.hpp"
#include "keyframe_utils.hpp"
#include <QColor>
#include <QHash>
#include <QObject>
#include <QQmlEngine>
#include <QVariant>
#include <QVariantList>
#include <algorithm>
#include <cmath>
#include <functional>

namespace AviQtl::UI {

// イージング関数シグネチャ: double function(t, params)
using EasingFunction = std::function<double(double, const std::vector<double> &, const QVariantMap &)>;

class EffectModel : public QObject {
    Q_OBJECT

  private:
    // Delegate to shared KeyframeUtils
    static bool isStructuredTrack(const QVariant &raw) { return Core::KeyframeUtils::isStructuredTrack(raw); }
    static QVariantList sortPoints(QVariantList points) { return Core::KeyframeUtils::sortPoints(std::move(points)); }
    static int inferredDurationForTrack(const QVariant &raw) { return Core::KeyframeUtils::inferredDurationForTrack(raw); }
    static QVariantList flattenStructuredTrack(const QVariantMap &track) { return Core::KeyframeUtils::flattenStructuredTrack(track); }
    static const QHash<QString, EasingFunction> &easingFunctions() { return Core::KeyframeUtils::easingFunctions(); }
    static QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback) { return Core::KeyframeUtils::evaluateTrack(track, frame, fallback); }
    static QVariantMap normalizeTrackForDuration(const QVariant &rawTrack, const QVariant &fallback, int durationFrames) { return Core::KeyframeUtils::normalizeTrackForDuration(rawTrack, fallback, durationFrames); }

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
            QVariantMap track;
            QVariantMap start;
            start[QStringLiteral("frame")] = 0;
            start[QStringLiteral("value")] = it.value();
            start[QStringLiteral("interp")] = QStringLiteral("none");
            track[QStringLiteral("start")] = start;
            // end は設定しない（任意終了点の哲学）
            track[QStringLiteral("points")] = QVariantList();
            m_keyframeTracks[it.key()] = track;
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
        if (isStructuredTrack(raw))
            return flattenStructuredTrack(raw.toMap());
        QVariantList list = raw.toList();
        std::sort(list.begin(), list.end(), [](const QVariant &a, const QVariant &b) { return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt(); });
        return list;
    }

    Q_INVOKABLE bool isEndpointFrame(const QString &paramName, int frame) const {
        const QVariant raw = m_keyframeTracks.value(paramName);
        const int startFrame = isStructuredTrack(raw) ? raw.toMap().value(QStringLiteral("start")).toMap().value(QStringLiteral("frame")).toInt() : 0;
        return frame == startFrame;
    }

    Q_INVOKABLE void syncTrackEndpoints(int durationFrames) {
        m_resolvedCache.clear();
        const int oldDuration = m_lastDuration;
        m_lastDuration = durationFrames;
        // 未初期化トラックを初期化し、旧終端フレームにある中間点を新終端フレームへ追従させる
        for (auto it = m_params.begin(); it != m_params.end(); ++it) {
            const QString &key = it.key();
            auto ktIt = m_keyframeTracks.find(key);
            if (ktIt == m_keyframeTracks.end() || !isStructuredTrack(ktIt.value())) {
                QVariantMap start;
                start[QStringLiteral("frame")] = 0;
                start[QStringLiteral("value")] = it.value();
                start[QStringLiteral("interp")] = QStringLiteral("none");
                QVariantMap track;
                track[QStringLiteral("start")] = start;
                track[QStringLiteral("points")] = QVariantList();
                m_keyframeTracks[key] = track;
            } else if (oldDuration > 0 && oldDuration != durationFrames) {
                // 旧終端フレームにある中間点を新終端フレームへ追従させる
                QVariantMap track = ktIt.value().toMap();
                QVariantList points = track[QStringLiteral("points")].toList();
                bool changed = false;
                for (int i = 0; i < points.size(); ++i) {
                    QVariantMap kf = points[i].toMap();
                    if (kf[QStringLiteral("frame")].toInt() == oldDuration) {
                        kf[QStringLiteral("frame")] = durationFrames;
                        points[i] = kf;
                        changed = true;
                        break;
                    }
                }
                if (changed) {
                    track[QStringLiteral("points")] = sortPoints(points);
                    m_keyframeTracks[key] = track;
                }
            }
        }
        emit keyframeTracksChanged();
    }

    Q_INVOKABLE QVariantMap splitTracks(int firstHalfDuration, int originalDuration) {
        m_resolvedCache.clear();
        QVariantMap secondHalfTracks;
        if (originalDuration < 1)
            return secondHalfTracks;

        const int firstEndFrame = std::max(0, firstHalfDuration - 1);
        const int secondHalfDur = std::max(1, originalDuration - firstHalfDuration);
        const int secondEndFrame = std::max(0, secondHalfDur - 1);
        QVariantMap currentTracks = m_keyframeTracks;

        for (auto it = m_params.begin(); it != m_params.end(); ++it) {
            const QString key = it.key();
            const QVariant fallback = it.value();
            QVariantMap track = normalizeTrackForDuration(currentTracks.value(key), fallback, originalDuration);
            QVariantList flat = flattenStructuredTrack(track);
            QVariantMap start = track.value(QStringLiteral("start")).toMap();
            QVariantList points = track.value(QStringLiteral("points")).toList();
            // 前半トラック
            QVariantMap firstTrack;
            QVariantList firstPoints;
            for (const auto &v : std::as_const(points)) {
                const int f = v.toMap().value(QStringLiteral("frame")).toInt();
                if (f > 0 && f < firstEndFrame)
                    firstPoints.append(v.toMap());
            }
            firstTrack[QStringLiteral("start")] = start;
            firstTrack[QStringLiteral("points")] = firstPoints;
            currentTracks[key] = firstTrack;

            // 後半トラック
            QVariantMap secondTrack;
            QVariantMap secondStart;
            secondStart[QStringLiteral("frame")] = 0;
            secondStart[QStringLiteral("value")] = evaluateTrack(flat, firstHalfDuration, fallback);
            secondStart[QStringLiteral("interp")] = start.value(QStringLiteral("interp"), QStringLiteral("none"));
            QVariantList secondPoints;
            for (const auto &v : std::as_const(points)) {
                auto m = v.toMap();
                const int f = m.value(QStringLiteral("frame")).toInt();
                if (f > firstHalfDuration && f < std::max(0, originalDuration - 1)) {
                    m[QStringLiteral("frame")] = f - firstHalfDuration;
                    const int nf = m.value(QStringLiteral("frame")).toInt();
                    if (nf > 0 && nf < secondEndFrame)
                        secondPoints.append(m);
                }
            }
            secondTrack[QStringLiteral("start")] = secondStart;
            secondTrack[QStringLiteral("points")] = secondPoints;
            secondHalfTracks[key] = secondTrack;
        }
        m_keyframeTracks = currentTracks;
        emit keyframeTracksChanged();
        return secondHalfTracks;
    }

    // Must be public to be invokable from QML
    Q_INVOKABLE QStringList availableEasings() const {
        QStringList keys;
        keys << QStringLiteral("none");
        const auto &funcs = easingFunctions();
        for (auto it = funcs.begin(); it != funcs.end(); ++it)
            keys << it.key();
        keys << QStringLiteral("random") << QStringLiteral("alternate");
        return keys;
    }

    void setEnabled(bool e) {
        m_resolvedCache.clear();
        m_trackPointCache.clear();
        if (m_enabled != e) {
            m_enabled = e;
            emit enabledChanged();
        }
    }

    Q_INVOKABLE void setParam(const QString &key, const QVariant &val) {
        invalidateCache(key);
        if (m_params[key] != val) {
            m_params[key] = val;
            m_expressionParamsBuilt = false; // Rebuild on next access

            // アニメーショントラックと同期させ、evaluatedParam() 等が常に最新の静値を返すようにする
            auto ktIt = m_keyframeTracks.find(key);
            if (ktIt != m_keyframeTracks.end()) {
                QVariant trackVar = ktIt.value();
                if (isStructuredTrack(trackVar)) {
                    QVariantMap trackMap = trackVar.toMap();
                    QVariantMap startPoint = trackMap.value(QStringLiteral("start")).toMap();
                    // 開始フレーム(0)の値を更新
                    if (startPoint.value(QStringLiteral("frame")).toInt() == 0) {
                        startPoint[QStringLiteral("value")] = val;
                        trackMap[QStringLiteral("start")] = startPoint;
                        m_keyframeTracks[key] = trackMap;
                        emit keyframeTracksChanged();
                    }
                }
            }

            emit paramsChanged();
            emit paramChanged(key, val);
        }
    }

    Q_INVOKABLE void setKeyframe(const QString &paramName, int frame, const QVariant &value, const QVariantMap &options) {
        invalidateCache(paramName);
        const QVariant fallback = m_params.value(paramName);
        QVariantMap track = normalizeTrackForDuration(m_keyframeTracks.value(paramName), fallback, inferredDurationForTrack(m_keyframeTracks.value(paramName)));

        QVariantMap start = track.value(QStringLiteral("start")).toMap();
        QVariantList points = track.value(QStringLiteral("points")).toList();
        const QString interp = options.value(QStringLiteral("interp"), QStringLiteral("none")).toString();

        const int startFrame = start.value(QStringLiteral("frame")).toInt();

        if (frame <= startFrame) {
            start[QStringLiteral("value")] = value;
            start[QStringLiteral("interp")] = options.value(QStringLiteral("interp"), start.value(QStringLiteral("interp"), QStringLiteral("none")));

            m_params[paramName] = value; // ベース値も同期

            track[QStringLiteral("start")] = start;
            m_keyframeTracks[paramName] = track;
            emit keyframeTracksChanged();
            return;
        }

        QVariantMap kf;
        kf[QStringLiteral("frame")] = frame;
        kf[QStringLiteral("value")] = value;
        kf[QStringLiteral("interp")] = interp;
        auto it = options.find(QStringLiteral("points"));
        if (it != options.end())
            kf[QStringLiteral("points")] = it.value();
        it = options.find(QStringLiteral("modeParams"));
        if (it != options.end())
            kf[QStringLiteral("modeParams")] = it.value();

        bool updated = false;
        for (int i = 0; i < points.size(); ++i) {
            if (points[i].toMap().value(QStringLiteral("frame")).toInt() == frame) {
                points[i] = kf;
                updated = true;
                break;
            }
        }
        if (!updated)
            points.append(kf);

        track[QStringLiteral("points")] = sortPoints(points);
        m_keyframeTracks[paramName] = track;
        emit keyframeTracksChanged();
    }

    Q_INVOKABLE void removeKeyframe(const QString &paramName, int frame) {
        invalidateCache(paramName);
        const QVariant fallback = m_params.value(paramName);
        QVariantMap track = normalizeTrackForDuration(m_keyframeTracks.value(paramName), fallback, inferredDurationForTrack(m_keyframeTracks.value(paramName)));

        const int startFrame = track.value(QStringLiteral("start")).toMap().value(QStringLiteral("frame")).toInt();
        if (frame <= startFrame)
            return;
        QVariantList points = track.value(QStringLiteral("points")).toList(), next;
        for (const auto &v : std::as_const(points))
            if (v.toMap().value(QStringLiteral("frame")).toInt() != frame)
                next.append(v);
        track[QStringLiteral("points")] = next;
        m_keyframeTracks[paramName] = track;
        emit keyframeTracksChanged();
    }

    Q_INVOKABLE bool moveKeyframe(const QString &paramName, int oldFrame, int newFrame) {
        if (oldFrame == newFrame)
            return true;

        invalidateCache(paramName);
        const QVariant fallback = m_params.value(paramName);
        QVariantMap track = normalizeTrackForDuration(m_keyframeTracks.value(paramName), fallback, inferredDurationForTrack(m_keyframeTracks.value(paramName)));

        const int startFrame = track.value(QStringLiteral("start")).toMap().value(QStringLiteral("frame")).toInt();
        if (oldFrame <= startFrame || newFrame <= startFrame)
            return false;

        QVariantList points = track.value(QStringLiteral("points")).toList();
        int sourceIndex = -1;
        for (int i = 0; i < points.size(); ++i) {
            const int frame = points[i].toMap().value(QStringLiteral("frame")).toInt();
            if (frame == newFrame)
                return false;
            if (frame == oldFrame)
                sourceIndex = i;
        }

        if (sourceIndex < 0)
            return false;

        QVariantMap moved = points[sourceIndex].toMap();
        moved[QStringLiteral("frame")] = newFrame;
        points[sourceIndex] = moved;
        track[QStringLiteral("points")] = sortPoints(points);
        m_keyframeTracks[paramName] = track;
        emit keyframeTracksChanged();
        return true;
    }

    Q_INVOKABLE QVariantMap evaluatedParams(int frame, double fps = 60.0) const {
        QVariantMap out;
        // 全てのキーを網羅するために m_params から開始 (avoid temporary QList from keys())
        for (auto it = m_params.cbegin(); it != m_params.cend(); ++it) {
            out[it.key()] = evaluatedParam(it.key(), frame, fps);
        }
        return out;
    }

    Q_INVOKABLE QVariant evaluatedParam(const QString &paramName, int frame, double fps = 60.0) const {
        const QVariant fallback = m_params.value(paramName);
        auto ktIt = m_keyframeTracks.find(paramName);
        if (ktIt == m_keyframeTracks.end())
            return fallback;

        auto rcIt = m_resolvedCache.find(paramName);
        if (rcIt == m_resolvedCache.end()) {
            const QVariant raw = ktIt.value();
            if (isStructuredTrack(raw)) {
                int d = (m_lastDuration > 0) ? m_lastDuration : inferredDurationForTrack(raw);
                QVariantMap normalized = normalizeTrackForDuration(raw, fallback, d);
                rcIt = m_resolvedCache.insert(paramName, flattenStructuredTrack(normalized));
            } else {
                rcIt = m_resolvedCache.insert(paramName, sortPoints(raw.toList()));
            }
            // Rebuild track point cache for this parameter
            m_trackPointCache.insert(paramName, Core::KeyframeUtils::extractTrackPoints(rcIt.value()));
        }

        // Use fast path with pre-extracted track points and binary search
        auto tpIt = m_trackPointCache.find(paramName);
        QVariant baseValue;
        if (tpIt != m_trackPointCache.end()) {
            baseValue = Core::KeyframeUtils::evaluateTrackFast(tpIt.value(), frame, fallback);
        } else {
            baseValue = evaluateTrack(rcIt.value(), frame, fallback);
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
        m_resolvedCache.clear();
        m_trackPointCache.clear();
        emit keyframeTracksChanged();
    }

    void invalidateCache(const QString &paramName) const {
        if (!paramName.isEmpty()) {
            m_resolvedCache.remove(paramName);
            m_trackPointCache.remove(paramName);
        }
    }

  signals:
    void enabledChanged();
    void paramsChanged();
    void paramChanged(const QString &key, const QVariant &val);
    void keyframeTracksChanged();

  private:
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
    mutable QHash<QString, std::vector<Core::KeyframeUtils::TrackPoint>> m_trackPointCache;
    mutable QSet<QString> m_expressionParams;
    mutable bool m_expressionParamsBuilt = false;
};
} // namespace AviQtl::UI
