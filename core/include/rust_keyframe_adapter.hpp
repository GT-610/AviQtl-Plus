#pragma once

#include "rust_keyframe_core.hpp"
#include <QHash>
#include <QStringView>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <optional>
#include <vector>

namespace AviQtl::Core::RustKeyframes {

[[nodiscard]] bool isNumericValue(const QVariant &value);
[[nodiscard]] RustCore::NumericInterpolation interpolationForName(QStringView name);

struct NumericTrackStorage {
    std::vector<RustCore::NumericKeyframe> keyframes;
    std::vector<double> customPoints;

    [[nodiscard]] RustCore::NumericTrackView view(double fallback) const;
};

[[nodiscard]] std::optional<NumericTrackStorage> buildNumericTrack(const QVariantList &track);
[[nodiscard]] std::optional<double> evaluateNumericTrack(const QVariantList &track, int frame,
                                                         const QVariant &fallback,
                                                         bool discrete = false);

class NumericTrackBatch {
  public:
    enum class Evaluation { Empty, Cached, Evaluated, Failed };

    NumericTrackBatch() = default;
    NumericTrackBatch(const NumericTrackBatch &) = delete;
    NumericTrackBatch &operator=(const NumericTrackBatch &) = delete;
    NumericTrackBatch(NumericTrackBatch &&other) noexcept;
    NumericTrackBatch &operator=(NumericTrackBatch &&other) noexcept;

    void clear();
    void rebuild(const QVariantMap &params, const QHash<QString, QVariantList> &resolvedTracks);

    [[nodiscard]] Evaluation evaluate(int frame) const;
    [[nodiscard]] std::optional<double> value(const QString &name) const;
    [[nodiscard]] std::size_t size() const { return m_views.size(); }

  private:
    void rebuildViews();

    std::vector<NumericTrackStorage> m_tracks;
    std::vector<double> m_fallbacks;
    std::vector<RustCore::NumericTrackView> m_views;
    QHash<QString, std::size_t> m_indices;
    mutable std::vector<double> m_output;
    mutable int m_lastFrame = 0;
    mutable bool m_hasLastFrame = false;
    mutable bool m_lastEvaluationValid = false;
};

} // namespace AviQtl::Core::RustKeyframes
