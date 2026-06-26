cmake_minimum_required(VERSION 4.0)
project(AviQtl VERSION 0.4.0 LANGUAGES CXX C)

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)
set(QT_QML_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml)

set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

if(POLICY QTP0004)
    qt_policy(SET QTP0004 NEW)
endif()

# --- Dependencies ---
set(EXTRA_QT_COMPONENTS "")
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    list(APPEND EXTRA_QT_COMPONENTS WaylandClient)
endif()

find_package(Qt6 REQUIRED COMPONENTS
    Core Gui GuiPrivate Quick QuickControls2 Quick3D
    Widgets Core5Compat Sql Svg Multimedia Concurrent
    ShaderTools ShaderToolsPrivate LinguistTools Network Test ${EXTRA_QT_COMPONENTS}
)

if(APPLE)
    find_package(Vulkan)
else()
    find_package(Vulkan REQUIRED)
endif()

find_package(ECM REQUIRED NO_MODULE)
list(APPEND CMAKE_MODULE_PATH ${ECM_MODULE_PATH})
find_package(PkgConfig REQUIRED)

pkg_check_modules(FFMPEG IMPORTED_TARGET REQUIRED libavcodec libavformat libavutil libswscale libswresample)
pkg_check_modules(LUAJIT IMPORTED_TARGET REQUIRED luajit)

# Carla Audio Plugin Backend
if(WIN32)
    if(NOT CARLA_SDK_DIR)
        set(CARLA_SDK_DIR "${CMAKE_SOURCE_DIR}/vendor/carla")
    else()
        file(TO_CMAKE_PATH "${CARLA_SDK_DIR}" CARLA_SDK_DIR)
    endif()
    # Include path is configured independently of library detection
    # CarlaBackend.h comes from source/backend/, CarlaNativePlugin.h comes from source/includes/
    if(EXISTS "${CARLA_SDK_DIR}/include/CarlaNativePlugin.h")
        set(CARLA_INCLUDE_DIRS "${CARLA_SDK_DIR}/include")
    endif()
    # MSYS2/MinGW links .dll directly (no .lib/.dll.a available)
    set(_carla_dll_dir "${CARLA_SDK_DIR}/lib")
    foreach(_carla_name
            libcarla_standalone2
            libcarla_native-plugin
            libcarla_host-plugin
            libcarla_utils)
        # MSVC prefers .lib import library; other environments fall back to .dll
        if(EXISTS "${_carla_dll_dir}/${_carla_name}.lib")
            list(APPEND CARLA_LIBRARIES "${_carla_dll_dir}/${_carla_name}.lib")
        elseif(EXISTS "${_carla_dll_dir}/${_carla_name}.dll")
            list(APPEND CARLA_LIBRARIES "${_carla_dll_dir}/${_carla_name}.dll")
        endif()
    endforeach()
else()
    pkg_check_modules(CARLA IMPORTED_TARGET REQUIRED carla-standalone carla-native-plugin carla-host-plugin carla-utils)
endif()

# --- Common Settings ---
set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_SCAN_FOR_MODULES OFF)  # Disable C++20 modules for clang-tidy compatibility

# Helper to apply consistent warning flags across all AviQtl targets
function(aviqtl_target_warning_flags target)
    target_compile_options(${target} PRIVATE
        $<$<OR:$<CXX_COMPILER_ID:Clang>,$<CXX_COMPILER_ID:GNU>>: -Wall -Wextra -Wpedantic -Wno-unused-parameter>
        $<$<CXX_COMPILER_ID:Clang>: -fcolor-diagnostics -Wno-nullability-extension -Wno-deprecated-declarations -Wno-non-virtual-dtor>
        $<$<CXX_COMPILER_ID:MSVC>: /W4 /permissive- /utf-8>
    )
endfunction()

