#include "rust_timeline_state.hpp"
#include <limits>
#include <utility>

namespace AviQtl::RustCore {
namespace {

using JsonCall = std::uint32_t (*)(AviQtlTimelineState *, std::uint8_t *, std::size_t,
                                   std::size_t *);

TimelineStateStatus collectJson(AviQtlTimelineState *handle, JsonCall call, QByteArray &output) {
    output.clear();
    std::size_t required = 0;
    auto status = static_cast<TimelineStateStatus>(call(handle, nullptr, 0, &required));
    if (status != TimelineStateStatus::BufferTooSmall || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        return status == TimelineStateStatus::Ok ? TimelineStateStatus::InvalidArgument : status;
    }
    output.resize(static_cast<qsizetype>(required));
    std::size_t written = 0;
    status = static_cast<TimelineStateStatus>(call(
        handle, reinterpret_cast<std::uint8_t *>(output.data()), required, &written));
    if (status != TimelineStateStatus::Ok || written != required) {
        output.clear();
        return status == TimelineStateStatus::Ok ? TimelineStateStatus::InvalidArgument : status;
    }
    return status;
}

} // namespace

TimelineState::TimelineState(TimelineState &&other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr)) {}

TimelineState &TimelineState::operator=(TimelineState &&other) noexcept {
    if (this != &other) {
        aviqtl_timeline_state_destroy(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

TimelineState::~TimelineState() { aviqtl_timeline_state_destroy(m_handle); }

TimelineStateStatus TimelineState::reset(const QByteArray &projectJson, std::int32_t nextClipHint,
                                         std::int32_t nextSceneHint) {
    const auto *input = reinterpret_cast<const std::uint8_t *>(projectJson.constData());
    const auto inputLength = static_cast<std::size_t>(projectJson.size());
    if (m_handle != nullptr) {
        return static_cast<TimelineStateStatus>(aviqtl_timeline_state_reset(
            m_handle, input, inputLength, nextClipHint, nextSceneHint));
    }
    return static_cast<TimelineStateStatus>(aviqtl_timeline_state_create(
        input, inputLength, nextClipHint, nextSceneHint, &m_handle));
}

TimelineStateStatus TimelineState::snapshot(QByteArray &output) const {
    if (m_handle == nullptr) {
        output.clear();
        return TimelineStateStatus::InvalidArgument;
    }
    return collectJson(m_handle, aviqtl_timeline_state_snapshot_json, output);
}

TimelineStateStatus TimelineState::plan(const QByteArray &request, QByteArray &transaction) const {
    transaction.clear();
    if (m_handle == nullptr) {
        return TimelineStateStatus::InvalidArgument;
    }
    const auto *input = reinterpret_cast<const std::uint8_t *>(request.constData());
    const auto inputLength = static_cast<std::size_t>(request.size());
    std::size_t required = 0;
    auto status = static_cast<TimelineStateStatus>(aviqtl_timeline_state_plan_json(
        m_handle, input, inputLength, nullptr, 0, &required));
    if (status != TimelineStateStatus::BufferTooSmall || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        return status == TimelineStateStatus::Ok ? TimelineStateStatus::InvalidArgument : status;
    }
    transaction.resize(static_cast<qsizetype>(required));
    std::size_t written = 0;
    status = static_cast<TimelineStateStatus>(aviqtl_timeline_state_plan_json(
        m_handle, input, inputLength, reinterpret_cast<std::uint8_t *>(transaction.data()), required,
        &written));
    if (status != TimelineStateStatus::Ok || written != required) {
        transaction.clear();
        return status == TimelineStateStatus::Ok ? TimelineStateStatus::InvalidArgument : status;
    }
    return status;
}

TimelineStateStatus TimelineState::applyPatch(const QByteArray &patch) {
    if (m_handle == nullptr) {
        return TimelineStateStatus::InvalidArgument;
    }
    return static_cast<TimelineStateStatus>(aviqtl_timeline_state_apply_patch_json(
        m_handle, reinterpret_cast<const std::uint8_t *>(patch.constData()),
        static_cast<std::size_t>(patch.size())));
}

TimelineStateStatus TimelineState::reserveClipIds(std::size_t count,
                                                  std::vector<std::int32_t> &output) {
    output.assign(count, 0);
    if (m_handle == nullptr) {
        output.clear();
        return TimelineStateStatus::InvalidArgument;
    }
    const auto status = static_cast<TimelineStateStatus>(aviqtl_timeline_state_reserve_clip_ids(
        m_handle, count, output.data(), output.size()));
    if (status != TimelineStateStatus::Ok) {
        output.clear();
    }
    return status;
}

TimelineStateStatus TimelineState::reserveSceneIds(std::size_t count,
                                                   std::vector<std::int32_t> &output) {
    output.assign(count, 0);
    if (m_handle == nullptr) {
        output.clear();
        return TimelineStateStatus::InvalidArgument;
    }
    const auto status = static_cast<TimelineStateStatus>(aviqtl_timeline_state_reserve_scene_ids(
        m_handle, count, output.data(), output.size()));
    if (status != TimelineStateStatus::Ok) {
        output.clear();
    }
    return status;
}

std::int32_t TimelineState::nextClipId() const {
    return m_handle != nullptr ? aviqtl_timeline_state_next_clip_id(m_handle) : -1;
}

std::int32_t TimelineState::nextSceneId() const {
    return m_handle != nullptr ? aviqtl_timeline_state_next_scene_id(m_handle) : -1;
}

TimelineStateStatus TimelineState::setNextClipHint(std::int32_t nextHint) {
    if (m_handle == nullptr) {
        return TimelineStateStatus::InvalidArgument;
    }
    return static_cast<TimelineStateStatus>(
        aviqtl_timeline_state_set_next_clip_hint(m_handle, nextHint));
}

} // namespace AviQtl::RustCore
