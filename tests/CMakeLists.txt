# --- Test Infrastructure ---
# Each subdirectory under tests/ corresponds to a test category.
# They are linked as standalone executables and registered with CTest.

# --- Dependencies ---
find_package(Qt6 REQUIRED COMPONENTS Test)

if(POLICY CMP0110)
    cmake_policy(SET CMP0110 NEW)
endif()

# --- Helper: Add a single test target ---
# Usage:
#   aviqtl_add_test(NAME effect_registry
#       SOURCES test_effect_registry.cpp
#       LINKS AviQtl_Core
#   )
function(aviqtl_add_test)
    set(oneValueArgs NAME)
    set(multiValueArgs SOURCES LINKS)
    cmake_parse_arguments(ARG "" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    set(TARGET_NAME "AviQtl_Test_${ARG_NAME}")
    add_executable(${TARGET_NAME} ${ARG_SOURCES})

    target_link_libraries(${TARGET_NAME} PRIVATE Qt6::Test)
    if(ARG_LINKS)
        target_link_libraries(${TARGET_NAME} PRIVATE ${ARG_LINKS})
    endif()

    # Register with CTest
    # Force Qt Test output to stdout. On Windows, the default logger can write
    # to the debugger instead, leaving CTest failures with no diagnostics.
    add_test(NAME ${ARG_NAME} COMMAND ${TARGET_NAME} -o -,txt)

    # On Windows, ensure vendor DLLs (e.g., Carla) are in PATH for runtime loading.
    # ENVIRONMENT_MODIFICATION handles platform-specific path separators correctly.
    if(WIN32)
        set_tests_properties(${ARG_NAME} PROPERTIES
            ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:${CMAKE_SOURCE_DIR}/vendor/carla/lib"
        )
    endif()
endfunction()

# --- Core Tests ---
aviqtl_add_test(
    NAME effect_registry
    SOURCES test_effect_registry.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME effect_registry_load
    SOURCES test_effect_registry_load.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME shader_compiler
    SOURCES test_shader_compiler.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME effect_package
    SOURCES test_effect_package.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME document_model
    SOURCES test_document_model.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME project_serializer
    SOURCES test_project_serializer.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME settings_manager
    SOURCES test_settings_manager.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME preset_manager
    SOURCES test_preset_manager.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME theme_controller
    SOURCES test_theme_controller.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME media_decoder
    SOURCES test_media_decoder.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME video_encoder
    SOURCES test_video_encoder.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME media_utils
    SOURCES test_media_utils.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME keyframe_utils
    SOURCES test_keyframe_utils.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME effect_model
    SOURCES test_effect_model.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME audio_decoder
    SOURCES test_audio_decoder.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME video_decoder
    SOURCES test_video_decoder.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME video_frame_store
    SOURCES test_video_frame_store.cpp
    LINKS AviQtl_UI
)

# --- Engine Tests ---
aviqtl_add_test(
    NAME dense_component_map
    SOURCES test_dense_component_map.cpp
    LINKS AviQtl_Engine
)

aviqtl_add_test(
    NAME audio_plugin_chain
    SOURCES test_audio_plugin_chain.cpp
    LINKS AviQtl_Engine
)

aviqtl_add_test(
    NAME bake_controller
    SOURCES test_bake_controller.cpp
    LINKS AviQtl_Engine AviQtl_UI
)

aviqtl_add_test(
    NAME ecs_render
    SOURCES test_ecs_render.cpp
    LINKS AviQtl_Engine AviQtl_UI
)

aviqtl_add_test(
    NAME triple_buffer
    SOURCES test_triple_buffer.cpp
    LINKS AviQtl_Engine
)

# --- Scripting Tests ---
aviqtl_add_test(
    NAME lua_host
    SOURCES test_lua_host.cpp
    LINKS AviQtl_Scripting
)

aviqtl_add_test(
    NAME plugin_manifest
    SOURCES test_plugin_manifest.cpp
    LINKS AviQtl_Scripting AviQtl_UI
)

aviqtl_add_test(
    NAME mod_engine
    SOURCES test_mod_engine.cpp
    LINKS AviQtl_Scripting
)

# --- Plugin System Tests ---
aviqtl_add_test(
    NAME permission_manager
    SOURCES test_permission_manager.cpp
    LINKS AviQtl_Core
)

aviqtl_add_test(
    NAME package_deploy
    SOURCES test_package_deploy.cpp
    LINKS AviQtl_Core Qt6::CorePrivate
)

# --- UI Tests ---
aviqtl_add_test(
    NAME selection_service
    SOURCES test_selection_service.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME project_service
    SOURCES test_project_service.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME transport_service
    SOURCES test_transport_service.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME timeline_batch_move
    SOURCES test_timeline_batch_move.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME large_timeline_performance
    SOURCES test_large_timeline_performance.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME clip_by_upper_object
    SOURCES test_clip_by_upper_object.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME daily_editing_workflow
    SOURCES test_daily_editing_workflow.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME missing_media
    SOURCES test_missing_media.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME export_workflow
    SOURCES test_export_workflow.cpp
    LINKS AviQtl_UI
)

aviqtl_add_test(
    NAME qml_composite_capture
    SOURCES test_qml_composite_capture.cpp
    LINKS AviQtl_UI
)
target_compile_definitions(AviQtl_Test_qml_composite_capture PRIVATE
    AVIQTL_SOURCE_DIR="${CMAKE_SOURCE_DIR}"
    AVIQTL_DEPLOYED_EFFECTS_DIR="$<TARGET_FILE_DIR:AviQtl>/effects"
)
add_dependencies(AviQtl_Test_qml_composite_capture AviQtl)

aviqtl_add_test(
    NAME transition_effects
    SOURCES test_transition_effects.cpp
    LINKS AviQtl_Core
)
target_compile_definitions(AviQtl_Test_transition_effects PRIVATE
    AVIQTL_SOURCE_DIR="${CMAKE_SOURCE_DIR}"
)

# Labels keep focused developer runs concise without changing the default suite.
function(aviqtl_label_tests LABEL)
    set_property(TEST ${ARGN} APPEND PROPERTY LABELS "${LABEL}")
endfunction()

aviqtl_label_tests(unit
    effect_registry shader_compiler document_model settings_manager
    theme_controller media_decoder media_utils keyframe_utils effect_model
    video_frame_store dense_component_map audio_plugin_chain triple_buffer
    lua_host plugin_manifest mod_engine permission_manager selection_service
    project_service transport_service timeline_batch_move clip_by_upper_object
    transition_effects
)

aviqtl_label_tests(integration
    effect_registry_load effect_package project_serializer preset_manager
    video_encoder audio_decoder video_decoder bake_controller ecs_render
    package_deploy large_timeline_performance daily_editing_workflow
    missing_media export_workflow qml_composite_capture
)

aviqtl_label_tests(media
    video_encoder audio_decoder video_decoder video_frame_store
    audio_plugin_chain bake_controller ecs_render daily_editing_workflow
    export_workflow qml_composite_capture
)

aviqtl_label_tests(qml large_timeline_performance qml_composite_capture)
aviqtl_label_tests(performance large_timeline_performance)