# --- Module: Core ---
set(CORE_SOURCES
    core/src/audio_decoder.cpp
    core/src/document_model.cpp
    core/src/effect_metadata_i18n.cpp
    core/src/effect_registry.cpp
    core/src/media_decoder.cpp
    core/src/media_utils.cpp
    core/src/package_manager.cpp
    core/src/permission_manager.cpp
    core/src/preset_manager.cpp
    core/src/project_serializer.cpp
    core/src/settings_manager.cpp
    core/src/shader_compiler.cpp
    core/src/theme_controller.cpp
    core/src/video_encoder.cpp
    core/include/audio_decoder.hpp
    core/include/aviqtl_context.hpp
    core/include/document_model.hpp
    core/include/effect_registry.hpp
    core/include/ffmpeg_video_buffer.hpp
    core/include/keyframe_utils.hpp
    core/include/media_decoder.hpp
    core/include/media_utils.hpp
    core/include/package_manager.hpp
    core/include/permission_manager.hpp
    core/include/preset_manager.hpp
    core/include/project_serializer.hpp
    core/include/settings_manager.hpp
    core/include/shader_compiler.hpp
    core/include/theme_controller.hpp
    core/include/version.hpp
    core/include/video_encoder.hpp
)

add_library(AviQtl_Core STATIC ${CORE_SOURCES})
target_include_directories(AviQtl_Core PUBLIC core/include ${CMAKE_SOURCE_DIR})
target_link_libraries(AviQtl_Core PUBLIC Qt6::Core Qt6::Gui Qt6::GuiPrivate Qt6::Widgets Qt6::Concurrent Qt6::Network Qt6::Multimedia Qt6::Qml PkgConfig::FFMPEG PkgConfig::LUAJIT)
target_link_libraries(AviQtl_Core PRIVATE Qt6::ShaderToolsPrivate)
aviqtl_target_warning_flags(AviQtl_Core)

# Define version macros for AviQtl_Core
if(NOT DEFINED AVIQTL_VERSION_MAJOR)
    set(AVIQTL_VERSION_MAJOR "${PROJECT_VERSION_MAJOR}")
endif()
if(NOT DEFINED AVIQTL_VERSION_MINOR)
    set(AVIQTL_VERSION_MINOR "${PROJECT_VERSION_MINOR}")
endif()
if(NOT DEFINED AVIQTL_VERSION_PATCH)
    set(AVIQTL_VERSION_PATCH "${PROJECT_VERSION_PATCH}")
endif()
if(NOT DEFINED AVIQTL_VERSION_STRING)
    set(AVIQTL_VERSION_STRING "${PROJECT_VERSION}")
endif()
target_compile_definitions(AviQtl_Core PUBLIC
    AVIQTL_VERSION_MAJOR=${AVIQTL_VERSION_MAJOR}
    AVIQTL_VERSION_MINOR=${AVIQTL_VERSION_MINOR}
    AVIQTL_VERSION_PATCH=${AVIQTL_VERSION_PATCH}
    AVIQTL_VERSION_STRING="${AVIQTL_VERSION_STRING}"
)

# --- Module: Engine ---
set(ENGINE_SOURCES
    engine/audio_mixer.cpp
    engine/plugin/audio_plugin_chain.cpp
    engine/plugin/audio_plugin_manager.cpp
    engine/timeline/bake_controller.cpp
    engine/timeline/ecs_system.cpp
    engine/timeline/keyframe_evaluator.cpp
    engine/audio_mixer.hpp
    engine/plugin/audio_plugin_chain.hpp
    engine/plugin/audio_plugin_host.hpp
    engine/plugin/audio_plugin_manager.hpp
    engine/timeline/bake_controller.hpp
    engine/timeline/ecs.hpp
    engine/timeline/ecs_profiler.hpp
    engine/timeline/keyframe_evaluator.hpp
)

add_library(AviQtl_Engine STATIC ${ENGINE_SOURCES})
target_include_directories(AviQtl_Engine PUBLIC engine ${CARLA_INCLUDE_DIRS})
if(WIN32)
    target_link_libraries(AviQtl_Engine PUBLIC AviQtl_Core Qt6::Qml Qt6::Quick ${CARLA_LIBRARIES})
