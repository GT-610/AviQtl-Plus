#pragma once

#include "rust_core_abi.hpp"
#include <cstdint>
#include <span>
#include <vector>

namespace AviQtl::RustCore {

enum class ProjectStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
    BufferTooSmall = AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL,
    InvalidJson = AVIQTL_RUST_CORE_STATUS_INVALID_JSON,
    UnsupportedVersion = AVIQTL_RUST_CORE_STATUS_UNSUPPORTED_VERSION,
};

[[nodiscard]] inline std::int32_t currentProjectVersion() { return aviqtl_project_current_version(); }

inline ProjectStatus normalizeProjectJson(std::span<const std::uint8_t> input, std::vector<std::uint8_t> &output) {
    output.clear();
    std::size_t required = 0;
    auto status = static_cast<ProjectStatus>(aviqtl_project_normalize_json(input.data(), input.size(), nullptr, 0, &required));
    if (status != ProjectStatus::BufferTooSmall)
        return status;

    output.resize(required);
    std::size_t written = 0;
    status = static_cast<ProjectStatus>(aviqtl_project_normalize_json(input.data(), input.size(), output.data(), output.size(), &written));
    if (status != ProjectStatus::Ok || written != output.size()) {
        output.clear();
        return status == ProjectStatus::Ok ? ProjectStatus::InvalidArgument : status;
    }
    return status;
}

} // namespace AviQtl::RustCore
