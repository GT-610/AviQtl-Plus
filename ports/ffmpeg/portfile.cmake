# Reuse the vcpkg-maintained FFmpeg 9.0.1 port and add the small MSVC
# Visual Studio's bundled vcpkg does not ship the builtin ports checkout. Keep
# a copy of the exact vcpkg FFmpeg 9.0.1 port in this overlay so the build is
# independent of the host vcpkg layout. The checkout and registry-cache
# fallbacks are useful for local development and remain compatible with older
# working trees that do not yet contain the vendored copy.
set(_ffmpeg_upstream_port_dir "")
set(_ffmpeg_vendored_port_dir "${CMAKE_CURRENT_LIST_DIR}/upstream")
set(_ffmpeg_checkout_port_dir "${VCPKG_ROOT_DIR}/ports/ffmpeg")
if(EXISTS "${_ffmpeg_vendored_port_dir}/portfile.cmake")
    set(_ffmpeg_upstream_port_dir "${_ffmpeg_vendored_port_dir}")
elseif(EXISTS "${_ffmpeg_checkout_port_dir}/portfile.cmake")
    set(_ffmpeg_upstream_port_dir "${_ffmpeg_checkout_port_dir}")
else()
    file(GLOB _ffmpeg_versioned_port_dirs LIST_DIRECTORIES true
        "${VCPKG_ROOT_DIR}/buildtrees/versioning_/versions/ffmpeg/*"
    )
    if(DEFINED ENV{LOCALAPPDATA} AND NOT "$ENV{LOCALAPPDATA}" STREQUAL "")
        file(GLOB _ffmpeg_registry_port_dirs LIST_DIRECTORIES true
            "$ENV{LOCALAPPDATA}/vcpkg/registries/git-trees/*/ports/ffmpeg"
        )
        list(APPEND _ffmpeg_versioned_port_dirs ${_ffmpeg_registry_port_dirs})
    endif()

    foreach(_ffmpeg_candidate IN LISTS _ffmpeg_versioned_port_dirs)
        if(NOT EXISTS "${_ffmpeg_candidate}/portfile.cmake"
           OR NOT EXISTS "${_ffmpeg_candidate}/vcpkg.json")
            continue()
        endif()
        file(READ "${_ffmpeg_candidate}/vcpkg.json" _ffmpeg_candidate_manifest)
        string(REGEX MATCH
            "\"version\"[ \t\r\n]*:[ \t\r\n]*\"9\\.0\\.1\""
            _ffmpeg_candidate_is_901
            "${_ffmpeg_candidate_manifest}"
        )
        if(_ffmpeg_candidate_is_901)
            set(_ffmpeg_upstream_port_dir "${_ffmpeg_candidate}")
            break()
        endif()
    endforeach()
endif()

set(_ffmpeg_upstream_portfile "${_ffmpeg_upstream_port_dir}/portfile.cmake")
if(NOT EXISTS "${_ffmpeg_upstream_portfile}")
    message(FATAL_ERROR
        "The vcpkg FFmpeg 9.0.1 port was not found in the vcpkg checkout or "
        "its versioning/registry cache. VCPKG_ROOT_DIR=${VCPKG_ROOT_DIR}"
    )
endif()

file(READ "${_ffmpeg_upstream_portfile}" _ffmpeg_port_contents)

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

set(_ffmpeg_generated_portfile "${CURRENT_BUILDTREES_DIR}/ffmpeg-overlay-portfile.cmake")
file(WRITE "${_ffmpeg_generated_portfile}" "${_ffmpeg_port_contents}")
include("${_ffmpeg_generated_portfile}")