else()
    target_link_libraries(AviQtl_Engine PUBLIC AviQtl_Core Qt6::Qml Qt6::Quick PkgConfig::CARLA)
endif()
aviqtl_target_warning_flags(AviQtl_Engine)

# --- Module: Scripting ---
set(SCRIPTING_SOURCES
    scripting/lua_host.cpp
    scripting/mod_engine.cpp
    scripting/script_params.cpp
    scripting/lua_host.hpp
    scripting/mod_engine.hpp
    scripting/script_params.hpp
)

add_library(AviQtl_Scripting STATIC ${SCRIPTING_SOURCES})
target_include_directories(AviQtl_Scripting PUBLIC scripting)
target_link_libraries(AviQtl_Scripting PUBLIC AviQtl_Core Qt6::Qml Qt6::Quick PkgConfig::LUAJIT)
aviqtl_target_warning_flags(AviQtl_Scripting)

# --- Module: UI (Bridge & Controllers) ---
# UI-dependent files that live under core/ but conceptually belong to the UI layer
set(CORE_UI_DEPENDENT_SOURCES
    core/src/compute_render_node.cpp
    core/src/image_decoder.cpp
    core/src/video_decoder.cpp
    core/src/video_frame_provider.cpp
    core/src/video_frame_store.cpp
    core/include/compute_render_node.hpp
    core/include/effect_model.hpp
    core/include/image_decoder.hpp
    core/include/video_decoder.hpp
    core/include/video_frame_provider.hpp
    core/include/video_frame_store.hpp
)

set(UI_SOURCES
    ui/src/bridge/ecs_render_bridge.cpp
    ui/src/selection_service.cpp
    ui/src/timeline/timeline_clip.cpp
    ui/src/timeline/timeline_commands.cpp
    ui/src/timeline/timeline_controller_clip.cpp
    ui/src/timeline/timeline_controller_export.cpp
    ui/src/timeline/timeline_controller_project.cpp
    ui/src/timeline/timeline_controller_scene.cpp
    ui/src/timeline/timeline_effect.cpp
    ui/src/timeline/timeline_export_manager.cpp
    ui/src/timeline/timeline_layer.cpp
    ui/src/timeline/timeline_media_manager.cpp
    ui/src/timeline/timeline_scene.cpp
    ui/src/timeline_controller.cpp
    ui/src/timeline_service.cpp
    ui/src/transport_service.cpp
    ui/src/window_manager.cpp
    ui/src/workspace.cpp
    ui/include/bridge/ecs_render_bridge.hpp
    ui/include/commands.hpp
    ui/include/project_service.hpp
    ui/include/selection_service.hpp
    ui/include/timeline_controller.hpp
    ui/include/timeline_export_manager.hpp
    ui/include/timeline_media_manager.hpp
    ui/include/timeline_service.hpp
    ui/include/timeline_types.hpp
    ui/include/transport_service.hpp
    ui/include/window_manager.hpp
    ui/include/workspace.hpp
    ${CORE_UI_DEPENDENT_SOURCES}
)

add_library(AviQtl_UI STATIC ${UI_SOURCES})
target_include_directories(AviQtl_UI PUBLIC ui/include core/include)
target_link_libraries(AviQtl_UI PUBLIC AviQtl_Engine AviQtl_Scripting Qt6::Quick Qt6::QuickControls2 Qt6::Quick3D Qt6::Multimedia Qt6::Svg Qt6::Core5Compat Qt6::GuiPrivate Vulkan::Vulkan)
aviqtl_target_warning_flags(AviQtl_UI)

