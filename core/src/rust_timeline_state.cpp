#include "rust_timeline_state.hpp"
#include <functional>
#include <limits>
#include <utility>

namespace AviQtl::RustCore {
namespace {

using JsonCall = std::function<std::uint32_t(AviQtlTimelineState *, std::uint8_t *, std::size_t,
                                             std::size_t *)>;

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
    return collectJson(
        m_handle,
        [input, inputLength](AviQtlTimelineState *handle, std::uint8_t *output,
                             std::size_t capacity, std::size_t *length) {
            return aviqtl_timeline_state_plan_json(handle, input, inputLength, output, capacity,
                                                   length);
        },
        transaction);
}

TimelineStateStatus TimelineState::planBatch(const QByteArray &requests,
                                             QByteArray &transaction) const {
    transaction.clear();
    if (m_handle == nullptr) {
        return TimelineStateStatus::InvalidArgument;
    }
    const auto *input = reinterpret_cast<const std::uint8_t *>(requests.constData());
    const auto inputLength = static_cast<std::size_t>(requests.size());
    return collectJson(
        m_handle,
        [input, inputLength](AviQtlTimelineState *handle, std::uint8_t *output,
                             std::size_t capacity, std::size_t *length) {
            return aviqtl_timeline_state_plan_batch_json(handle, input, inputLength, output,
                                                         capacity, length);
        },
        transaction);
}

TimelineStateStatus TimelineState::combineTransactions(const QByteArray &first,
                                                       const QByteArray &second,
                                                       QByteArray &transaction) const {
    transaction.clear();
    if (m_handle == nullptr) {
        return TimelineStateStatus::InvalidArgument;
    }
    const auto *firstData = reinterpret_cast<const std::uint8_t *>(first.constData());
    const auto firstLength = static_cast<std::size_t>(first.size());
    const auto *secondData = reinterpret_cast<const std::uint8_t *>(second.constData());
    const auto secondLength = static_cast<std::size_t>(second.size());
    return collectJson(
        m_handle,
        [firstData, firstLength, secondData, secondLength](AviQtlTimelineState *,
                                                          std::uint8_t *output,
                                                          std::size_t capacity,
                                                          std::size_t *length) {
            return aviqtl_timeline_transaction_combine_json(
                firstData, firstLength, secondData, secondLength, output, capacity, length);
        },
        transaction);
}

TimelineStateStatus TimelineState::applyTransaction(const QByteArray &transaction, bool forward) {
    if (m_handle == nullptr) {
        return TimelineStateStatus::InvalidArgument;
    }
    return static_cast<TimelineStateStatus>(aviqtl_timeline_state_apply_transaction_json(
        m_handle, reinterpret_cast<const std::uint8_t *>(transaction.constData()),
        static_cast<std::size_t>(transaction.size()), forward ? 1U : 0U));
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
