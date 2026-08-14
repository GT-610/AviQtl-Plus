#pragma once
#include "rust_keyframe_adapter.hpp"
#include "rust_keyframe_document.hpp"
#include <QColor>
#include <QHash>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <algorithm>
#include <vector>

namespace AviQtl::Core::KeyframeUtils {

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
    const auto result = RustKeyframeDocument::inspect(raw, QVariant(), 0);
    return result ? result->inferredDuration : 1;
}

inline QVariantList flattenStructuredTrack(const QVariantMap &track) {
    const QVariant fallback = track.value(QStringLiteral("start")).toMap().value(QStringLiteral("value"));
    const auto result = RustKeyframeDocument::inspect(track, fallback, 0);
    return result ? result->flat : QVariantList();
}

inline double solveBezierT(double x, double x1, double x2) {
    return RustCore::solveBezierT(x, x1, x2);
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

    if (const std::optional<double> numeric = RustKeyframes::evaluateNumericTrack(track, frame, fallback))
        return *numeric;

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
                const double t = RustCore::evaluateEasing(
                    type, tRaw, params,
                    modeParams.value(QStringLiteral("amplitude"), 1.0).toDouble(),
                    modeParams.value(QStringLiteral("period"), 0.3).toDouble());
                return QColor(static_cast<int>(c0.red() + (c1.red() - c0.red()) * t), static_cast<int>(c0.green() + (c1.green() - c0.green()) * t),
                              static_cast<int>(c0.blue() + (c1.blue() - c0.blue()) * t), static_cast<int>(c0.alpha() + (c1.alpha() - c0.alpha()) * t))
                    .name(QColor::HexArgb);
            }
        }
        return v0;
    }
    return getValue(track.back());
}

inline QVariant numericResultWithSourceType(const QVariantList &track, int frame, double value) {
    if (track.isEmpty())
        return value;

    auto pointFrame = [](const QVariant &point) {
        return point.toMap().value(QStringLiteral("frame")).toInt();
    };
    auto pointValue = [](const QVariant &point) {
        return point.toMap().value(QStringLiteral("value"));
    };
    if (frame <= pointFrame(track.front()))
        return pointValue(track.front());
    if (frame >= pointFrame(track.back()))
        return pointValue(track.back());

    for (int index = 0; index + 1 < track.size(); ++index) {
        const QVariantMap first = track[index].toMap();
        const QVariantMap second = track[index + 1].toMap();
        const int firstFrame = first.value(QStringLiteral("frame")).toInt();
        const int secondFrame = second.value(QStringLiteral("frame")).toInt();
        if (frame < firstFrame || frame > secondFrame)
            continue;
        if (firstFrame == secondFrame)
            return first.value(QStringLiteral("value"));
        if (first.value(QStringLiteral("interp")).toString() == QStringLiteral("none")) {
            return frame < secondFrame ? first.value(QStringLiteral("value"))
                                       : second.value(QStringLiteral("value"));
        }
        break;
    }
    return value;
}

inline QVariantMap normalizeTrackForDuration(const QVariant &rawTrack, const QVariant &fallback, int durationFrames) {
    const auto result = RustKeyframeDocument::normalize(rawTrack, fallback, durationFrames);
    return result ? result->track : QVariantMap();
}

// Resolve one track to its flattened evaluation-ready form.
// This is the expensive step (normalize + flatten) and should be cached
// when evaluating many frames or many parameters of the same track.
inline QVariantList resolveTrack(const QVariant &raw, const QVariant &fallback, int durationFrames) {
    const auto result = RustKeyframeDocument::inspect(raw, fallback, durationFrames);
    return result ? result->flat : QVariantList();
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