# --- QML Resources & Module ---
# Effects and objects are loaded at runtime as external modules and are not embedded here.
set(QML_FILES
    ui/qml/AboutWindow.qml
    ui/qml/common/AudioKfParamTrack.qml
    ui/qml/common/AudioPluginMenu.qml
    ui/qml/common/AviQtlIcon.qml
    ui/qml/common/AviQtlWindow.qml
    ui/qml/common/BaseComputeEffect.qml
    ui/qml/common/BaseEffect.qml
    ui/qml/common/BaseObject.qml
    ui/qml/common/ControlLoader.qml
    ui/qml/common/DisplayModel.qml
    ui/qml/common/EasingConfigWindow.qml
    ui/qml/common/IconMenuItem.qml
    ui/qml/common/NodeLoader.qml
    ui/qml/common/ObjectRenderer.qml
    ui/qml/common/ParamControl.qml
    ui/qml/CompositeView.qml
    ui/qml/ExportDialog.qml
    ui/qml/MainWindow.qml
    ui/qml/PackageManagerWindow.qml
    ui/qml/PluginPermissionDialog.qml
    ui/qml/ProjectLauncherWindow.qml
    ui/qml/ProjectSettingsWindow.qml
    ui/qml/SceneRenderer.qml
    ui/qml/SceneSettingsWindow.qml
    ui/qml/SettingDialog.qml
    ui/qml/settings/AppearanceSettingsPage.qml
    ui/qml/settings/DecodeAudioSettingsPage.qml
    ui/qml/settings/ExportSettingsPage.qml
    ui/qml/settings/GeneralSettingsPage.qml
    ui/qml/settings/PerformanceSettingsPage.qml
    ui/qml/settings/PluginSettingsPage.qml
    ui/qml/settings/ProjectSettingsPage.qml
    ui/qml/settings/ShortcutSettingsPage.qml
    ui/qml/settings/TimelineSettingsPage.qml
    ui/qml/SystemSettingsWindow.qml
    ui/qml/timeline/ClipItem.qml
    ui/qml/timeline/LayerHeader.qml
    ui/qml/timeline/Ruler.qml
    ui/qml/timeline/TimelineGrid.qml
    ui/qml/timeline/TimelineView.qml
    ui/qml/TimelineWindow.qml
    ui/qml/common/Icons.js
    ui/qml/common/Logger.js
    ui/qml/common/BlendModeUtils.js
)

qt_add_qml_module(AviQtl_UI
    URI "AviQtl"
    VERSION 1.0
    RESOURCE_PREFIX "/qt/qml"
    SOURCES
        "core/src/compute_effect.cpp"
        "core/include/compute_effect.hpp"
    QML_FILES
        ${QML_FILES}
)

# --- External Assets: Shaders Compilation ---
# Shaders are treated as data files; they are compiled to .qsb so BUILD.py can pick them up.
# Using GLOB_RECURSE here is an intentional exception: every effect/object adds a matching
# .frag file, and they are compiled generically. Source changes that add/remove shaders
# still require a CMake reconfigure (e.g. touch CMakeLists.txt or use --fresh).
set(EXTERNAL_QSB_FILES "")

# Compile shaders from ui/qml/ (built-in effects)
foreach(DIR "effects" "objects" "transitions" "common/shaders")
    file(GLOB_RECURSE SHADERS RELATIVE ${CMAKE_CURRENT_SOURCE_DIR} "ui/qml/${DIR}/*.frag" "ui/qml/${DIR}/*.vert" "ui/qml/${DIR}/*.comp")
    foreach(SH ${SHADERS})
        get_filename_component(SH_FILE_NAME ${SH} NAME)
        get_filename_component(SH_EXT ${SH} EXT)

        set(OUTPUT_QSB "${CMAKE_BINARY_DIR}/${DIR}/${SH_FILE_NAME}.qsb")
        list(APPEND EXTERNAL_QSB_FILES ${OUTPUT_QSB})

        set(QSB_ARGS "--glsl" "100es,120,150" "--hlsl" "50" "--msl" "12")
        if(SH_EXT STREQUAL ".comp")
            set(QSB_ARGS "--glsl" "310es,430" "--hlsl" "50" "--msl" "12")
        endif()

        add_custom_command(
            OUTPUT ${OUTPUT_QSB}
            COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_BINARY_DIR}/${DIR}"
            COMMAND Qt6::qsb ${QSB_ARGS} -o ${OUTPUT_QSB} "${CMAKE_SOURCE_DIR}/${SH}"
            DEPENDS "${CMAKE_SOURCE_DIR}/${SH}"
            VERBATIM
        )
    endforeach()
