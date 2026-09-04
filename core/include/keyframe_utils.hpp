#pragma once
#include "rust_keyframe_core.hpp"
#include "rust_keyframe_document.hpp"
#include <QHash>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

namespace AviQtl::Core::KeyframeUtils {

inline QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback) {
    return RustKeyframeDocument::evaluate(track, frame, fallback).value_or(fallback);
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
