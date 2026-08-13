#pragma once
#include "rust_keyframe_core.hpp"
#include <QColor>
#include <QHash>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <algorithm>
#include <functional>
#include <vector>

namespace AviQtl::Core::KeyframeUtils {

using EasingFunction = std::function<double(double, const std::vector<double> &, const QVariantMap &)>;

// Pre-extracted track point to avoid per-frame QVariantMap allocations
struct TrackPoint {
    int frame;
    QVariant value;
    QString interp;
    QVariantMap modeParams;
    QVariantList points; // For custom bezier interpolation
};

inline std::vector<TrackPoint> extractTrackPoints(const QVariantList &track) {
    std::vector<TrackPoint> points;
    points.reserve(track.size());
    for (const auto &v : track) {
        const QVariantMap m = v.toMap();
        QVariantList customPoints;
        auto it = m.find(QStringLiteral("points"));
        if (it != m.end()) {
            customPoints = it.value().toList();
        }
        points.push_back({
            .frame = m.value(QStringLiteral("frame")).toInt(),
            .value = m.value(QStringLiteral("value")),
            .interp = m.value(QStringLiteral("interp")).toString(),
            .modeParams = m.value(QStringLiteral("modeParams")).toMap(),
            .points = customPoints
        });
    }
    return points;
}

// Forward declaration - defined later in the file
inline const QHash<QString, EasingFunction> &easingFunctions();

inline QVariant evaluateTrackFast(const std::vector<TrackPoint> &track, int frame, const QVariant &fallback) {
    if (track.empty()) {
        return fallback;
    }

    if (frame <= track.front().frame) {
        return track.front().value;
    }
    if (frame >= track.back().frame) {
        return track.back().value;
    }

    const bool numeric = fallback.canConvert<double>();

    // Binary search for the correct segment
    size_t lo = 0, hi = track.size() - 1;
    while (lo < hi - 1) {
        size_t mid = lo + (hi - lo) / 2;
        if (track[mid].frame <= frame) {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    const auto &p0 = track[lo];
    const auto &p1 = track[lo + 1];
    const int f0 = p0.frame;
    const int f1 = p1.frame;
    const QVariant &v0 = p0.value;
    const QVariant &v1 = p1.value;

    if (f0 == f1) {
        return v0;
    }
    const double tRaw = (frame - f0) / double(f1 - f0);

    if (p0.interp == QStringLiteral("none")) {
        return (frame < f1) ? v0 : v1;
    }

    if (v0.typeId() == QMetaType::QString && v1.typeId() == QMetaType::QString) {
        QColor c0(v0.toString()), c1(v1.typeId() == QMetaType::QString ? v1.toString() : v0.toString());
        if (c0.isValid() && c1.isValid()) {
            std::vector<double> params;
            if (p0.interp == QStringLiteral("custom")) {
                if (!p0.points.isEmpty()) {
                    for (const auto &val : std::as_const(p0.points))
                        params.push_back(val.toDouble());
                } else {
                    params = {p0.modeParams.value(QStringLiteral("bzx1"), 0.33).toDouble(), p0.modeParams.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                              p0.modeParams.value(QStringLiteral("bzx2"), 0.66).toDouble(), p0.modeParams.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
                }
            }
            const auto &funcs = easingFunctions();
            QString type = p0.interp;
            auto efIt = funcs.find(type);
            if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
            const double t = efIt.value()(tRaw, params, p0.modeParams);
            return QColor(static_cast<int>(c0.red() + (c1.red() - c0.red()) * t), static_cast<int>(c0.green() + (c1.green() - c0.green()) * t),
                          static_cast<int>(c0.blue() + (c1.blue() - c0.blue()) * t), static_cast<int>(c0.alpha() + (c1.alpha() - c0.alpha()) * t))
                .name(QColor::HexArgb);
        }
    }

    if (!numeric || !v0.canConvert<double>() || !v1.canConvert<double>())
        return v0;

    const double a = v0.toDouble(), b = v1.toDouble();
    if (p0.interp == QStringLiteral("random")) {
        const int stepFrames = std::max(1, p0.modeParams.value(QStringLiteral("stepFrames"), 1).toInt()), stepIndex = (frame - f0) / stepFrames;
        const quint32 seed = qHash(f0) ^ qHash(f1) ^ qHash(stepIndex) ^ qHash(static_cast<qint64>(a * 1000)) ^ qHash(static_cast<qint64>(b * 1000));
        return std::min(a, b) + (std::max(a, b) - std::min(a, b)) * (double(seed % 1000000u) / 999999.0);
    }
    if (p0.interp == QStringLiteral("alternate")) {
        const int stepFrames = std::max(1, p0.modeParams.value(QStringLiteral("stepFrames"), 1).toInt());
        return ((frame - f0) / stepFrames % 2 == 0) ? a : b;
    }

    std::vector<double> params;
    if (p0.interp == QStringLiteral("custom")) {
        if (!p0.points.isEmpty()) {
            for (const auto &val : std::as_const(p0.points))
                params.push_back(val.toDouble());
        } else {
            params = {p0.modeParams.value(QStringLiteral("bzx1"), 0.33).toDouble(), p0.modeParams.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                      p0.modeParams.value(QStringLiteral("bzx2"), 0.66).toDouble(), p0.modeParams.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
        }
    }
    const auto &funcs = easingFunctions();
    QString type = p0.interp;
    auto efIt = funcs.find(type);
    if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
    return a + (b - a) * efIt.value()(tRaw, params, p0.modeParams);
}

inline bool isStructuredTrack(const QVariant &raw) {
    const QVariantMap m = raw.toMap();
    return m.contains(QStringLiteral("start")) && m.contains(QStringLiteral("points"));
}

inline QVariantList sortPoints(QVariantList points) {
    std::sort(points.begin(), points.end(), [](const QVariant &a, const QVariant &b) {
        return a.toMap().value(QStringLiteral("frame")).toInt() < b.toMap().value(QStringLiteral("frame")).toInt();
    });
    return points;
}

inline int inferredDurationForTrack(const QVariant &raw) {
    if (isStructuredTrack(raw)) {
        const QVariantList points = raw.toMap().value(QStringLiteral("points")).toList();
        int maxFrame = 0;
        for (const auto &v : std::as_const(points))
            maxFrame = std::max(maxFrame, v.toMap().value(QStringLiteral("frame")).toInt());
        return std::max(1, maxFrame + 1);
    }
    const QVariantList list = raw.toList();
    if (list.isEmpty())
        return 1;
    int maxFrame = 0;
    for (const auto &v : std::as_const(list))
        maxFrame = std::max(maxFrame, v.toMap().value(QStringLiteral("frame")).toInt());
    return std::max(1, maxFrame + 1);
}

inline QVariantList flattenStructuredTrack(const QVariantMap &track) {
    QVariantList out;
    out.append(track.value(QStringLiteral("start")));
    QVariantList points = track.value(QStringLiteral("points")).toList();
    points = sortPoints(points);
    for (const auto &v : std::as_const(points))
        out.append(v);
    return out;
}

inline double solveBezierT(double x, double x1, double x2) {
    return RustCore::solveBezierT(x, x1, x2);
}

inline EasingFunction rustEasingFunction(RustCore::EasingKind kind) {
    return [kind](double t, const std::vector<double> &points, const QVariantMap &modeParams) {
        const double amplitude = modeParams.value(QStringLiteral("amplitude"), 1.0).toDouble();
        const double period = modeParams.value(QStringLiteral("period"), 0.3).toDouble();
        return RustCore::evaluateEasing(kind, t, points, amplitude, period);
    };
}

inline const QHash<QString, EasingFunction> &easingFunctions() {
    using enum RustCore::EasingKind;
    static const QHash<QString, EasingFunction> funcs = {
        {QStringLiteral("linear"), rustEasingFunction(Linear)},
        {QStringLiteral("ease_in_sine"), rustEasingFunction(EaseInSine)},
        {QStringLiteral("ease_out_sine"), rustEasingFunction(EaseOutSine)},
        {QStringLiteral("ease_in_out_sine"), rustEasingFunction(EaseInOutSine)},
        {QStringLiteral("ease_out_in_sine"), rustEasingFunction(EaseOutInSine)},
        {QStringLiteral("ease_in_quad"), rustEasingFunction(EaseInQuad)},
        {QStringLiteral("ease_out_quad"), rustEasingFunction(EaseOutQuad)},
        {QStringLiteral("ease_in_out_quad"), rustEasingFunction(EaseInOutQuad)},
        {QStringLiteral("ease_out_in_quad"), rustEasingFunction(EaseOutInQuad)},
        {QStringLiteral("ease_in_cubic"), rustEasingFunction(EaseInCubic)},
        {QStringLiteral("ease_out_cubic"), rustEasingFunction(EaseOutCubic)},
        {QStringLiteral("ease_in_out_cubic"), rustEasingFunction(EaseInOutCubic)},
        {QStringLiteral("ease_out_in_cubic"), rustEasingFunction(EaseOutInCubic)},
        {QStringLiteral("ease_in_quart"), rustEasingFunction(EaseInQuart)},
        {QStringLiteral("ease_out_quart"), rustEasingFunction(EaseOutQuart)},
        {QStringLiteral("ease_in_out_quart"), rustEasingFunction(EaseInOutQuart)},
        {QStringLiteral("ease_out_in_quart"), rustEasingFunction(EaseOutInQuart)},
        {QStringLiteral("ease_in_quint"), rustEasingFunction(EaseInQuint)},
        {QStringLiteral("ease_out_quint"), rustEasingFunction(EaseOutQuint)},
        {QStringLiteral("ease_in_out_quint"), rustEasingFunction(EaseInOutQuint)},
        {QStringLiteral("ease_out_in_quint"), rustEasingFunction(EaseOutInQuint)},
        {QStringLiteral("ease_in_expo"), rustEasingFunction(EaseInExpo)},
        {QStringLiteral("ease_out_expo"), rustEasingFunction(EaseOutExpo)},
        {QStringLiteral("ease_in_out_expo"), rustEasingFunction(EaseInOutExpo)},
        {QStringLiteral("ease_out_in_expo"), rustEasingFunction(EaseOutInExpo)},
        {QStringLiteral("ease_in_circ"), rustEasingFunction(EaseInCirc)},
        {QStringLiteral("ease_out_circ"), rustEasingFunction(EaseOutCirc)},
        {QStringLiteral("ease_in_out_circ"), rustEasingFunction(EaseInOutCirc)},
        {QStringLiteral("ease_out_in_circ"), rustEasingFunction(EaseOutInCirc)},
        {QStringLiteral("ease_in_back"), rustEasingFunction(EaseInBack)},
        {QStringLiteral("ease_out_back"), rustEasingFunction(EaseOutBack)},
        {QStringLiteral("ease_in_out_back"), rustEasingFunction(EaseInOutBack)},
        {QStringLiteral("ease_out_in_back"), rustEasingFunction(EaseOutInBack)},
        {QStringLiteral("ease_in_elastic"), rustEasingFunction(EaseInElastic)},
        {QStringLiteral("ease_out_elastic"), rustEasingFunction(EaseOutElastic)},
        {QStringLiteral("ease_in_out_elastic"), rustEasingFunction(EaseInOutElastic)},
        {QStringLiteral("ease_out_in_elastic"), rustEasingFunction(EaseOutInElastic)},
        {QStringLiteral("ease_out_bounce"), rustEasingFunction(EaseOutBounce)},
        {QStringLiteral("ease_in_bounce"), rustEasingFunction(EaseInBounce)},
        {QStringLiteral("ease_in_out_bounce"), rustEasingFunction(EaseInOutBounce)},
        {QStringLiteral("ease_out_in_bounce"), rustEasingFunction(EaseOutInBounce)},
        {QStringLiteral("custom"), rustEasingFunction(Custom)},
    };
    return funcs;
}

inline QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback) {
    if (track.isEmpty())
        return fallback;
    auto getFrame = [](const QVariant &v) { return v.toMap().value(QStringLiteral("frame")).toInt(); };
    auto getValue = [](const QVariant &v) { return v.toMap().value(QStringLiteral("value")); };

    if (frame <= getFrame(track.front()))
        return getValue(track.front());
    if (frame >= getFrame(track.back()))
        return getValue(track.back());

    const bool numeric = fallback.canConvert<double>();
    for (int i = 0; i < track.size() - 1; ++i) {
        const QVariantMap m_i = track[i].toMap();
        const QVariantMap m_i1 = track[i + 1].toMap();
        const int f0 = m_i.value(QStringLiteral("frame")).toInt(), f1 = m_i1.value(QStringLiteral("frame")).toInt();
        if (frame < f0 || frame > f1)
            continue;
        const QVariant v0 = m_i.value(QStringLiteral("value")), v1 = m_i1.value(QStringLiteral("value"));
        if (f0 == f1)
            return v0;
        const double tRaw = (frame - f0) / double(f1 - f0);
        QString type = m_i.value(QStringLiteral("interp")).toString();
        const QVariantMap modeParams = m_i.value(QStringLiteral("modeParams")).toMap();

        if (type == QStringLiteral("none"))
            return (frame < f1) ? v0 : v1;
        if (v0.typeId() == QMetaType::QString && v1.typeId() == QMetaType::QString) {
            QColor c0(v0.toString()), c1(v1.typeId() == QMetaType::QString ? v1.toString() : v0.toString());
            if (c0.isValid() && c1.isValid()) {
                std::vector<double> params;
                if (type == QStringLiteral("custom")) {
                    auto it = m_i.find(QStringLiteral("points"));
                    if (it != m_i.end()) {
                        QVariantList lst = it.value().toList();
                        for (const auto &val : std::as_const(lst))
                            params.push_back(val.toDouble());
                    } else {
                        params = {m_i.value(QStringLiteral("bzx1"), 0.33).toDouble(), m_i.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                                  m_i.value(QStringLiteral("bzx2"), 0.66).toDouble(), m_i.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
                    }
                }
                const auto &funcs = easingFunctions();
                auto efIt = funcs.find(type);
                if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
                const double t = efIt.value()(tRaw, params, modeParams);
                return QColor(static_cast<int>(c0.red() + (c1.red() - c0.red()) * t), static_cast<int>(c0.green() + (c1.green() - c0.green()) * t),
                              static_cast<int>(c0.blue() + (c1.blue() - c0.blue()) * t), static_cast<int>(c0.alpha() + (c1.alpha() - c0.alpha()) * t))
                    .name(QColor::HexArgb);
            }
        }
        if (!numeric || !v0.canConvert<double>() || !v1.canConvert<double>())
            return v0;
        const double a = v0.toDouble(), b = v1.toDouble();
        if (type == QStringLiteral("random")) {
            const int stepFrames = std::max(1, modeParams.value(QStringLiteral("stepFrames"), 1).toInt()), stepIndex = (frame - f0) / stepFrames;
            const quint32 seed = qHash(f0) ^ qHash(f1) ^ qHash(stepIndex) ^ qHash(static_cast<qint64>(a * 1000)) ^ qHash(static_cast<qint64>(b * 1000));
            return std::min(a, b) + (std::max(a, b) - std::min(a, b)) * (double(seed % 1000000u) / 999999.0);
        }
        if (type == QStringLiteral("alternate")) {
            const int stepFrames = std::max(1, modeParams.value(QStringLiteral("stepFrames"), 1).toInt());
            return ((frame - f0) / stepFrames % 2 == 0) ? a : b;
        }
        std::vector<double> params;
        if (type == QStringLiteral("custom")) {
            auto it = m_i.find(QStringLiteral("points"));
            if (it != m_i.end()) {
                QVariantList lst = it.value().toList();
                for (const auto &val : std::as_const(lst))
                    params.push_back(val.toDouble());
            } else {
                params = {m_i.value(QStringLiteral("bzx1"), 0.33).toDouble(), m_i.value(QStringLiteral("bzy1"), 0.0).toDouble(),
                          m_i.value(QStringLiteral("bzx2"), 0.66).toDouble(), m_i.value(QStringLiteral("bzy2"), 1.0).toDouble(), 1.0, 1.0};
            }
        }
        const auto &funcs = easingFunctions();
        auto efIt = funcs.find(type);
        if (efIt == funcs.end()) { type = QStringLiteral("linear"); efIt = funcs.find(type); }
        return a + (b - a) * efIt.value()(tRaw, params, modeParams);
    }
    return getValue(track.back());
}

inline QVariantMap normalizeTrackForDuration(const QVariant &rawTrack, const QVariant &fallback, int durationFrames) {
    if (isStructuredTrack(rawTrack)) {
        QVariantMap raw = rawTrack.toMap();
        QVariantMap start = raw.value(QStringLiteral("start")).toMap();
        QVariantList points = raw.value(QStringLiteral("points")).toList(), nextPoints;
        start[QStringLiteral("frame")] = 0;
        if (!start.contains(QStringLiteral("value")))
            start[QStringLiteral("value")] = fallback;

        const int ceiling = durationFrames;
        for (const auto &v : std::as_const(points)) {
            const int f = v.toMap().value(QStringLiteral("frame")).toInt();
            if (f > 0 && f <= ceiling)
                nextPoints.append(v);
        }
        QVariantMap out;
        out[QStringLiteral("start")] = start;
        out[QStringLiteral("points")] = sortPoints(nextPoints);
        return out;
    }
    QVariantList legacy = sortPoints(rawTrack.toList()), points;
    QVariantMap start;
    start[QStringLiteral("frame")] = 0;
    start[QStringLiteral("value")] = legacy.isEmpty() ? fallback : evaluateTrack(legacy, 0, fallback);
    // Preserve interp from existing frame-0 key if present, otherwise default to linear
    QString startInterp = QStringLiteral("linear");
    for (const auto &v : std::as_const(legacy)) {
        if (v.toMap().value(QStringLiteral("frame")).toInt() == 0) {
            startInterp = v.toMap().value(QStringLiteral("interp"), QStringLiteral("linear")).toString();
            break;
        }
    }
    start[QStringLiteral("interp")] = startInterp;
    for (const auto &v : std::as_const(legacy)) {
        const int f = v.toMap().value(QStringLiteral("frame")).toInt();
        if (f > 0 && f < durationFrames)
            points.append(v);
    }
    QVariantMap out;
    out[QStringLiteral("start")] = start;
    out[QStringLiteral("points")] = sortPoints(points);
    return out;
}

// Resolve one track to its flattened evaluation-ready form.
// This is the expensive step (normalize + flatten) and should be cached
// when evaluating many frames or many parameters of the same track.
inline QVariantList resolveTrack(const QVariant &raw, const QVariant &fallback, int durationFrames) {
    if (isStructuredTrack(raw)) {
        int d = (durationFrames > 0) ? durationFrames : inferredDurationForTrack(raw);
        QVariantMap normalized = normalizeTrackForDuration(raw, fallback, d);
        return flattenStructuredTrack(normalized);
    }
    return sortPoints(raw.toList());
}

// Resolve every keyframe track in a single pass. The returned hash maps each
// known parameter name (from `params` plus any track key) to its flattened
// evaluation-ready point list, sharing fallback values pulled from `params`.
// Callers can then call evaluateTrack() directly per frame without repeating
// the expensive normalize + flatten step.
inline QHash<QString, QVariantList> resolveAllTracks(const QVariantMap &params,
                                                     const QVariantMap &keyframeTracks,
                                                     int durationFrames) {
    QHash<QString, QVariantList> out;
    out.reserve(params.size() + keyframeTracks.size());
    for (auto it = keyframeTracks.constBegin(); it != keyframeTracks.constEnd(); ++it) {
        const QVariant fallback = params.value(it.key());
        out.insert(it.key(), resolveTrack(it.value(), fallback, durationFrames));
    }
    return out;
}

inline QVariant evaluateResolvedParam(const QVariantMap &params,
                                       const QHash<QString, QVariantList> &resolved,
                                       const QString &paramName, int frame) {
    const QVariant fallback = params.value(paramName);
    auto it = resolved.find(paramName);
    if (it == resolved.end())
        return fallback;
    return evaluateTrack(it.value(), frame, fallback);
}

} // namespace AviQtl::Core::KeyframeUtils
