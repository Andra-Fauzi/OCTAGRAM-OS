#!/bin/sh
# bootstrap_rustc_octagram.sh: prepare and run the rustc host bootstrap for
# x86_64-unknown-octagram (stage M4.3.4 of docs/rust-m4-3-backend-plan.md).
#
# What this script does, in order:
#   1. verifies (or clones) the pinned Rust checkout;
#   2. applies the OCTAGRAM patches: patches/rust/0001, the pinned libc target
#      module, and (with --libc-checkout) tools/apply_rust_libc_octagram.py;
#   3. registers the host triple as a builtin rustc target (target definition,
#      Os::Octagram enum variant, supported_targets! entry);
#   4. writes a bootstrap config.toml using the static libLLVM from
#      tools/build_llvm_octagram.sh and the OCTAGRAM linker driver;
#   5. optionally runs `x.py build` and stages rustc/cargo into a toolchain
#      directory.
#
# Why step 3 exists: bootstrap cannot cross-compile a compiler for a triple
# that exists only as a JSON spec. The host triple must be known to rustc_target
# itself, so tools/apply_rust_octagram_host_target.py adds the target definition,
# the Os enum variant and the supported_targets! registration to the checkout.
# Keep compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs and
# user/rust/targets/x86_64-unknown-octagram.json in sync.
#
# Usage:
#   tools/bootstrap_rustc_octagram.sh [options]
#     --rust-checkout DIR   existing rust-lang/rust checkout
#     --libc-checkout DIR   existing rust-lang/libc 0.2.191 checkout
#     --llvm-prefix DIR     install prefix from build_llvm_octagram.sh
#     --host-llvm-config F  llvm-config for the build machine (see below)
#     --target-llvm-config F  llvm-config describing the OCTAGRAM library set
#     --stage-dir DIR       where rustc/cargo are collected after the build
#     --jobs N              bootstrap parallelism
#     --build               also run x.py build (default: prepare only)
#     --check               validate the inputs, print the pass marker, exit
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
host_llvm_config=
target_llvm_config=
stage_dir=$build_root/octagram-toolchain
jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
run_build=0
check_only=0
clean=0

while [ $# -gt 0 ]; do
    case $1 in
        --rust-checkout) rust_checkout=$2; shift 2 ;;
        --libc-checkout) libc_checkout=$2; shift 2 ;;
        --llvm-prefix) llvm_prefix=$2; shift 2 ;;
        --host-llvm-config) host_llvm_config=$2; shift 2 ;;
        --target-llvm-config) target_llvm_config=$2; shift 2 ;;
        --stage-dir) stage_dir=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        --build) run_build=1; shift ;;
        --check) check_only=1; shift ;;
        --clean) clean=1; shift ;;
        -h|--help)
            sed -n '2,43p' "$0"
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

# llvm-config is executed by bootstrap on the build machine, so the copy that
# answers for x86_64-unknown-linux-gnu must be a host binary. $llvm_config above
# is an OCTAGRAM ELF: it only describes the target library set and cannot run
# here, and bootstrap would stop with "could not execute llvm-config".
if [ -z "$host_llvm_config" ]; then
    host_llvm_config=${HOST_LLVM_CONFIG:-}
fi
if [ -z "$host_llvm_config" ]; then
    for candidate in llvm-config llvm-config-21; do
        if command -v "$candidate" >/dev/null 2>&1; then
            host_llvm_config=$(command -v "$candidate")
            break
        fi
    done
fi
if [ -z "$host_llvm_config" ] || [ ! -x "$host_llvm_config" ]; then
    echo "bootstrap_rustc_octagram: error: no host-runnable llvm-config found" >&2
    echo "bootstrap_rustc_octagram: bootstrap executes llvm-config while building the" >&2
    echo "bootstrap_rustc_octagram: build-platform compiler; $llvm_config is an" >&2
    echo "bootstrap_rustc_octagram: x86_64-unknown-octagram ELF and cannot run on this host." >&2
    echo "bootstrap_rustc_octagram: install LLVM 21 for the build host, or pass" >&2
    echo "bootstrap_rustc_octagram: --host-llvm-config <path> (must report the same version)." >&2
    exit 1
fi
host_llvm_version=$("$host_llvm_config" --version 2>/dev/null || true)
if [ -z "$host_llvm_version" ]; then
    echo "bootstrap_rustc_octagram: error: $host_llvm_config did not report a version" >&2
    exit 1
fi
target_llvm_major=$(sed -n 's/^#define LLVM_VERSION_MAJOR \([0-9]*\)$/\1/p' \
    "$llvm_prefix/include/llvm/Config/llvm-config.h" 2>/dev/null | head -n 1)
host_llvm_major=$(printf '%s' "$host_llvm_version" | cut -d. -f1)
if [ -n "$target_llvm_major" ] && [ "$target_llvm_major" != "$host_llvm_major" ]; then
    echo "bootstrap_rustc_octagram: error: host llvm-config is $host_llvm_version," >&2
    echo "bootstrap_rustc_octagram:        target LLVM is major $target_llvm_major" >&2
    echo "bootstrap_rustc_octagram: rustc refuses a mismatched LLVM; use the same version." >&2
    exit 1
fi

# The OCTAGRAM compiler links the cross-built static libs, so its llvm-config
# must describe that install. build_llvm_octagram.sh writes a host-runnable
# replacement next to the target-only binary for exactly this reason; a raw
# bin/llvm-config cannot be queried here.
if [ -z "$target_llvm_config" ]; then
    target_llvm_config=$llvm_prefix/bin/octagram-llvm-config
