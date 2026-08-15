#pragma once

#include "rust_core_abi.hpp"
#include <QStringView>
#include <cstdint>
#include <span>
#include <vector>

namespace AviQtl::RustCore {

enum class TimelineDomainStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
};

enum class SceneGridMode : std::uint32_t {
    Auto = 0,
    Bpm = 1,
    Frame = 2,
};

[[nodiscard]] inline SceneGridMode sceneGridMode(QStringView mode) {
    if (mode == QStringView(u"BPM")) {
        return SceneGridMode::Bpm;
    }
    if (mode == QStringView(u"Frame")) {
        return SceneGridMode::Frame;
    }
    return SceneGridMode::Auto;
}

using SceneSettings = AviQtlSceneSettings;
using IdAllocation = AviQtlIdAllocation;

[[nodiscard]] inline TimelineDomainStatus allocateId(std::span<const std::int32_t> existingIds,
                                                     std::int32_t nextHint,
                                                     std::int32_t minimumId,
                                                     IdAllocation &output) {
    return static_cast<TimelineDomainStatus>(aviqtl_timeline_allocate_id(
        existingIds.data(), existingIds.size(), nextHint, minimumId, &output));
}

[[nodiscard]] inline TimelineDomainStatus normalizeSceneSettings(const SceneSettings &input,
                                                                 SceneSettings &output) {
    return static_cast<TimelineDomainStatus>(
        aviqtl_timeline_normalize_scene_settings(&input, &output));
}

[[nodiscard]] inline std::int32_t snapFrame(double frame, bool ignoreSnap,
                                            const SceneSettings &settings,
                                            double timelineScale) {
    return aviqtl_timeline_snap_frame(frame, ignoreSnap ? 1U : 0U, &settings, timelineScale);
}

[[nodiscard]] inline std::int32_t timelineDuration(
    std::span<const AviQtlTimelineClipGeometry> clips) {
    return aviqtl_timeline_duration(clips.data(), clips.size());
}

[[nodiscard]] inline std::int32_t clampSceneDuration(std::int32_t requestedDuration,
                                                     std::int32_t sceneDuration, double speed,
                                                     std::int32_t offset) {
    return aviqtl_timeline_clamp_scene_duration(requestedDuration, sceneDuration, speed, offset);
}

template <typename Planner>
[[nodiscard]] inline TimelineDomainStatus planIds(std::size_t capacity,
                                                  std::vector<std::int32_t> &output,
                                                  std::int32_t &primary,
                                                  Planner &&planner) {
    output.resize(capacity);
    std::size_t written = 0;
    std::int32_t nextPrimary = primary;
    const auto status = static_cast<TimelineDomainStatus>(
        planner(output.data(), output.size(), &written, &nextPrimary));
    if (status != TimelineDomainStatus::Ok || written > output.size()) {
        output.clear();
        return status == TimelineDomainStatus::Ok ? TimelineDomainStatus::InvalidArgument : status;
    }
    output.resize(written);
    primary = nextPrimary;
    return status;
}

[[nodiscard]] inline TimelineDomainStatus replaceSelection(
    std::span<const std::int32_t> ids, std::int32_t requestedPrimary,
    std::vector<std::int32_t> &output, std::int32_t &primary) {
    return planIds(ids.size(), output, primary,
                   [&](std::int32_t *data, std::size_t capacity, std::size_t *written,
                       std::int32_t *nextPrimary) {
                       return aviqtl_selection_replace(ids.data(), ids.size(), requestedPrimary,
                                                       data, capacity, written, nextPrimary);
                   });
}

[[nodiscard]] inline TimelineDomainStatus toggleSelection(
    std::span<const std::int32_t> currentIds, std::int32_t currentPrimary,
    std::int32_t toggledId, std::vector<std::int32_t> &output, std::int32_t &primary) {
    return planIds(currentIds.size() + 1, output, primary,
                   [&](std::int32_t *data, std::size_t capacity, std::size_t *written,
                       std::int32_t *nextPrimary) {
                       return aviqtl_selection_toggle(
                           currentIds.data(), currentIds.size(), currentPrimary, toggledId, data,
                           capacity, written, nextPrimary);
                   });
}

[[nodiscard]] inline TimelineDomainStatus normalizeRemovalIndices(
    std::size_t length, std::span<const std::int32_t> indices, std::int32_t minimumIndex,
    std::vector<std::int32_t> &output) {
    output.resize(indices.size());
    std::size_t written = 0;
    const auto status = static_cast<TimelineDomainStatus>(
        aviqtl_timeline_normalize_removal_indices(
            length, indices.data(), indices.size(), minimumIndex, output.data(), output.size(),
            &written));
    if (status != TimelineDomainStatus::Ok || written > output.size()) {
        output.clear();
        return status == TimelineDomainStatus::Ok ? TimelineDomainStatus::InvalidArgument : status;
    }
    output.resize(written);
    return status;
}

[[nodiscard]] inline TimelineDomainStatus planIndexMove(
    std::size_t length, std::int32_t oldIndex, std::int32_t newIndex,
    std::int32_t minimumIndex, std::vector<std::int32_t> &redo,
    std::vector<std::int32_t> &undo) {
    redo.resize(length);
    undo.resize(length);
    const auto status = static_cast<TimelineDomainStatus>(aviqtl_timeline_plan_index_move(
        length, oldIndex, newIndex, minimumIndex, redo.data(), undo.data()));
    if (status != TimelineDomainStatus::Ok) {
        redo.clear();
        undo.clear();
    }
    return status;
}

[[nodiscard]] inline TimelineDomainStatus planMultiReorder(
    std::size_t length, std::span<const std::int32_t> indices, std::int32_t targetIndex,
    std::int32_t minimumIndex, std::vector<std::int32_t> &redo,
    std::vector<std::int32_t> &undo, std::size_t &selectedCount) {
    redo.resize(length);
    undo.resize(length);
    const auto status = static_cast<TimelineDomainStatus>(aviqtl_timeline_plan_multi_reorder(
        length, indices.data(), indices.size(), targetIndex, minimumIndex, redo.data(),
        undo.data(), &selectedCount));
    if (status != TimelineDomainStatus::Ok) {
        redo.clear();
        undo.clear();
        selectedCount = 0;
    }
    return status;
}

} // namespace AviQtl::RustCore
