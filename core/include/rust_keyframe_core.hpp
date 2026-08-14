#pragma once

#include "rust_core_abi.hpp"
#include <cstdint>
#include <span>
#include <vector>

namespace AviQtl::RustCore {

enum class EasingKind : std::uint32_t {
    Linear = 0,
    EaseInSine = 1,
    EaseOutSine = 2,
    EaseInOutSine = 3,
    EaseOutInSine = 4,
    EaseInQuad = 5,
    EaseOutQuad = 6,
    EaseInOutQuad = 7,
    EaseOutInQuad = 8,
    EaseInCubic = 9,
    EaseOutCubic = 10,
    EaseInOutCubic = 11,
    EaseOutInCubic = 12,
    EaseInQuart = 13,
    EaseOutQuart = 14,
    EaseInOutQuart = 15,
    EaseOutInQuart = 16,
    EaseInQuint = 17,
    EaseOutQuint = 18,
    EaseInOutQuint = 19,
    EaseOutInQuint = 20,
    EaseInExpo = 21,
    EaseOutExpo = 22,
    EaseInOutExpo = 23,
    EaseOutInExpo = 24,
    EaseInCirc = 25,
    EaseOutCirc = 26,
    EaseInOutCirc = 27,
    EaseOutInCirc = 28,
    EaseInBack = 29,
    EaseOutBack = 30,
    EaseInOutBack = 31,
    EaseOutInBack = 32,
    EaseInElastic = 33,
    EaseOutElastic = 34,
    EaseInOutElastic = 35,
    EaseOutInElastic = 36,
    EaseOutBounce = 37,
    EaseInBounce = 38,
    EaseInOutBounce = 39,
    EaseOutInBounce = 40,
    Custom = 41,
};

enum class NumericInterpolation : std::uint32_t {
    Linear = 0,
    EaseInSine = 1,
    EaseOutSine = 2,
    EaseInOutSine = 3,
    EaseOutInSine = 4,
    EaseInQuad = 5,
    EaseOutQuad = 6,
    EaseInOutQuad = 7,
    EaseOutInQuad = 8,
    EaseInCubic = 9,
    EaseOutCubic = 10,
    EaseInOutCubic = 11,
    EaseOutInCubic = 12,
    EaseInQuart = 13,
    EaseOutQuart = 14,
    EaseInOutQuart = 15,
    EaseOutInQuart = 16,
    EaseInQuint = 17,
    EaseOutQuint = 18,
    EaseInOutQuint = 19,
    EaseOutInQuint = 20,
    EaseInExpo = 21,
    EaseOutExpo = 22,
    EaseInOutExpo = 23,
    EaseOutInExpo = 24,
    EaseInCirc = 25,
    EaseOutCirc = 26,
    EaseInOutCirc = 27,
    EaseOutInCirc = 28,
    EaseInBack = 29,
    EaseOutBack = 30,
    EaseInOutBack = 31,
    EaseOutInBack = 32,
    EaseInElastic = 33,
    EaseOutElastic = 34,
    EaseInOutElastic = 35,
    EaseOutInElastic = 36,
    EaseOutBounce = 37,
    EaseInBounce = 38,
    EaseInOutBounce = 39,
    EaseOutInBounce = 40,
    Custom = 41,
    None = 42,
    Random = 43,
    Alternate = 44,
};

enum class NumericBatchStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
};

using NumericKeyframe = AviQtlNumericKeyframe;
using NumericTrackView = AviQtlNumericTrackView;

inline double solveBezierT(double x, double x1, double x2) {
    return aviqtl_solve_bezier_t(x, x1, x2);
}

inline double evaluateEasing(EasingKind kind, double t, const std::vector<double> &points,
                             double amplitude, double period) {
    const double *data = points.empty() ? nullptr : points.data();
    return aviqtl_easing_evaluate(static_cast<std::uint32_t>(kind), t, data, points.size(),
                                  {.amplitude = amplitude, .period = period});
}

[[nodiscard]] inline NumericBatchStatus evaluateNumericTracks(
    std::span<const NumericTrackView> tracks, std::int32_t frame, std::span<double> output) {
    return static_cast<NumericBatchStatus>(aviqtl_numeric_keyframe_batch_evaluate(
        tracks.data(), tracks.size(), frame, output.data(), output.size()));
}

} // namespace AviQtl::RustCore
