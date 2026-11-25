# CMake toolchain file: build C/C++ programs and libraries for the OCTAGRAM
# userspace ABI (x86_64, static ET_EXEC, user/linker.ld, OCTAGRAM libc/libc++).
#
# Usage:
#   cmake -G Ninja -DCMAKE_TOOLCHAIN_FILE=user/toolchain/octagram-toolchain.cmake ...
#
# Prerequisites:
#   make -C user sysroot          # produces user/build/sysroot-octagram
#
# Interpretation notes:
#   * CMAKE_SYSTEM_NAME is Generic: there is no OS detection to run on the
#     target, and no target-side execute step is possible from the host.
#   * try_compile links nothing (STATIC_LIBRARY), because a linked test program
#     cannot run on the host anyway.
#   * C++ is built freestanding without exceptions and RTTI, matching
#     CXXFLAGS in user/Makefile and the annotations libc++ 18 supports.

get_filename_component(OCTAGRAM_TOOLCHAIN_DIR "${CMAKE_CURRENT_LIST_FILE}" DIRECTORY)
get_filename_component(OCTAGRAM_REPO_ROOT "${OCTAGRAM_TOOLCHAIN_DIR}/../.." ABSOLUTE)

set(OCTAGRAM_SYSROOT
    "${OCTAGRAM_REPO_ROOT}/user/build/sysroot-octagram"
    CACHE PATH "OCTAGRAM userspace sysroot produced by 'make -C user sysroot'")

if(NOT EXISTS "${OCTAGRAM_SYSROOT}/usr/include")
  message(FATAL_ERROR
    "OCTAGRAM sysroot not found at ${OCTAGRAM_SYSROOT}. Run 'make -C user sysroot' first.")
endif()

set(CMAKE_SYSTEM_NAME Generic)
set(CMAKE_SYSTEM_PROCESSOR x86_64)

set(CMAKE_C_COMPILER "${OCTAGRAM_TOOLCHAIN_DIR}/bin/x86_64-octagram-gcc")
set(CMAKE_CXX_COMPILER "${OCTAGRAM_TOOLCHAIN_DIR}/bin/x86_64-octagram-g++")

# Archivers stay host tools: ar/ranlib only rearrange ELF objects.
find_program(OCTAGRAM_HOST_AR NAMES ar llvm-ar REQUIRED)
find_program(OCTAGRAM_HOST_RANLIB NAMES ranlib llvm-ranlib REQUIRED)
set(CMAKE_AR "${OCTAGRAM_HOST_AR}" CACHE FILEPATH "")
set(CMAKE_RANLIB "${OCTAGRAM_HOST_RANLIB}" CACHE FILEPATH "")

set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
set(CMAKE_CROSSCOMPILING_EMULATOR "" CACHE STRING "" FORCE)

set(CMAKE_FIND_ROOT_PATH "${OCTAGRAM_SYSROOT}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# The drivers already add the freestanding, no-red-zone, SSE2 and sysroot
# flags, so only optimisation and language policy belong here.
set(CMAKE_C_FLAGS_INIT "-O2")
set(CMAKE_CXX_FLAGS_INIT "-O2 -fno-exceptions -fno-rtti")
set(CMAKE_EXE_LINKER_FLAGS_INIT "-static")

# No implicit C/C++ runtime: the sysroot libs are linked by the driver.
set(CMAKE_C_STANDARD_LIBRARIES "" CACHE STRING "" FORCE)
set(CMAKE_CXX_STANDARD_LIBRARIES "" CACHE STRING "" FORCE)
