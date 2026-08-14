#pragma once

#include "rust_core_abi.hpp"

namespace AviQtl::RustCore {

enum class TimelineBakeStatus : std::uint32_t {
    Ok = AVIQTL_RUST_CORE_STATUS_OK,
    InvalidArgument = AVIQTL_RUST_CORE_STATUS_INVALID_ARGUMENT,
    OverlappingBuffers = AVIQTL_RUST_CORE_STATUS_OVERLAPPING_BUFFERS,
};

using RenderBakeInput = AviQtlRenderBakeInput;
using RenderBakeOutput = AviQtlRenderBakeOutput;
using AudioBakeInput = AviQtlAudioBakeInput;
using AudioBakeOutput = AviQtlAudioBakeOutput;

[[nodiscard]] inline TimelineBakeStatus bakeRender(const RenderBakeInput &input,
                                                   RenderBakeOutput &output) {
    return static_cast<TimelineBakeStatus>(aviqtl_timeline_bake_render(&input, &output));
}

[[nodiscard]] inline TimelineBakeStatus bakeAudio(const AudioBakeInput &input,
                                                  AudioBakeOutput &output) {
    return static_cast<TimelineBakeStatus>(aviqtl_timeline_bake_audio(&input, &output));
}

} // namespace AviQtl::RustCore
