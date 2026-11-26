#!/bin/sh
# Verify CMake's target-side FindThreads result for the OCTAGRAM sysroot.
# CMake cross probes use STATIC_LIBRARY and cannot reliably discover a
# separate pthread library; OCTAGRAM exposes its pthread ABI through libc.a.
set -eu

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)
toolchain=$repo_root/user/toolchain/octagram-toolchain.cmake
sysroot=${OCTAGRAM_SYSROOT:-$repo_root/user/build/sysroot-octagram}

case ${1:-} in
    -h|--help)
        sed -n '2,6p' "$0"
        exit 0
        ;;
    '')
        ;;
    *)
        echo "check_octagram_cmake_threads: unknown argument '$1'" >&2
        exit 2
        ;;
esac

if [ ! -f "$sysroot/usr/lib/libc.a" ]; then
    echo "check_octagram_cmake_threads: missing OCTAGRAM libc at $sysroot" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
mkdir -p "$work/src"
cat > "$work/src/CMakeLists.txt" <<'CMAKE'
cmake_minimum_required(VERSION 3.20)
project(OctagramThreadsProbe C)
find_package(Threads REQUIRED)

if(NOT CMAKE_HAVE_LIBC_PTHREAD)
  message(FATAL_ERROR "OCTAGRAM pthread-in-libc result was not selected")
endif()
if(NOT CMAKE_THREAD_LIBS_INIT STREQUAL "")
  message(FATAL_ERROR "unexpected OCTAGRAM thread link flags: ${CMAKE_THREAD_LIBS_INIT}")
endif()
if(NOT CMAKE_USE_PTHREADS_INIT)
  message(FATAL_ERROR "CMake did not mark the target as pthread-compatible")
endif()
if(NOT TARGET Threads::Threads)
  message(FATAL_ERROR "FindThreads did not create Threads::Threads")
endif()
get_target_property(thread_libraries Threads::Threads INTERFACE_LINK_LIBRARIES)
get_target_property(thread_options Threads::Threads INTERFACE_LINK_OPTIONS)
foreach(value IN ITEMS "${thread_libraries}" "${thread_options}")
  if("${value}" MATCHES "pthread|pthreads")
    message(FATAL_ERROR "Threads::Threads adds an invalid external dependency: ${value}")
  endif()
endforeach()
message(STATUS "OCTAGRAM-CMAKE-THREADS-PASS: pthread ABI is provided by libc.a")
CMAKE

cmake -S "$work/src" -B "$work/build" \
    -DCMAKE_TOOLCHAIN_FILE="$toolchain" \
    -DOCTAGRAM_SYSROOT="$sysroot"
echo "OCTAGRAM-CMAKE-THREADS-PASS"
