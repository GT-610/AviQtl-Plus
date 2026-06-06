#pragma once
#include <QColor>
#include <QHash>
#include <QVariant>
#include <cmath>
#include <functional>
#include <vector>

namespace AviQtl::Engine::Timeline {

using EasingFunction = std::function<double(double, const std::vector<double> &, const QVariantMap &)>;

const QHash<QString, EasingFunction> &easingFunctions();

QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback);

bool isStructuredTrack(const QVariant &raw);

QVariantList flattenStructuredTrack(const QVariantMap &track);

QVariantMap normalizeTrackForDuration(const QVariant &rawTrack, const QVariant &fallback, int durationFrames);

int inferredDurationForTrack(const QVariant &raw);

QVariant evaluateParam(const QVariantMap &params, const QVariantMap &keyframeTracks,
                       const QString &paramName, int frame, double fps, int durationFrames);

} // namespace AviQtl::Engine::Timeline
