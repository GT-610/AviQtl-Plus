#pragma once
#include "rust_keyframe_adapter.hpp"
#include "rust_keyframe_core.hpp"
#include "rust_keyframe_document.hpp"
#include <QColor>
#include <QHash>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <algorithm>
#include <optional>

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

inline std::optional<QVariant> evaluateResolvedTrack(const QVariantList &track, int frame,
                                                     const QVariant &fallback) {
    if (track.isEmpty())
        return fallback;

    const auto pointFrame = [](const QVariant &point) {
        return point.toMap().value(QStringLiteral("frame")).toInt();
    };
    const auto pointValue = [&fallback](const QVariant &point) {
        const QVariant value = point.toMap().value(QStringLiteral("value"));
        return value.isValid() ? value : fallback;
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
        const QVariant firstValue = pointValue(first);
        const QVariant secondValue = pointValue(second);
        if (firstFrame == secondFrame)
            return firstValue;
        if (first.value(QStringLiteral("interp")).toString() == QStringLiteral("none"))
            return frame < secondFrame ? firstValue : secondValue;

        const auto numeric = Core::RustKeyframes::evaluateNumericTrack(track, frame, fallback);
        if (numeric.has_value())
            return numericResultWithSourceType(track, frame, *numeric);

        const auto parseColor = [](const QVariant &value) -> std::optional<QRgb> {
            if (value.typeId() != QMetaType::QString)
                return std::nullopt;
            const QString text = value.toString();
            if (!text.startsWith(QLatin1Char('#')) ||
                (text.size() != 7 && text.size() != 9)) {
                return std::nullopt;
            }
            bool ok = false;
            const quint32 encoded = text.sliced(1).toUInt(&ok, 16);
            if (!ok)
                return std::nullopt;
            if (text.size() == 7)
                return qRgba((encoded >> 16) & 0xff, (encoded >> 8) & 0xff,
                             encoded & 0xff, 0xff);
            return qRgba((encoded >> 16) & 0xff, (encoded >> 8) & 0xff,
                         encoded & 0xff, (encoded >> 24) & 0xff);
        };
        const auto firstColor = parseColor(firstValue);
        const auto secondColor = parseColor(secondValue);
        if (!firstColor.has_value() || !secondColor.has_value())
            return firstValue;

        QVariantMap progressStart = first;
        QVariantMap progressEnd = second;
        progressStart.insert(QStringLiteral("value"), 0.0);
        progressEnd.insert(QStringLiteral("value"), 1.0);
        const auto progress = Core::RustKeyframes::evaluateNumericTrack(
            {progressStart, progressEnd}, frame, 0.0);
        if (!progress.has_value())
            return std::nullopt;
        const auto channel = [amount = *progress](int start, int end) {
            return static_cast<int>(std::clamp(
                start + (end - start) * amount, 0.0, 255.0));
        };
        return QStringLiteral("#%1%2%3%4")
            .arg(channel(qAlpha(*firstColor), qAlpha(*secondColor)), 2, 16,
                 QLatin1Char('0'))
            .arg(channel(qRed(*firstColor), qRed(*secondColor)), 2, 16,
                 QLatin1Char('0'))
            .arg(channel(qGreen(*firstColor), qGreen(*secondColor)), 2, 16,
                 QLatin1Char('0'))
            .arg(channel(qBlue(*firstColor), qBlue(*secondColor)), 2, 16,
                 QLatin1Char('0'));
    }
    return pointValue(track.back());
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
    const auto value = evaluateResolvedTrack(it.value(), frame, fallback);
    return value.value_or(evaluateTrack(it.value(), frame, fallback));
}

} // namespace AviQtl::Core::KeyframeUtils