endforeach()

# Compile shaders from effect-packages/ (external effect packages)
file(GLOB_RECURSE EFFECT_PACKAGE_SHADERS RELATIVE ${CMAKE_CURRENT_SOURCE_DIR}
    "effect-packages/**/*.frag" "effect-packages/**/*.vert" "effect-packages/**/*.comp")
foreach(SH ${EFFECT_PACKAGE_SHADERS})
    get_filename_component(SH_DIR ${SH} DIRECTORY)
    get_filename_component(SH_FILE_NAME ${SH} NAME)
    get_filename_component(SH_EXT ${SH} EXT)

    set(OUTPUT_QSB "${CMAKE_BINARY_DIR}/${SH_DIR}/${SH_FILE_NAME}.qsb")
    list(APPEND EXTERNAL_QSB_FILES ${OUTPUT_QSB})

    set(QSB_ARGS "--glsl" "100es,120,150" "--hlsl" "50" "--msl" "12")
    if(SH_EXT STREQUAL ".comp")
        set(QSB_ARGS "--glsl" "310es,430" "--hlsl" "50" "--msl" "12")
    endif()

    add_custom_command(
        OUTPUT ${OUTPUT_QSB}
        COMMAND ${CMAKE_COMMAND} -E make_directory "${CMAKE_BINARY_DIR}/${SH_DIR}"
        COMMAND Qt6::qsb ${QSB_ARGS} -o ${OUTPUT_QSB} "${CMAKE_SOURCE_DIR}/${SH}"
        DEPENDS "${CMAKE_SOURCE_DIR}/${SH}"
        VERBATIM
    )
endforeach()

add_custom_target(AviQtl_ExternalShaders ALL DEPENDS ${EXTERNAL_QSB_FILES})

# --- Executable ---
set(AVIQTL_EXECUTABLE_SOURCES
    core/src/main.cpp
    ui/resources.qrc
)

if(WIN32)
    list(APPEND AVIQTL_EXECUTABLE_SOURCES assets/windows/aviqtl.rc)
endif()

qt_add_executable(AviQtl
    ${AVIQTL_EXECUTABLE_SOURCES}
)

if(APPLE)
    set_target_properties(AviQtl PROPERTIES
        MACOSX_BUNDLE TRUE
        MACOSX_BUNDLE_BUNDLE_NAME "AviQtl"
        MACOSX_BUNDLE_GUI_IDENTIFIER "org.aviqtl.AviQtl"
        MACOSX_BUNDLE_BUNDLE_VERSION "${PROJECT_VERSION}"
        MACOSX_BUNDLE_SHORT_VERSION_STRING "${PROJECT_VERSION}"
    )
endif()

add_dependencies(AviQtl AviQtl_ExternalShaders)

foreach(AVIQTL_EXTERNAL_ASSET_DIR effects objects transitions)
    add_custom_command(TARGET AviQtl POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory
            "${CMAKE_BINARY_DIR}/${AVIQTL_EXTERNAL_ASSET_DIR}"
        COMMAND ${CMAKE_COMMAND} -E copy_directory
            "${CMAKE_CURRENT_SOURCE_DIR}/ui/qml/${AVIQTL_EXTERNAL_ASSET_DIR}"
            "${CMAKE_BINARY_DIR}/${AVIQTL_EXTERNAL_ASSET_DIR}"
        COMMAND ${CMAKE_COMMAND} -E copy_directory
            "${CMAKE_CURRENT_SOURCE_DIR}/ui/qml/${AVIQTL_EXTERNAL_ASSET_DIR}"
            "$<TARGET_FILE_DIR:AviQtl>/${AVIQTL_EXTERNAL_ASSET_DIR}"
        COMMAND ${CMAKE_COMMAND} -E copy_directory
            "${CMAKE_BINARY_DIR}/${AVIQTL_EXTERNAL_ASSET_DIR}"
            "$<TARGET_FILE_DIR:AviQtl>/${AVIQTL_EXTERNAL_ASSET_DIR}"
        VERBATIM
    )
