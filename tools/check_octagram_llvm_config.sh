#!/bin/sh
# check_octagram_llvm_config.sh: exercise tools/octagram_llvm_config.py and the
# llvm-config preflight in tools/bootstrap_rustc_octagram.sh without building
# LLVM.
#
# The default build directory is a fixture that mimics the files LLVM generates
# during configure, so this check runs in seconds. Pass --build-dir to run the
# same assertions against a real configured LLVM build tree as well.
#
# Usage:
#   tools/check_octagram_llvm_config.sh [--build-dir DIR] [--bootstrap-only]
#
# --build-dir DIR    LLVM build tree (or fixture) with tools/llvm-config/*.inc
# --bootstrap-only   only check the rustc bootstrap preflight
set -eu

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)

build_dir=$repo_root/tools/toolchain-probes/llvm-config-fixture
bootstrap_only=0
python=${PYTHON:-python3}

while [ $# -gt 0 ]; do
    case $1 in
        --build-dir) build_dir=$2; shift 2 ;;
        --bootstrap-only) bootstrap_only=1; shift ;;
        -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
        *) echo "check_octagram_llvm_config: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

if [ ! -f "$build_dir/tools/llvm-config/LibraryDependencies.inc" ]; then
    echo "check_octagram_llvm_config: error: $build_dir is not a configured LLVM tree" >&2
    echo "check_octagram_llvm_config: expected tools/llvm-config/LibraryDependencies.inc" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# Installed the way the bot does: a prefix plus fake host llvm-config binaries.
prefix=$work/prefix
host_config=$work/host-llvm-config
# bootstrap also wants an OCTAGRAM sysroot; its own job covers that, so this
# check only needs the directory to exist.
stub_sysroot=$work/sysroot
mkdir -p "$stub_sysroot/usr/include"

expected_version=$(sed -n 's/^#define LLVM_VERSION_STRING "\(.*\)"$/\1/p' \
    "$build_dir/include/llvm/Config/llvm-config.h" | head -n 1)
if [ -z "$expected_version" ]; then
    echo "check_octagram_llvm_config: error: no LLVM_VERSION_STRING in $build_dir" >&2
    exit 1
fi
major=$(printf '%s' "$expected_version" | cut -d. -f1)

printf '#!/bin/sh\necho %s\n' "$expected_version" > "$host_config"
chmod +x "$host_config"

if [ "$bootstrap_only" -eq 0 ]; then
    echo "== generator: $build_dir =="
    "$python" "$repo_root/tools/octagram_llvm_config.py" \
        --build-dir "$build_dir" --prefix "$prefix" \
        --output "$prefix/bin/octagram-llvm-config"
    config=$prefix/bin/octagram-llvm-config

    [ -x "$config" ] || fail "$config is not executable"
    [ "$("$config" --version)" = "$expected_version" ] \
        || fail "--version is not $expected_version"
    [ -n "$("$config" --host-target)" ] || fail "--host-target is empty"
    [ -n "$("$config" --components)" ] || fail "--components is empty"
    # bootstrap needs the target binary's name to exist even though it never
    # runs it; the generator only writes the host-runnable replacement.
    printf '#!/bin/sh\necho %s\n' "$expected_version" > "$prefix/bin/llvm-config"
    chmod +x "$prefix/bin/llvm-config"
    grep -q '^#define LLVM_VERSION_MAJOR' "$build_dir/include/llvm/Config/llvm-config.h" \
        && [ "$(sed -n 's/^#define LLVM_VERSION_MAJOR \([0-9]*\)$/\1/p' \
              "$build_dir/include/llvm/Config/llvm-config.h" | head -n 1)" = "$major" ] \
        || fail "fixture major version disagrees with the version string"

    echo "== generator: partial install reports missing archives =="
    if "$config" --libs > "$work/libs.out" 2> "$work/libs.err"; then
        fail "--libs succeeded before any archive existed"
    fi
    grep -q 'missing:' "$work/libs.err" || fail "--libs did not report missing archives"
    if "$config" --link-shared --libs > /dev/null 2>&1; then
        fail "--link-shared succeeded on a static-only install"
    fi
    if "$config" --definitely-unknown > /dev/null 2>&1; then
        fail "an unknown argument was accepted"
    fi

    echo "== generator: archives present =="
    libdir=$("$config" --libdir)
    mkdir -p "$libdir"
    "$python" - "$config" "$libdir" <<'PY'
import json, pathlib, re, sys

config = pathlib.Path(sys.argv[1]).read_text()
libdir = pathlib.Path(sys.argv[2])
data = json.loads(re.search(r"DATA_JSON = r'''(.*?)'''", config, re.S).group(1))
count = 0
for entry in data["components"].values():
    if entry["library"]:
        (libdir / ("lib" + entry["library"] + ".a")).write_bytes(b"!<arch>\n")
        count += 1
