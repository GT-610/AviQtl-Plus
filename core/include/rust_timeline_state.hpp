#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <cstdint>
#include <span>
#include <vector>

namespace AviQtl::RustCore {

enum class TimelineStateStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
    UnsupportedVersion = AVIQTL_RUST_CORE_STATUS_UNSUPPORTED_VERSION,
    StateConflict = AVIQTL_RUST_CORE_STATUS_STATE_CONFLICT,
};

class TimelineState final {
  public:
    TimelineState() = default;
    TimelineState(const TimelineState &) = delete;
    TimelineState &operator=(const TimelineState &) = delete;
    TimelineState(TimelineState &&other) noexcept;
    TimelineState &operator=(TimelineState &&other) noexcept;
    ~TimelineState();

    [[nodiscard]] bool isValid() const { return m_handle != nullptr; }
    [[nodiscard]] TimelineStateStatus reset(const QByteArray &projectJson,
                                            std::int32_t nextClipHint = 1,
                                            std::int32_t nextSceneHint = 1);
    [[nodiscard]] TimelineStateStatus snapshot(QByteArray &output) const;
    [[nodiscard]] TimelineStateStatus plan(const QByteArray &request,
                                           QByteArray &transaction) const;
    [[nodiscard]] TimelineStateStatus planBatch(const QByteArray &requests,
                                                QByteArray &transaction) const;
    [[nodiscard]] TimelineStateStatus combineTransactions(const QByteArray &first,
                                                          const QByteArray &second,
                                                          QByteArray &transaction) const;
    [[nodiscard]] TimelineStateStatus applyTransaction(const QByteArray &transaction,
                                                       bool forward);
    [[nodiscard]] TimelineStateStatus reserveClipIds(std::size_t count,
                                                     std::vector<std::int32_t> &output);
    [[nodiscard]] TimelineStateStatus reserveSceneIds(std::size_t count,
                                                      std::vector<std::int32_t> &output);
    [[nodiscard]] std::int32_t nextClipId() const;
    [[nodiscard]] std::int32_t nextSceneId() const;
    [[nodiscard]] TimelineStateStatus setNextClipHint(std::int32_t nextHint);

  private:
    AviQtlTimelineState *m_handle = nullptr;
};

} // namespace AviQtl::RustCore
