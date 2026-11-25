#!/bin/sh
# bootstrap_rustc_octagram.sh: prepare and run the rustc host bootstrap for
# x86_64-unknown-octagram (stage M4.3.4 of docs/rust-m4-3-backend-plan.md).
#
# What this script does, in order:
#   1. verifies (or clones) the pinned Rust checkout;
#   2. applies the OCTAGRAM patches: patches/rust/0001, the pinned libc target
#      module, and (with --libc-checkout) tools/apply_rust_libc_octagram.py;
#   3. refuses to continue unless rustc's own target list knows the host triple
#      (see "Known prerequisite" below);
#   4. writes a bootstrap config.toml using the static libLLVM from
#      tools/build_llvm_octagram.sh and the OCTAGRAM linker driver;
#   5. optionally runs `x.py build` and stages rustc/cargo into a toolchain
#      directory.
#
# Known prerequisite (not yet in the repo): bootstrap needs the host triple to
# exist inside rustc itself, not only as a JSON target spec. That means a
# compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs (base
# x86_64_unknown_none plus the Unix family, linker and static crt fields from
# user/rust/targets/x86_64-unknown-octagram.json) and its registration in
# compiler/rustc_target/src/spec/mod.rs. This script checks for that file and
# stops with this message instead of producing a confusing bootstrap error.
#
# Usage:
#   tools/bootstrap_rustc_octagram.sh [options]
#     --rust-checkout DIR   existing rust-lang/rust checkout
#     --libc-checkout DIR   existing rust-lang/libc 0.2.191 checkout
#     --llvm-prefix DIR     install prefix from build_llvm_octagram.sh
#     --stage-dir DIR       where rustc/cargo are collected after the build
#     --jobs N              bootstrap parallelism
#     --build               also run x.py build (default: prepare only)
#     --clean               remove the bootstrap build directory first
#
# Status: scaffolding. Not executed in this workspace: it needs a Rust nightly
# host toolchain, network access to fetch the checkouts, and the prebuilt
# libLLVM. Nothing here has been verified against a real octagram rustc.

set -eu

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)

RUST_PIN=342d74463af7dd3018ef6aa04dd5500c2deb5bd1
LIBC_PIN=ccc2ff4aaab962f7fde7b8edda7379c045b1ca12
HOST=x86_64-unknown-octagram
TARGET=x86_64-unknown-none

build_root=$repo_root/build/toolchain
rust_checkout=$build_root/rust
libc_checkout=
llvm_prefix=
stage_dir=$build_root/octagram-toolchain
jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
run_build=0
clean=0

while [ $# -gt 0 ]; do
    case $1 in
        --rust-checkout) rust_checkout=$2; shift 2 ;;
        --libc-checkout) libc_checkout=$2; shift 2 ;;
        --llvm-prefix) llvm_prefix=$2; shift 2 ;;
        --stage-dir) stage_dir=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        --build) run_build=1; shift ;;
        --clean) clean=1; shift ;;
        -h|--help)
            sed -n '2,42p' "$0"
            exit 0
            ;;
        *)
            echo "bootstrap_rustc_octagram: unknown argument '$1'" >&2
            exit 2
            ;;
    esac
done

if [ -z "$llvm_prefix" ]; then
    llvm_prefix=$(find "$build_root" -maxdepth 1 -type d -name 'llvm-*-octagram' 2>/dev/null | sort | tail -n 1)
fi
llvm_config=
if [ -n "$llvm_prefix" ]; then
    llvm_config=$llvm_prefix/bin/llvm-config
fi
if [ -z "$llvm_config" ] || [ ! -x "$llvm_config" ]; then
    echo "bootstrap_rustc_octagram: error: no static libLLVM install found" >&2
    echo "bootstrap_rustc_octagram: run tools/build_llvm_octagram.sh first, or pass --llvm-prefix" >&2
    exit 1
fi

sysroot=${OCTAGRAM_SYSROOT:-$repo_root/user/build/sysroot-octagram}
if [ ! -d "$sysroot/usr/include" ]; then
    echo "bootstrap_rustc_octagram: error: no OCTAGRAM sysroot at $sysroot" >&2
    echo "bootstrap_rustc_octagram: run 'make -C user sysroot' first" >&2
    exit 1
fi
linker=$repo_root/user/toolchain/bin/x86_64-octagram-gcc
if [ ! -x "$linker" ]; then
    echo "bootstrap_rustc_octagram: error: missing linker driver $linker" >&2
    exit 1
fi

# 1. Rust checkout ---------------------------------------------------------
if [ ! -d "$rust_checkout/.git" ]; then
    echo "cloning rust-lang/rust into $rust_checkout"
    mkdir -p "$(dirname "$rust_checkout")"
    git clone --filter=blob:none https://github.com/rust-lang/rust "$rust_checkout"
fi
current=$(git -C "$rust_checkout" rev-parse HEAD)
if [ "$current" != "$RUST_PIN" ]; then
    echo "checking out pinned Rust revision $RUST_PIN (current $current)"
    git -C "$rust_checkout" fetch --depth 1 origin "$RUST_PIN"
    git -C "$rust_checkout" checkout --detach "$RUST_PIN"
fi

# 2. OCTAGRAM patches ------------------------------------------------------
patch_file=$repo_root/patches/rust/0001-std-recognize-octagram-target.patch
if git -C "$rust_checkout" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
    echo "patch 0001 already applied"
else
    echo "applying patches/rust/0001-std-recognize-octagram-target.patch"
    git -C "$rust_checkout" apply --check "$patch_file"
    git -C "$rust_checkout" apply "$patch_file"
