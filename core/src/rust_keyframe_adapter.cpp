#include "rust_keyframe_adapter.hpp"
#include <algorithm>
#include <limits>
#include <utility>

namespace AviQtl::Core::RustKeyframes {

bool isNumericValue(const QVariant &value) {
    return value.isValid() && value.typeId() != QMetaType::QString && value.canConvert<double>();
}

RustCore::NumericInterpolation interpolationForName(QStringView name) {
    using enum RustCore::NumericInterpolation;
    static const QHash<QString, RustCore::NumericInterpolation> kinds = {
        {QStringLiteral("linear"), Linear},
        {QStringLiteral("ease_in_sine"), EaseInSine},
        {QStringLiteral("ease_out_sine"), EaseOutSine},
        {QStringLiteral("ease_in_out_sine"), EaseInOutSine},
        {QStringLiteral("ease_out_in_sine"), EaseOutInSine},
        {QStringLiteral("ease_in_quad"), EaseInQuad},
        {QStringLiteral("ease_out_quad"), EaseOutQuad},
        {QStringLiteral("ease_in_out_quad"), EaseInOutQuad},
        {QStringLiteral("ease_out_in_quad"), EaseOutInQuad},
        {QStringLiteral("ease_in_cubic"), EaseInCubic},
        {QStringLiteral("ease_out_cubic"), EaseOutCubic},
        {QStringLiteral("ease_in_out_cubic"), EaseInOutCubic},
        {QStringLiteral("ease_out_in_cubic"), EaseOutInCubic},
        {QStringLiteral("ease_in_quart"), EaseInQuart},
        {QStringLiteral("ease_out_quart"), EaseOutQuart},
        {QStringLiteral("ease_in_out_quart"), EaseInOutQuart},
        {QStringLiteral("ease_out_in_quart"), EaseOutInQuart},
        {QStringLiteral("ease_in_quint"), EaseInQuint},
        {QStringLiteral("ease_out_quint"), EaseOutQuint},
        {QStringLiteral("ease_in_out_quint"), EaseInOutQuint},
        {QStringLiteral("ease_out_in_quint"), EaseOutInQuint},
        {QStringLiteral("ease_in_expo"), EaseInExpo},
        {QStringLiteral("ease_out_expo"), EaseOutExpo},
        {QStringLiteral("ease_in_out_expo"), EaseInOutExpo},
        {QStringLiteral("ease_out_in_expo"), EaseOutInExpo},
        {QStringLiteral("ease_in_circ"), EaseInCirc},
        {QStringLiteral("ease_out_circ"), EaseOutCirc},
        {QStringLiteral("ease_in_out_circ"), EaseInOutCirc},
        {QStringLiteral("ease_out_in_circ"), EaseOutInCirc},
        {QStringLiteral("ease_in_back"), EaseInBack},
        {QStringLiteral("ease_out_back"), EaseOutBack},
        {QStringLiteral("ease_in_out_back"), EaseInOutBack},
        {QStringLiteral("ease_out_in_back"), EaseOutInBack},
        {QStringLiteral("ease_in_elastic"), EaseInElastic},
        {QStringLiteral("ease_out_elastic"), EaseOutElastic},
        {QStringLiteral("ease_in_out_elastic"), EaseInOutElastic},
        {QStringLiteral("ease_out_in_elastic"), EaseOutInElastic},
        {QStringLiteral("ease_out_bounce"), EaseOutBounce},
        {QStringLiteral("ease_in_bounce"), EaseInBounce},
        {QStringLiteral("ease_in_out_bounce"), EaseInOutBounce},
        {QStringLiteral("ease_out_in_bounce"), EaseOutInBounce},
        {QStringLiteral("custom"), Custom},
        {QStringLiteral("none"), None},
        {QStringLiteral("random"), Random},
        {QStringLiteral("alternate"), Alternate},
    };
    return kinds.value(name.toString(), Linear);
}

RustCore::NumericTrackView NumericTrackStorage::view(double fallback) const {
    return {
        .keyframes = keyframes.data(),
        .keyframes_length = keyframes.size(),
        .custom_points = customPoints.data(),
        .custom_points_length = customPoints.size(),
        .fallback_value = fallback,
    };
}

std::optional<NumericTrackStorage> buildNumericTrack(const QVariantList &track) {
    NumericTrackStorage storage;
    storage.keyframes.reserve(static_cast<std::size_t>(track.size()));
    for (const QVariant &pointValue : track) {
        const QVariantMap point = pointValue.toMap();
        const QVariant value = point.value(QStringLiteral("value"));
        if (!isNumericValue(value))
            return std::nullopt;

        const auto interpolation = interpolationForName(point.value(QStringLiteral("interp")).toString());
        const QVariantMap modeParams = point.value(QStringLiteral("modeParams")).toMap();
        const std::size_t customOffset = storage.customPoints.size();
        if (interpolation == RustCore::NumericInterpolation::Custom) {
            QVariantList customValues = point.value(QStringLiteral("points")).toList();
            if (customValues.isEmpty()) {
                customValues = {
                    modeParams.value(QStringLiteral("bzx1"), point.value(QStringLiteral("bzx1"), 0.33)),
                    modeParams.value(QStringLiteral("bzy1"), point.value(QStringLiteral("bzy1"), 0.0)),
                    modeParams.value(QStringLiteral("bzx2"), point.value(QStringLiteral("bzx2"), 0.66)),
                    modeParams.value(QStringLiteral("bzy2"), point.value(QStringLiteral("bzy2"), 1.0)),
                    1.0,
                    1.0,
                };
            }
            if (customValues.size() < 6 || customValues.size() % 6 != 0)
                return std::nullopt;
            if (customOffset > std::numeric_limits<std::uint32_t>::max() ||
                static_cast<std::size_t>(customValues.size()) >
                    std::numeric_limits<std::uint32_t>::max() - customOffset) {
                return std::nullopt;
            }
            storage.customPoints.reserve(customOffset + static_cast<std::size_t>(customValues.size()));
            for (const QVariant &customValue : std::as_const(customValues))
                storage.customPoints.push_back(customValue.toDouble());
        }

        const std::size_t customLength = storage.customPoints.size() - customOffset;
        storage.keyframes.push_back({
            .frame = point.value(QStringLiteral("frame")).toInt(),
            .interpolation = static_cast<std::uint32_t>(interpolation),
            .step_frames = static_cast<std::uint32_t>(
                std::max(1, modeParams.value(QStringLiteral("stepFrames"), 1).toInt())),
            .custom_points_offset = static_cast<std::uint32_t>(customOffset),
            .custom_points_length = static_cast<std::uint32_t>(customLength),
            .reserved = 0,
            .value = value.toDouble(),
            .amplitude = modeParams.value(QStringLiteral("amplitude"), 1.0).toDouble(),
            .period = modeParams.value(QStringLiteral("period"), 0.3).toDouble(),
        });
    }
    return storage;
}

std::optional<double> evaluateNumericTrack(const QVariantList &track, int frame,
                                           const QVariant &fallback) {
    std::optional<NumericTrackStorage> storage = buildNumericTrack(track);
    if (!storage)
        return std::nullopt;
    const double fallbackValue = isNumericValue(fallback) ? fallback.toDouble() : 0.0;
    const RustCore::NumericTrackView view = storage->view(fallbackValue);
    double output = 0.0;
    const auto status = RustCore::evaluateNumericTracks(
        std::span<const RustCore::NumericTrackView>(&view, 1), frame,
        std::span<double>(&output, 1));
    if (status != RustCore::NumericBatchStatus::Ok)
        return std::nullopt;
    return output;
}

NumericTrackBatch::NumericTrackBatch(NumericTrackBatch &&other) noexcept
    : m_tracks(std::move(other.m_tracks)), m_fallbacks(std::move(other.m_fallbacks)),
      m_indices(std::move(other.m_indices)), m_output(std::move(other.m_output)),
      m_lastFrame(other.m_lastFrame), m_hasLastFrame(other.m_hasLastFrame),
      m_lastEvaluationValid(other.m_lastEvaluationValid) {
    rebuildViews();
    other.clear();
}

NumericTrackBatch &NumericTrackBatch::operator=(NumericTrackBatch &&other) noexcept {
    if (this == &other)
        return *this;
    m_tracks = std::move(other.m_tracks);
    m_fallbacks = std::move(other.m_fallbacks);
    m_indices = std::move(other.m_indices);
    m_output = std::move(other.m_output);
    m_lastFrame = other.m_lastFrame;
    m_hasLastFrame = other.m_hasLastFrame;
    m_lastEvaluationValid = other.m_lastEvaluationValid;
    rebuildViews();
    other.clear();
    return *this;
}

void NumericTrackBatch::clear() {
    m_tracks.clear();
    m_fallbacks.clear();
    m_views.clear();
    m_indices.clear();
    m_output.clear();
    m_hasLastFrame = false;
    m_lastEvaluationValid = false;
}

void NumericTrackBatch::rebuild(const QVariantMap &params,
                                const QHash<QString, QVariantList> &resolvedTracks) {
    clear();
    m_tracks.reserve(static_cast<std::size_t>(resolvedTracks.size()));
    m_fallbacks.reserve(static_cast<std::size_t>(resolvedTracks.size()));
    for (auto it = resolvedTracks.constBegin(); it != resolvedTracks.constEnd(); ++it) {
        const QVariant fallback = params.value(it.key());
        std::optional<NumericTrackStorage> track = buildNumericTrack(it.value());
        if (!track)
            continue;
        m_indices.insert(it.key(), m_tracks.size());
        m_tracks.push_back(std::move(*track));
        m_fallbacks.push_back(isNumericValue(fallback) ? fallback.toDouble() : 0.0);
    }
    m_output.resize(m_tracks.size());
    rebuildViews();
}

NumericTrackBatch::Evaluation NumericTrackBatch::evaluate(int frame) const {
    if (m_views.empty())
        return Evaluation::Empty;
    if (m_hasLastFrame && m_lastFrame == frame)
        return m_lastEvaluationValid ? Evaluation::Cached : Evaluation::Failed;

    m_lastFrame = frame;
    m_hasLastFrame = true;
    m_lastEvaluationValid = RustCore::evaluateNumericTracks(m_views, frame, m_output) ==
                            RustCore::NumericBatchStatus::Ok;
    return m_lastEvaluationValid ? Evaluation::Evaluated : Evaluation::Failed;
}

std::optional<double> NumericTrackBatch::value(const QString &name) const {
    const auto it = m_indices.constFind(name);
    if (!m_lastEvaluationValid || it == m_indices.constEnd() || *it >= m_output.size())
        return std::nullopt;
    return m_output[*it];
}

void NumericTrackBatch::rebuildViews() {
    m_views.resize(m_tracks.size());
    for (std::size_t index = 0; index < m_tracks.size(); ++index)
        m_views[index] = m_tracks[index].view(m_fallbacks[index]);
}

} // namespace AviQtl::Core::RustKeyframes
