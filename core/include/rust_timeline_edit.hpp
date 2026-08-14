#pragma once

#include "rust_core_abi.hpp"
#include <cstdint>
#include <span>
#include <vector>

namespace AviQtl::RustCore {

enum class TimelineEditStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    LockedLayer = AVIQTL_RUST_CORE_STATUS_LOCKED_LAYER,
};

using TimelineClipGeometry = AviQtlTimelineClipGeometry;
using TimelineMoveInput = AviQtlTimelineMoveInput;
using TimelinePosition = AviQtlTimelinePosition;

[[nodiscard]] inline TimelineEditStatus findVacantFrame(std::span<const TimelineClipGeometry> clips, std::span<const std::int32_t> excludedIds, std::int32_t layer, std::int32_t startFrame, std::int32_t durationFrames, std::int32_t &outputFrame) {
    return static_cast<TimelineEditStatus>(aviqtl_timeline_find_vacant_frame(clips.data(), clips.size(), excludedIds.data(), excludedIds.size(), layer, startFrame, durationFrames, &outputFrame));
}

[[nodiscard]] inline TimelineEditStatus planBatchMove(std::span<const TimelineClipGeometry> clips, std::span<const TimelineMoveInput> moves, std::span<const std::int32_t> lockedLayers, std::vector<TimelineClipGeometry> &output) {
    output.resize(moves.size());
    const auto status = static_cast<TimelineEditStatus>(aviqtl_timeline_plan_batch_move(clips.data(), clips.size(), moves.data(), moves.size(), lockedLayers.data(), lockedLayers.size(), output.data(), output.size()));
    if (status != TimelineEditStatus::Ok) {
        output.clear();
    }
    return status;
}

[[nodiscard]] inline TimelineEditStatus resolveDrag(std::span<const TimelineClipGeometry> clips, std::span<const std::int32_t> movingIds, std::span<const std::int32_t> lockedLayers, std::int32_t primaryClipId, std::int32_t targetLayer,
                                                    std::int32_t proposedStartFrame, TimelinePosition &output) {
    return static_cast<TimelineEditStatus>(aviqtl_timeline_resolve_drag(clips.data(), clips.size(), movingIds.data(), movingIds.size(), lockedLayers.data(), lockedLayers.size(), primaryClipId, targetLayer, proposedStartFrame, &output));
}

[[nodiscard]] inline TimelineEditStatus planResize(std::span<const TimelineClipGeometry> clips, std::int32_t deltaStartFrame, std::int32_t deltaDurationFrames, std::vector<TimelineClipGeometry> &output) {
    output.resize(clips.size());
    const auto status = static_cast<TimelineEditStatus>(aviqtl_timeline_plan_resize(clips.data(), clips.size(), deltaStartFrame, deltaDurationFrames, output.data(), output.size()));
    if (status != TimelineEditStatus::Ok) {
        output.clear();
    }
    return status;
}

template <typename Planner> [[nodiscard]] inline TimelineEditStatus planVariable(std::span<const TimelineClipGeometry> clips, std::vector<TimelineClipGeometry> &output, Planner &&planner) {
    output.resize(clips.size());
    std::size_t written = 0;
    const auto status = static_cast<TimelineEditStatus>(planner(output.data(), output.size(), &written));
    if (status != TimelineEditStatus::Ok || written > output.size()) {
        output.clear();
        return status == TimelineEditStatus::Ok ? TimelineEditStatus::InvalidArgument : status;
    }
    output.resize(written);
    return status;
}

[[nodiscard]] inline TimelineEditStatus planDeltaMove(std::span<const TimelineClipGeometry> clips, std::span<const std::int32_t> movingIds, std::span<const std::int32_t> lockedLayers, std::int32_t deltaLayer, std::int32_t deltaFrame,
                                                      std::vector<TimelineClipGeometry> &output) {
    return planVariable(clips, output, [&](TimelineClipGeometry *data, std::size_t capacity, std::size_t *written) {
        return aviqtl_timeline_plan_delta_move(clips.data(), clips.size(), movingIds.data(), movingIds.size(), lockedLayers.data(), lockedLayers.size(), deltaLayer, deltaFrame, data, capacity, written);
    });
}

[[nodiscard]] inline TimelineEditStatus planInsertLayers(std::span<const TimelineClipGeometry> clips, std::int32_t targetLayer, std::int32_t count, bool above, std::vector<TimelineClipGeometry> &output) {
    return planVariable(clips, output, [&](TimelineClipGeometry *data, std::size_t capacity, std::size_t *written) { return aviqtl_timeline_plan_insert_layers(clips.data(), clips.size(), targetLayer, count, above ? 1U : 0U, data, capacity, written); });
}

[[nodiscard]] inline TimelineEditStatus planShiftLayers(std::span<const TimelineClipGeometry> clips, std::int32_t startLayer, std::int32_t endLayer, std::int32_t delta, std::vector<TimelineClipGeometry> &output) {
    return planVariable(clips, output, [&](TimelineClipGeometry *data, std::size_t capacity, std::size_t *written) { return aviqtl_timeline_plan_shift_layers(clips.data(), clips.size(), startLayer, endLayer, delta, data, capacity, written); });
}

[[nodiscard]] inline TimelineEditStatus clipboardDuration(std::span<const TimelineClipGeometry> clips, std::int32_t &outputDuration) {
    return static_cast<TimelineEditStatus>(aviqtl_timeline_clipboard_duration(clips.data(), clips.size(), &outputDuration));
}

[[nodiscard]] inline TimelineEditStatus findVacantClipboardFrame(std::span<const TimelineClipGeometry> existing, std::span<const TimelineClipGeometry> clipboard, std::int32_t requestedFrame, std::int32_t layerOffset, std::int32_t &outputFrame) {
    return static_cast<TimelineEditStatus>(aviqtl_timeline_find_vacant_clipboard_frame(existing.data(), existing.size(), clipboard.data(), clipboard.size(), requestedFrame, layerOffset, &outputFrame));
}

[[nodiscard]] inline TimelineEditStatus planClipboardPlacement(std::span<const TimelineClipGeometry> existing, std::span<const TimelineClipGeometry> clipboard, std::int32_t requestedFrame, std::int32_t layerOffset,
                                                               std::vector<TimelineClipGeometry> &output, std::int32_t &outputFrame) {
    output.resize(clipboard.size());
    const auto status = static_cast<TimelineEditStatus>(aviqtl_timeline_plan_clipboard_placement(existing.data(), existing.size(), clipboard.data(), clipboard.size(), requestedFrame, layerOffset, output.data(), output.size(), &outputFrame));
    if (status != TimelineEditStatus::Ok) {
        output.clear();
    }
    return status;
}

[[nodiscard]] inline TimelineEditStatus splitClip(const TimelineClipGeometry &clip, std::int32_t frame, TimelineClipGeometry &first, TimelineClipGeometry &second) {
    return static_cast<TimelineEditStatus>(aviqtl_timeline_split_clip(&clip, frame, &first, &second));
}

} // namespace AviQtl::RustCore
