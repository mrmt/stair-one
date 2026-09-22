# Stair One の AU / VST3 / Standalone。make au (リポジトリ直下の Makefile) から呼ぶ
cmake_minimum_required(VERSION 3.22)

set(CMAKE_OSX_ARCHITECTURES "arm64;x86_64" CACHE STRING "")
set(CMAKE_OSX_DEPLOYMENT_TARGET "11.0" CACHE STRING "")

# バージョンはタグ (au-v0.2.0 など) から。無ければ 0.0.0
set(STAIR_VERSION "0.0.0" CACHE STRING "プラグインのバージョン")
project(StairOne VERSION ${STAIR_VERSION} LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)

include(FetchContent)
FetchContent_Declare(JUCE
    GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
    GIT_TAG 8.0.15
    GIT_SHALLOW TRUE)
FetchContent_MakeAvailable(JUCE)

# engine/ffi の staticlib (arm64 + x86_64 を lipo でまとめたもの)
set(STAIR_FFI_LIB "" CACHE FILEPATH "libstair_ffi.a")
if(NOT EXISTS "${STAIR_FFI_LIB}")
    message(FATAL_ERROR "STAIR_FFI_LIB に libstair_ffi.a を指定する (make au が作る)")
endif()

juce_add_plugin(StairOne
    COMPANY_NAME "mrmt"
    BUNDLE_ID "com.mrmt.stairone"
    PLUGIN_MANUFACTURER_CODE Mrmt
    PLUGIN_CODE Str1
    FORMATS AU VST3 Standalone
    PRODUCT_NAME "Stair One"
    DESCRIPTION "16-pad noise instrument"
    IS_SYNTH TRUE
    NEEDS_MIDI_INPUT TRUE
    NEEDS_MIDI_OUTPUT FALSE
    IS_MIDI_EFFECT FALSE
    NEEDS_WEB_BROWSER TRUE
    AU_MAIN_TYPE kAudioUnitType_MusicDevice
    VST3_CATEGORIES Instrument Synth
    COPY_PLUGIN_AFTER_BUILD FALSE)

# 画面は Web 版と同じ index.html
juce_add_binary_data(StairOneWeb SOURCES ../index.html)

target_sources(StairOne PRIVATE src/Processor.cpp src/Editor.cpp)
target_include_directories(StairOne PRIVATE src)

target_compile_definitions(StairOne PUBLIC
    JUCE_WEB_BROWSER=1
    JUCE_USE_CURL=0
    JUCE_VST3_CAN_REPLACE_VST2=0)

target_link_libraries(StairOne
    PRIVATE
        StairOneWeb
        juce::juce_audio_utils
        juce::juce_gui_extra
        ${STAIR_FFI_LIB}
    PUBLIC
        juce::juce_recommended_config_flags
        juce::juce_recommended_lto_flags
        juce::juce_recommended_warning_flags)
