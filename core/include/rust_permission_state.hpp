#pragma once

#include "rust_core_abi.hpp"
#include <QVariantMap>
#include <cstdint>

namespace AviQtl::RustCore {

enum class PermissionStateStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
};

class PermissionState final {
  public:
    PermissionState() = default;
    PermissionState(const PermissionState &) = delete;
    PermissionState &operator=(const PermissionState &) = delete;
    PermissionState(PermissionState &&other) noexcept;
    PermissionState &operator=(PermissionState &&other) noexcept;
    ~PermissionState();

    [[nodiscard]] bool isValid() const { return m_handle != nullptr; }
    [[nodiscard]] PermissionStateStatus reset(const QVariantMap &permissions);
    [[nodiscard]] PermissionStateStatus snapshot(QVariantMap &permissions) const;
    [[nodiscard]] bool has(const QString &pluginId, std::int32_t permission) const;
    [[nodiscard]] PermissionStateStatus grant(const QString &pluginId, std::int32_t permission);
    [[nodiscard]] PermissionStateStatus revoke(const QString &pluginId, std::int32_t permission,
                                               bool &pluginExisted);
    [[nodiscard]] PermissionStateStatus grantAll(const QString &pluginId);
    [[nodiscard]] PermissionStateStatus revokeAll(const QString &pluginId);
    [[nodiscard]] std::uint64_t mask(const QString &pluginId) const;
    [[nodiscard]] bool isAuthorized(const QString &pluginId) const;

  private:
    AviQtlPermissionState *m_handle = nullptr;
};

} // namespace AviQtl::RustCore
