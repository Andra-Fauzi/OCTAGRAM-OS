#!/bin/sh
# build_octagram_toolchain.sh: one entry point for the native Rust toolchain.
#
# Order matters and each step is a separate, inspectable script:
#
#   1. make -C user sysroot                  target headers, libc, libc++, crt0
#   2. tools/build_llvm_octagram.sh          static libLLVM for the target (M4.3.3)
#   3. tools/bootstrap_rustc_octagram.sh     rustc/cargo for the target (M4.3.4)
#
# The result is build/toolchain/octagram-toolchain/{bin,lib}, which
# tools/install-disk.sh --toolchain ... copies into /toolchain in the guest.
#
# Usage:
#   tools/build_octagram_toolchain.sh [options]
#     --jobs N             parallelism for LLVM and bootstrap
#     --llvm-version VER   LLVM version to build (default 22.1.8)
#     --llvm-prefix DIR    reuse an existing libLLVM install, skip step 2
#     --libc-checkout DIR  rust-lang/libc 0.2.191 checkout for the target module
#     --prepare-only       stop after writing configs (no x.py build)
#     --skip-sysroot       reuse user/build/sysroot-octagram
#
# Host requirements: build-essential, cmake, ninja-build, curl, python3, git,
# and a Rust nightly stage0 toolchain with rust-src.
#
# Status: orchestration only. .github/workflows/octagram-native-toolchain.yml
# runs the same three steps as separate jobs, but a full end-to-end pass has
# not been recorded yet; see docs/rust-llvm-octagram-build.md and
# docs/rustc-native-bootstrap-runbook.md for the gates each step must pass.

set -eu

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)

jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
llvm_version=${LLVM_VERSION:-22.1.8}
llvm_prefix=${LLVM_PREFIX:-}
libc_checkout=${LIBC_CHECKOUT:-}
prepare_only=0
skip_sysroot=0

while [ $# -gt 0 ]; do
    case $1 in
        --jobs) jobs=$2; shift 2 ;;
        --llvm-version) llvm_version=$2; shift 2 ;;
        --llvm-prefix) llvm_prefix=$2; shift 2 ;;
        --libc-checkout) libc_checkout=$2; shift 2 ;;
        --prepare-only) prepare_only=1; shift ;;
        --skip-sysroot) skip_sysroot=1; shift ;;
        -h|--help) sed -n '2,29p' "$0"; exit 0 ;;
        *) echo "build_octagram_toolchain: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

step() {
    echo
    echo "=== $* ==="
}

if [ "$skip_sysroot" -eq 0 ]; then
    step "1/3 sysroot (make -C user sysroot)"
    make -C "$repo_root/user" sysroot -j "$jobs"
else
    step "1/3 sysroot (skipped)"
    if [ ! -f "$repo_root/user/build/sysroot-octagram/sysroot.json" ]; then
        echo "build_octagram_toolchain: error: no sysroot to reuse" >&2
        exit 1
    fi
fi

llvm_config=
if [ -n "$llvm_prefix" ]; then
    step "2/3 libLLVM (reusing $llvm_prefix)"
    llvm_config=$llvm_prefix/bin/llvm-config
else
    step "2/3 libLLVM (tools/build_llvm_octagram.sh)"
    "$tools_dir/build_llvm_octagram.sh" --llvm-version "$llvm_version" --jobs "$jobs"
    llvm_prefix=$repo_root/build/toolchain/llvm-$llvm_version-octagram
    llvm_config=$llvm_prefix/bin/llvm-config
fi
if [ ! -x "$llvm_config" ]; then
    echo "build_octagram_toolchain: error: no llvm-config at $llvm_config" >&2
    exit 1
fi

step "3/3 rustc and cargo (tools/bootstrap_rustc_octagram.sh)"
bootstrap_arguments="--llvm-prefix $llvm_prefix --jobs $jobs"
if [ -n "$libc_checkout" ]; then
    bootstrap_arguments="$bootstrap_arguments --libc-checkout $libc_checkout"
fi
if [ "$prepare_only" -eq 0 ]; then
    bootstrap_arguments="$bootstrap_arguments --build"
fi
# shellcheck disable=SC2086 # the argument string is intentionally word-split.
"$tools_dir/bootstrap_rustc_octagram.sh" $bootstrap_arguments

echo
echo "Toolchain directory: $repo_root/build/toolchain/octagram-toolchain"
echo "Install it into a bootable root with:"
echo "  sudo tools/install-disk.sh --toolchain build/toolchain/octagram-toolchain /dev/sdX"
echo "Then, from the OCTAGRAM shell:"
echo "  rustc --version"
echo "  cd /cargo-demo && cargo run --offline"