print("stubbed %d archives" % count)
PY
    "$config" --libs > "$work/libs.out" 2> "$work/libs.err" \
        || fail "--libs failed with every archive present"
    [ -n "$("$config" --system-libs)" ] || fail "--system-libs is empty"
    [ "$("$config" --shared-mode)" = "static" ] || fail "--shared-mode is not static"
    lib_count=$(wc -w < "$work/libs.out" | tr -d ' ')
    [ "${lib_count:-0}" -gt 0 ] || fail "--libs printed nothing"
    duplicates=$(tr ' ' '\n' < "$work/libs.out" | sed '/^$/d' | sort | uniq -d)
    [ -z "$duplicates" ] || fail "--libs repeats libraries: $duplicates"
    last=$(tr ' ' '\n' < "$work/libs.out" | sed '/^$/d' | tail -n 1)
    [ "$last" = "-lLLVMDemangle" ] || fail "leaf library is not last (got $last)"
    "$config" --libs | tr ' ' '\n' | grep -qx -- '-lLLVMSupport' \
        || fail "--libs is missing -lLLVMSupport"
    [ "$("$config" --libnames Support | cut -d' ' -f1)" = "libLLVMSupport.a" ] \
        || fail "--libnames Support is wrong"
    files=$("$config" --libfiles Support)
    for file in $files; do
        case $file in
            /*/libLLVM*.a) ;;
            *) fail "--libfiles printed a non-absolute path: $file" ;;
        esac
    done
    for lib in $(tr ' ' '\n' < "$work/libs.out" | sed '/^$/d'); do
        [ -f "$libdir/lib${lib#-l}.a" ] || fail "--libs names an archive with no file: $lib"
    done

    echo "== generator: fixture ordering =="
    if [ "$build_dir" = "$repo_root/tools/toolchain-probes/llvm-config-fixture" ]; then
        expected="-lLLVMCore -lLLVMBitReader -lLLVMSupport -lLLVMDemangle"
        actual=$("$config" --libs core support)
        [ "$actual" = "$expected" ] \
            || fail "--libs core support printed '$actual', expected '$expected'"
        expected_all="-lLLVMTableGen -lLLVMAnalysis -lLLVMCore -lLLVMBitReader -lLLVMSupport -lLLVMDemangle"
        actual_all=$("$config" --libs)
        [ "$actual_all" = "$expected_all" ] \
            || fail "--libs printed '$actual_all', expected '$expected_all'"
    fi
fi

echo "== bootstrap preflight =="
bootstrap=$repo_root/tools/bootstrap_rustc_octagram.sh
[ -x "$bootstrap" ] || fail "$bootstrap is not executable"

OCTAGRAM_SYSROOT="$stub_sysroot" "$bootstrap" --llvm-prefix "$prefix" --host-llvm-config "$host_config" --check \
    > "$work/bootstrap.out" 2>&1 \
    || { fail "bootstrap --check rejected a valid install"; cat "$work/bootstrap.out" >&2; }
grep -q 'OCTAGRAM-BOOTSTRAP-PREFLIGHT-PASS' "$work/bootstrap.out" \
    || fail "bootstrap --check did not report success"

echo "-- missing target llvm-config --"
mv "$prefix/bin/octagram-llvm-config" "$prefix/bin/octagram-llvm-config.saved" 2>/dev/null || true
if OCTAGRAM_SYSROOT="$stub_sysroot" "$bootstrap" --llvm-prefix "$prefix" --host-llvm-config "$host_config" --check \
        > "$work/bootstrap.out" 2>&1; then
    fail "bootstrap accepted an install without octagram-llvm-config"
fi
grep -q 'no host-runnable llvm-config for x86_64-unknown-octagram' "$work/bootstrap.out" \
    || fail "bootstrap error message does not name the missing llvm-config"
mv "$prefix/bin/octagram-llvm-config.saved" "$prefix/bin/octagram-llvm-config" 2>/dev/null || true

echo "-- mismatched target version --"
printf '#!/bin/sh\necho 0.0.0-wrong\n' > "$prefix/bin/octagram-llvm-config"
chmod +x "$prefix/bin/octagram-llvm-config"
if OCTAGRAM_SYSROOT="$stub_sysroot" "$bootstrap" --llvm-prefix "$prefix" --host-llvm-config "$host_config" --check \
        > "$work/bootstrap.out" 2>&1; then
    fail "bootstrap accepted a mismatched target llvm-config version"
fi
grep -q 'target llvm-config is' "$work/bootstrap.out" \
    || fail "bootstrap error message does not report the version mismatch"

echo "-- target llvm-config that cannot run --"
printf 'not an executable format\n' > "$prefix/bin/octagram-llvm-config"
chmod +x "$prefix/bin/octagram-llvm-config"
if OCTAGRAM_SYSROOT="$stub_sysroot" "$bootstrap" --llvm-prefix "$prefix" --host-llvm-config "$host_config" --check \
        > "$work/bootstrap.out" 2>&1; then
    fail "bootstrap accepted a target llvm-config that does not run"
fi
grep -q 'did not report a version' "$work/bootstrap.out" \
    || fail "bootstrap error message does not explain the unreadable llvm-config"

echo "-- missing host llvm-config --"
if env -u HOST_LLVM_CONFIG OCTAGRAM_SYSROOT="$stub_sysroot" PATH=/usr/bin:/bin \
        "$bootstrap" --llvm-prefix "$prefix" --check \
        > "$work/bootstrap.out" 2>&1; then
    fail "bootstrap accepted a missing host llvm-config"
fi

if [ "$failures" -ne 0 ]; then
    echo "OCTAGRAM-LLVM-CONFIG-FAIL ($failures)" >&2
    exit 1
fi
echo "OCTAGRAM-LLVM-CONFIG-PASS ($build_dir)"
