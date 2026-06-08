#include "keyframe_evaluator.hpp"
#include "core/include/keyframe_utils.hpp"
#include <algorithm>
#include <QVariantMap>

namespace AviQtl::Engine::Timeline {

bool isStructuredTrack(const QVariant &raw) {
    return Core::KeyframeUtils::isStructuredTrack(raw);
}

int inferredDurationForTrack(const QVariant &raw) {
    return Core::KeyframeUtils::inferredDurationForTrack(raw);
}

QVariantList flattenStructuredTrack(const QVariantMap &track) {
    return Core::KeyframeUtils::flattenStructuredTrack(track);
}

const QHash<QString, EasingFunction> &easingFunctions() {
    return Core::KeyframeUtils::easingFunctions();
}

QVariant evaluateTrack(const QVariantList &track, int frame, const QVariant &fallback) {
    return Core::KeyframeUtils::evaluateTrack(track, frame, fallback);
}

QVariantMap normalizeTrackForDuration(const QVariant &rawTrack, const QVariant &fallback, int durationFrames) {
    return Core::KeyframeUtils::normalizeTrackForDuration(rawTrack, fallback, durationFrames);
}

QVariant evaluateParam(const QVariantMap &params, const QVariantMap &keyframeTracks,
                       const QString &paramName, int frame, double fps, int durationFrames) {
    return Core::KeyframeUtils::evaluateParam(params, keyframeTracks, paramName, frame, fps, durationFrames);
}

} // namespace AviQtl::Engine::Timeline