fi

if [ -n "$libc_checkout" ]; then
    echo "applying pinned libc OCTAGRAM module to $libc_checkout"
    python3 "$repo_root/tools/apply_rust_libc_octagram.py" "$libc_checkout"
else
    echo "note: pass --libc-checkout <rust-lang/libc at $LIBC_PIN> to patch the"
    echo "      libc crate used by std; without it the build stops at the first"
    echo "      missing target_os = \"octagram\" arm."
fi

# 3. Host triple must exist inside rustc itself ----------------------------
host_target_source=$rust_checkout/compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs
if [ ! -f "$host_target_source" ]; then
    cat >&2 <<EOF
bootstrap_rustc_octagram: error: the host triple $HOST is not defined in
rustc itself. Bootstrap cannot cross-compile a compiler for a triple that only
exists as a JSON spec.

Add compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs (base
x86_64_unknown_none + unix family + linker + crt-static fields from
user/rust/targets/x86_64-unknown-octagram.json), register it in
compiler/rustc_target/src/spec/mod.rs, and rerun. See
docs/rustc-native-bootstrap-runbook.md section "Host target definition".
EOF
    exit 1
fi

# 4. bootstrap config.toml -------------------------------------------------
config=$rust_checkout/config.toml
if [ -f "$config" ] && [ "$clean" -eq 0 ]; then
    echo "keeping existing $config (use --clean to rewrite it)"
else
    echo "writing $config"
    cat > "$config" <<EOF
# Generated by tools/bootstrap_rustc_octagram.sh -- regenerate instead of
# editing by hand.

[build]
build = "x86_64-unknown-linux-gnu"
host = ["$HOST"]
target = ["$TARGET"]
build-dir = "$rust_checkout/build"
docs = false
extended = false
submodules = false

[install]
prefix = "$stage_dir"
sysconfdir = "etc"

[rust]
channel = "nightly"
llvm-config = "$llvm_config"
optimize = 2
debug = false
debuginfo-level = 0
backtrace = false
jemalloc = false
profiler = false
lld = false
use-lld = false
codegen-tests = false
tests = false
docs = false
download-rustc = false
remap-debuginfo = false
vendor = false
EOF
    # `rust.llvm-config` selects the library for the build machine; the host
    # entry below selects it for the compiler being produced.
    cat >> "$config" <<EOF

[llvm]
download-ci-llvm = false
ninja = true
targets = "X86"
link-jobs = $jobs
static-libstdcpp = false

[target.$HOST]
linker = "$linker"
llvm-config = "$llvm_config"
crt-static = true
relocation-model = "static"
panic-strategy = "abort"
EOF
    cat >> "$config" <<EOF

[target.$TARGET]
linker = "$linker"
crt-static = true
relocation-model = "static"
panic-strategy = "abort"

[dist]
compression-formats = ["xz"]
EOF
fi

echo
echo "bootstrap prepared:"
echo "  rust checkout: $rust_checkout"
echo "  libLLVM:       $llvm_config"
echo "  sysroot:       $sysroot"
echo "  linker:        $linker"
echo "  stage dir:     $stage_dir"

if [ "$run_build" -eq 0 ]; then
    echo
    echo "Dry run: nothing was compiled. Re-run with --build to execute:"
    echo "  (cd $rust_checkout && ./x.py build --stage 2 --host $HOST compiler/rustc src/tools/cargo -j $jobs)"
    exit 0
fi

# 5. Build and stage -------------------------------------------------------
if ! command -v rustup >/dev/null 2>&1 && ! command -v rustc >/dev/null 2>&1; then
    echo "bootstrap_rustc_octagram: error: no Rust stage0 toolchain on the host" >&2
    echo "bootstrap_rustc_octagram: install the nightly toolchain named in $rust_checkout/rust-toolchain.toml" >&2
    exit 1
fi

cd "$rust_checkout"
./x.py build --stage 2 --host "$HOST" compiler/rustc src/tools/cargo -j "$jobs"

bin_dir=$rust_checkout/build/$HOST/stage2/bin
if [ ! -x "$bin_dir/rustc" ]; then
    echo "bootstrap_rustc_octagram: error: bootstrap produced no $bin_dir/rustc" >&2
    exit 1
fi

mkdir -p "$stage_dir/bin" "$stage_dir/lib"
cp "$bin_dir/rustc" "$stage_dir/bin/rustc"
if [ -x "$bin_dir/cargo" ]; then
    cp "$bin_dir/cargo" "$stage_dir/bin/cargo"
else
    echo "bootstrap_rustc_octagram: warning: no cargo binary was produced" >&2
fi

# lib/rustlib holds core/alloc/std for every target. It must travel with rustc:
# legend: rustc looks for the sysroot relative to its own path.
if [ -d "$rust_checkout/build/$HOST/stage2/lib/rustlib" ]; then
    mkdir -p "$stage_dir/lib/rustlib"
    cp -R "$rust_checkout/build/$HOST/stage2/lib/rustlib/." "$stage_dir/lib/rustlib/"
fi

echo
echo "staged OCTAGRAM toolchain at $stage_dir"
ls -l "$stage_dir/bin"
echo
echo "These are OCTAGRAM ELFs and cannot run on this host. Copy the tree into"
echo "the guest's /toolchain (see docs/rustc-native-bootstrap-runbook.md) and run"
echo "  rustc --version"
echo "  cargo build --offline"
echo "from the OCTAGRAM shell."
