cmake_minimum_required(VERSION 3.13)

# totk-mod-manager: a borealis homebrew managing the mods merged by
# totk-mod-merger-plugin. Build with ./build.sh, which builds the Rust core
# first (see core/) and then runs this for the Switch.

set(BOREALIS_DIR ${CMAKE_CURRENT_SOURCE_DIR}/library/borealis)
set(BOREALIS_LIBRARY ${BOREALIS_DIR}/library)

include(${BOREALIS_LIBRARY}/cmake/commonOption.cmake)
option(USE_SHARED_LIB "Whether to use shared libs provided by system" OFF)
cmake_dependent_option(USE_SYSTEM_FMT "" OFF "NOT USE_SHARED_LIB" ON)
cmake_dependent_option(USE_SYSTEM_TINYXML2 "" OFF "NOT USE_SHARED_LIB" ON)
cmake_dependent_option(USE_SYSTEM_TWEENY "" OFF "NOT USE_SHARED_LIB" ON)

include(${BOREALIS_LIBRARY}/cmake/toolchain.cmake)

project(totk-mod-manager)
set(VERSION_MAJOR "0")
set(VERSION_MINOR "1")
set(VERSION_ALTER "0")
set(PROJECT_AUTHOR "skyline-totk")
set(PROJECT_TITLE "TotK Mod Manager")
set(PROJECT_ICON ${CMAKE_CURRENT_SOURCE_DIR}/resources/icon.jpg)
set(PROJECT_RESOURCES ${CMAKE_CURRENT_SOURCE_DIR}/resources)

if (NOT PLATFORM_SWITCH)
    message(FATAL_ERROR "totk-mod-manager only builds for the Switch: -DPLATFORM_SWITCH=ON")
endif ()

add_subdirectory(${BOREALIS_LIBRARY} borealis)

find_package(Threads REQUIRED)
# devkitPro portlibs, linked by name: msys2's cmake takes the Windows paths
# pkg-config prints for relative ones.
set(PORTLIBS $ENV{DEVKITPRO}/portlibs/switch)
set(PORTLIBS_LIBS curl archive z bz2 lzma lz4 zstd expat)

# The Rust core (core/), built by build.sh for aarch64-unknown-none. It passes
# -DCORE_LIBRARY when it builds somewhere else than build/ (the container).
if (NOT DEFINED CORE_LIBRARY)
    set(CORE_LIBRARY ${CMAKE_CURRENT_SOURCE_DIR}/build/rust/aarch64-unknown-none/release/libtotk_manager_core.a)
endif ()
if (NOT EXISTS ${CORE_LIBRARY})
    message(FATAL_ERROR "${CORE_LIBRARY} is missing: build the core with ./build.sh")
endif ()
add_library(totk_manager_core STATIC IMPORTED)
set_target_properties(totk_manager_core PROPERTIES IMPORTED_LOCATION ${CORE_LIBRARY})

file(GLOB_RECURSE APP_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/source/*.cpp ${CMAKE_CURRENT_SOURCE_DIR}/source/*.c)
list(APPEND APP_SOURCES ${BOREALIS_LIBRARY}/lib/platforms/switch/switch_wrapper.c)

program_target(${PROJECT_NAME} "${APP_SOURCES}")
set_target_properties(${PROJECT_NAME} PROPERTIES CXX_STANDARD 17)
target_include_directories(${PROJECT_NAME} PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/source
    ${CMAKE_CURRENT_SOURCE_DIR}/core/include)
target_compile_options(${PROJECT_NAME} PRIVATE -ffunction-sections -fdata-sections -Wall -Wno-unused-parameter)
target_compile_definitions(${PROJECT_NAME} PRIVATE
    APP_VERSION="${VERSION_MAJOR}.${VERSION_MINOR}.${VERSION_ALTER}"
    CURL_STATICLIB LIBARCHIVE_STATIC)
# The core is linked twice around the C++ code: it calls back into
# source/core/host.cpp, and the C++ code calls into it.
target_link_libraries(${PROJECT_NAME} PRIVATE
    borealis totk_manager_core ${PORTLIBS_LIBS} totk_manager_core nx ${CMAKE_THREAD_LIBS_INIT})
target_link_options(${PROJECT_NAME} PRIVATE -Wl,--gc-sections)

# romfs: our resources, borealis' own, and the deko3d shaders.
set(ROMFS_DIR ${CMAKE_BINARY_DIR}/romfs)
file(REMOVE_RECURSE ${ROMFS_DIR})
file(COPY ${BOREALIS_DIR}/resources/i18n ${BOREALIS_DIR}/resources/material ${BOREALIS_DIR}/resources/img
    DESTINATION ${ROMFS_DIR}
    PATTERN "demo.json" EXCLUDE
    PATTERN "pokemon" EXCLUDE
    PATTERN "borealis_*" EXCLUDE
    PATTERN "demo_icon.jpg" EXCLUDE
    PATTERN "tiles.png" EXCLUDE)
file(COPY ${PROJECT_RESOURCES}/ DESTINATION ${ROMFS_DIR} PATTERN "icon.jpg" EXCLUDE)
if (USE_DEKO3D)
    gen_dksh("${ROMFS_DIR}/shaders")
endif ()

add_custom_target(${PROJECT_NAME}.nro ALL DEPENDS ${PROJECT_NAME}
    COMMAND ${NX_NACPTOOL_EXE} --create "${PROJECT_TITLE}" "${PROJECT_AUTHOR}"
        "${VERSION_MAJOR}.${VERSION_MINOR}.${VERSION_ALTER}" ${PROJECT_NAME}.nacp
    COMMAND ${NX_ELF2NRO_EXE} ${PROJECT_NAME}.elf ${PROJECT_NAME}.nro
        --icon=${PROJECT_ICON} --nacp=${PROJECT_NAME}.nacp --romfsdir=${ROMFS_DIR}
)