fi
if [ ! -x "$target_llvm_config" ]; then
    echo "bootstrap_rustc_octagram: error: no host-runnable llvm-config for $HOST" >&2
    echo "bootstrap_rustc_octagram: looked for $target_llvm_config" >&2
    echo "bootstrap_rustc_octagram: regenerate the LLVM install with" >&2
    echo "bootstrap_rustc_octagram:   tools/build_llvm_octagram.sh (it writes" >&2
    echo "bootstrap_rustc_octagram:   bin/octagram-llvm-config), or pass --target-llvm-config." >&2
    exit 1
fi
target_llvm_reported=$("$target_llvm_config" --version 2>/dev/null || true)
if [ -z "$target_llvm_reported" ]; then
    echo "bootstrap_rustc_octagram: error: $target_llvm_config did not report a version" >&2
    echo "bootstrap_rustc_octagram: it must run on the build machine, not inside OCTAGRAM" >&2
    exit 1
fi
if [ "$(printf '%s' "$target_llvm_reported" | cut -d. -f1)" != "$host_llvm_major" ]; then
    echo "bootstrap_rustc_octagram: error: target llvm-config is $target_llvm_reported," >&2
    echo "bootstrap_rustc_octagram:        host llvm-config is $host_llvm_version" >&2
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

# --check stops here: every input has been validated (sysroot, linker, and both
# llvm-config binaries), so CI can test the preflight without a Rust toolchain
# or network access to the rust checkout.
if [ "$check_only" -eq 1 ]; then
    echo "  host llvm:   $host_llvm_config ($host_llvm_version)"
    echo "  target llvm: $target_llvm_config ($target_llvm_reported)"
    echo "OCTAGRAM-BOOTSTRAP-PREFLIGHT-PASS"
    exit 0
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

# 1b. Submodules the build reads from ---------------------------------------
# src/tools/cargo and library/backtrace are submodules, and config.toml keeps
# `submodules = false` so bootstrap never clones src/llvm-project (we link the
# system libLLVM instead). Without these two, `x.py build src/tools/cargo` and
# the std build fail with empty directories.
for submodule in src/tools/cargo library/backtrace; do
    status=$(git -C "$rust_checkout" submodule status -- "$submodule" 2>/dev/null || true)
    case $status in
        -*) echo "initializing submodule $submodule"
            git -C "$rust_checkout" submodule update --init --depth 1 "$submodule" ;;
        '') echo "bootstrap_rustc_octagram: error: $submodule is not a submodule of this checkout" >&2
            exit 1 ;;
        *) echo "submodule $submodule already present" ;;
    esac
done

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
echo "registering $HOST as a builtin rustc target"
python3 "$repo_root/tools/apply_rust_octagram_host_target.py" "$rust_checkout"
host_target_source=$rust_checkout/compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs
if [ ! -f "$host_target_source" ]; then
    echo "bootstrap_rustc_octagram: error: $host_target_source was not installed" >&2
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
optimize = 2
debug = false
debuginfo-level = 0
backtrace = false
jemalloc = false
lld = false
codegen-tests = false
download-rustc = false
remap-debuginfo = false
EOF
    # External LLVM is selected per target triple: the build machine needs it
    # to link stage0 tools, the OCTAGRAM host entry to produce a compiler that
    # links against the same library. `llvm.targets` is omitted because no LLVM
    # is built here.
    cat >> "$config" <<EOF

[llvm]
download-ci-llvm = false
link-jobs = $jobs

[target.x86_64-unknown-linux-gnu]
llvm-config = "$host_llvm_config"

# The OCTAGRAM compiler links the cross-built static libLLVM. bootstrap still
# runs this path on the build machine, so it must be a host-runnable
# llvm-config whose output describes the OCTAGRAM library set (a wrapper around
# the native one is the expected shape); the raw $llvm_config binary cannot be
# executed here.
[target.$HOST]
linker = "$linker"
llvm-config = "$target_llvm_config"
crt-static = true

[target.$TARGET]
linker = "$linker"
crt-static = true
EOF
fi

echo
echo "bootstrap prepared:"
echo "  rust checkout: $rust_checkout"
echo "  libLLVM:       $llvm_config"
echo "  host llvm:     $host_llvm_config ($host_llvm_version)"
echo "  target llvm:   $target_llvm_config ($target_llvm_reported)"
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

stage_root=$rust_checkout/build/$HOST/stage2
if [ ! -x "$stage_root/bin/rustc" ]; then
    echo "bootstrap_rustc_octagram: error: bootstrap produced no $stage_root/bin/rustc" >&2
    exit 1
fi

# Staging validates the result: ELF binaries, a host std rlib, and a manifest
# the installer/packaging steps can audit. It refuses a host toolchain or a
# compiler without its sysroot instead of letting it reach an image.
llvm_version=$("$llvm_config" --version 2>/dev/null || echo unknown)
"$repo_root/tools/stage_octagram_toolchain.sh" \
    --source "$stage_root" \
    --output "$stage_dir" \
    --host "$HOST" \
    --target "$TARGET" \
    --rust-commit "$RUST_PIN" \
    --llvm-version "$llvm_version"

echo
ls -l "$stage_dir/bin"
echo
echo "These are OCTAGRAM ELFs and cannot run on this host. Copy the tree into"
echo "the guest's /toolchain (see docs/rustc-native-bootstrap-runbook.md) and run"
echo "  rustc --version"
echo "  cargo build --offline"
echo "from the OCTAGRAM shell."
