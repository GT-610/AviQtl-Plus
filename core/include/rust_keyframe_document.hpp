#pragma once

#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <optional>

namespace AviQtl::Core::RustKeyframeDocument {

struct TrackResult {
    QVariantMap track;
    QVariantList flat;
    int inferredDuration = 1;
    bool accepted = false;
    bool changed = false;
    std::optional<QVariant> baseValue;
    std::optional<QVariantMap> secondaryTrack;
};

std::optional<TrackResult> inspect(const QVariant &track, const QVariant &fallback,
                                   int duration = 0);
std::optional<TrackResult> normalize(const QVariant &track, const QVariant &fallback,
                                     int duration);
std::optional<TrackResult> set(const QVariant &track, const QVariant &fallback, int duration,
                               int frame, const QVariant &value, const QVariantMap &options);
std::optional<TrackResult> remove(const QVariant &track, const QVariant &fallback, int duration,
                                  int frame);
std::optional<TrackResult> move(const QVariant &track, const QVariant &fallback, int duration,
                                int oldFrame, int newFrame);
std::optional<TrackResult> sync(const QVariant &track, const QVariant &fallback, int oldDuration,
                                int newDuration);
std::optional<TrackResult> split(const QVariant &track, const QVariant &fallback,
                                 int firstHalfDuration, int originalDuration);
std::optional<QVariant> evaluate(const QVariantList &track, int frame, const QVariant &fallback);

} // namespace AviQtl::Core::RustKeyframeDocument
