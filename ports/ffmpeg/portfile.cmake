# Reuse the vcpkg-maintained FFmpeg 9.0.1 port and add the small MSVC
# compatibility patch kept in this repository.

set(_ffmpeg_upstream_port_dir "${VCPKG_ROOT_DIR}/ports/ffmpeg")
set(_ffmpeg_upstream_portfile "${_ffmpeg_upstream_port_dir}/portfile.cmake")
set(_ffmpeg_compat_patch "${CMAKE_CURRENT_LIST_DIR}/0008-msvc-no-stdalign.patch")

if(NOT EXISTS "${_ffmpeg_upstream_portfile}")
    message(FATAL_ERROR "The vcpkg FFmpeg port was not found at ${_ffmpeg_upstream_portfile}")
endif()

file(READ "${_ffmpeg_upstream_portfile}" _ffmpeg_port_contents)

# Windows SDK 10.0.20348.0 (shipped by VS2022 17.4+) provides stdalign.h.
# Keep the compatibility patch only for older MSVC toolsets, which is the
# case for VS2019 16.x / _MSC_VER 1929.
vcpkg_cmake_get_vars(_ffmpeg_cmake_vars_file)
include("${_ffmpeg_cmake_vars_file}")
set(_ffmpeg_needs_std_align_compat OFF)
if(VCPKG_DETECTED_MSVC)
    if(NOT VCPKG_DETECTED_MSVC_VERSION OR VCPKG_DETECTED_MSVC_VERSION LESS 1934)
        set(_ffmpeg_needs_std_align_compat ON)
    endif()
endif()

# The upstream port resolves its helper files relative to its own directory.
# The generated copy keeps those references valid while allowing us to append
# the project-specific patch to vcpkg_from_github(PATCHES ...).
string(REPLACE "\${CMAKE_CURRENT_LIST_DIR}" "${_ffmpeg_upstream_port_dir}" _ffmpeg_port_contents "${_ffmpeg_port_contents}")
# PATCHES entries in the generated copy are no longer relative to the
# upstream port directory, so make the vcpkg patch paths explicit as well.
set(_ffmpeg_patch_names
    0003-fix-windowsinclude.patch
    0004-dependencies.patch
    0005-fix-nasm.patch
    0007-fix-lib-naming.patch
    0013-define-WINVER.patch
    0024-fix-osx-host-c11.patch
    0040-ffmpeg-add-av_stream_get_first_dts-for-chromium.patch
    0045-use-prebuilt-bin2c.patch
    0046-fix-msvc-detection.patch
    0047-fix-msvc-utf8.patch
    0049-fix-twolame-pkgconfig.patch
    0050-fix-test-ld-absolute-lib-paths.patch
    0051-fix-msvc-undef-flags.patch
    0052-fix-disable-unstable-swscale-link.patch
)
foreach(_ffmpeg_patch_name IN LISTS _ffmpeg_patch_names)
    string(REPLACE
        "        ${_ffmpeg_patch_name}"
        "        ${_ffmpeg_upstream_port_dir}/${_ffmpeg_patch_name}"
        _ffmpeg_port_contents
        "${_ffmpeg_port_contents}"
    )
endforeach()
if(_ffmpeg_needs_std_align_compat)
    string(REPLACE
        "        ${_ffmpeg_upstream_port_dir}/0052-fix-disable-unstable-swscale-link.patch"
        "        ${_ffmpeg_upstream_port_dir}/0052-fix-disable-unstable-swscale-link.patch\n        ${_ffmpeg_compat_patch}"
        _ffmpeg_port_contents
        "${_ffmpeg_port_contents}"
    )
endif()

set(_ffmpeg_generated_portfile "${CURRENT_BUILDTREES_DIR}/ffmpeg-overlay-portfile.cmake")
file(WRITE "${_ffmpeg_generated_portfile}" "${_ffmpeg_port_contents}")
include("${_ffmpeg_generated_portfile}")
