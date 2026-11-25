#!/bin/sh
# check_octagram_runtime_probes.sh: link small C++ programs against the OCTAGRAM
# sysroot before the real toolchain build.
#
# Each probe in tools/toolchain-probes/ mirrors a dependency of libLLVM or rustc
# (synchronisation, thread-local destructors, mmap-based file IO, large
# allocation). They are linked, not executed, so this runs in seconds on the
# build host and reports a missing libc/libc++ entry point while the cause is
# still obvious — instead of after an hour of LLVM compilation.
#
# Usage: tools/check_octagram_runtime_probes.sh
#   OCTAGRAM_SYSROOT  sysroot to link against
#                     (default: user/build/sysroot-octagram)

set -eu

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)
sysroot=${OCTAGRAM_SYSROOT:-$repo_root/user/build/sysroot-octagram}
driver=$repo_root/user/toolchain/bin/x86_64-octagram-g++
probe_dir=$tools_dir/toolchain-probes

if [ ! -f "$sysroot/usr/lib/libc++.a" ]; then
    echo "check_octagram_runtime_probes: error: no sysroot at $sysroot" >&2
    echo "check_octagram_runtime_probes: run 'make -C user sysroot' first" >&2
    exit 1
fi
if [ ! -x "$driver" ]; then
    echo "check_octagram_runtime_probes: error: missing driver $driver" >&2
    exit 1
fi

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT
failures=0
total=0

for probe in "$probe_dir"/*.cpp; do
    total=$((total + 1))
    name=$(basename "$probe" .cpp)
    binary=$work_dir/$name.elf
    if "$driver" "$probe" -o "$binary" >"$work_dir/$name.log" 2>&1; then
        magic=$(head -c 4 "$binary" | od -An -tx1 | tr -d ' \n')
        if [ "$magic" = "7f454c46" ]; then
            echo "PASS $name"
        else
            echo "FAIL $name: output is not an ELF executable" >&2
            failures=$((failures + 1))
        fi
    else
        echo "FAIL $name: did not link" >&2
        sed -n '1,6p' "$work_dir/$name.log" | sed 's/^/     /' >&2
        failures=$((failures + 1))
    fi
done

if [ "$total" -eq 0 ]; then
    echo "check_octagram_runtime_probes: error: no probes in $probe_dir" >&2
    exit 1
fi

if [ "$failures" -ne 0 ]; then
    echo "OCTAGRAM-RUNTIME-PROBES-FAIL ($failures of $total probes)"
    exit 1
fi

echo "OCTAGRAM-RUNTIME-PROBES-PASS ($total probes)"