endforeach()

# Copy effect-packages/ to build output (source files + compiled .qsb)
add_custom_command(TARGET AviQtl POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_directory
        "${CMAKE_CURRENT_SOURCE_DIR}/effect-packages"
        "$<TARGET_FILE_DIR:AviQtl>/effect-packages"
    COMMAND ${CMAKE_COMMAND} -E copy_directory
        "${CMAKE_BINARY_DIR}/effect-packages"
        "$<TARGET_FILE_DIR:AviQtl>/effect-packages"
    VERBATIM
)

add_custom_command(TARGET AviQtl POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "$<TARGET_FILE:AviQtl>"
        "${CMAKE_BINARY_DIR}/$<TARGET_FILE_NAME:AviQtl>"
    VERBATIM
)

target_link_libraries(AviQtl PRIVATE AviQtl_UI)
aviqtl_target_warning_flags(AviQtl)

# --- Translation ---
set(TS_FILES
    i18n/AviQtl_en_US.ts
    i18n/AviQtl_ja_JP.ts
    i18n/AviQtl_zh_CN.ts
)

if(TS_FILES)
    qt_add_translations(AviQtl TS_FILES ${TS_FILES})
    add_dependencies(AviQtl AviQtl_lupdate AviQtl_lrelease)
    set(QM_FILES "")
    foreach(TS_FILE ${TS_FILES})
        get_filename_component(TS_NAME "${TS_FILE}" NAME_WE)
        list(APPEND QM_FILES "${CMAKE_BINARY_DIR}/${TS_NAME}.qm")
    endforeach()
    if(APPLE)
        set(AVIQTL_I18N_OUTPUT_DIR "$<TARGET_BUNDLE_DIR:AviQtl>/Contents/Resources/i18n")
    else()
        set(AVIQTL_I18N_OUTPUT_DIR "$<TARGET_FILE_DIR:AviQtl>/i18n")
    endif()
    add_custom_command(TARGET AviQtl POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory "${AVIQTL_I18N_OUTPUT_DIR}"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different ${QM_FILES} "${AVIQTL_I18N_OUTPUT_DIR}"
        VERBATIM
    )
endif()

# --- Platform Specific ---
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    target_link_options(AviQtl PRIVATE "-rdynamic")
    target_link_libraries(AviQtl PRIVATE Qt6::WaylandClient)
elseif(MSVC)
    target_link_libraries(AviQtl PRIVATE opengl32)
    set_target_properties(AviQtl PROPERTIES MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")
endif()

# --- Link-time optimization ---
# LTO is kept at the executable level because enabling it on every static library
# can inflate incremental build times. Individual targets still get full warnings.
target_link_options(AviQtl PRIVATE
    $<$<AND:$<CXX_COMPILER_ID:Clang>,$<PLATFORM_ID:Linux>>: -flto=thin>
    $<$<AND:$<CXX_COMPILER_ID:MSVC>,$<CONFIG:Release>>: /LTCG>
)

# --- Linker selection ---
if(CMAKE_SYSTEM_NAME STREQUAL "Linux" AND NOT DEFINED CMAKE_LINKER_TYPE)
    find_program(MOLD_LINKER mold)
    if(MOLD_LINKER)
        set(CMAKE_LINKER_TYPE MOLD CACHE STRING "" FORCE)
    endif()
endif()

# --- Tests ---
enable_testing()
add_subdirectory(tests)
