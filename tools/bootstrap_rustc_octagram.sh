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
#     --time-budget MIN     stop x.py after MIN minutes and keep the build
#                           directory; exit 75 so CI can checkpoint and resume.
#                           The stop is SIGINT, escalating to SIGTERM/SIGKILL
#                           when x.py ignores it, so the state file is always
#                           written
#     --resume              continue the checkout/build directory from an
#                           earlier run; fails when there is nothing to resume
#     --status              print octagram-build-state.json and exit
#     --clean               remove the bootstrap build directory first
#
# Status: scaffolding. Not executed in this workspace: it needs a Rust nightly
# host toolchain, network access to fetch the checkouts, and the prebuilt
# libLLVM. Nothing here has been verified against a real octagram rustc.

set -eu

start_epoch=$(date +%s)

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)

RUST_PIN=342d74463af7dd3018ef6aa04dd5500c2deb5bd1
LIBC_PIN=ccc2ff4aaab962f7fde7b8edda7379c045b1ca12
LIBC_VERSION=0.2.191
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
resume=0
status_only=0
time_budget=${OCTAGRAM_RUSTC_TIME_BUDGET:-0}
# How long the build gets to react to each signal before the next one. A budget
# stop must reach the state file: a job still running when GitHub kills it at
# six hours never runs the checkpoint save step.
stop_grace_int=${OCTAGRAM_STOP_GRACE_INT:-120}
stop_grace_term=${OCTAGRAM_STOP_GRACE_TERM:-60}
stop_grace_kill=15
stop_grace_children=30
stop_signal=

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
        --time-budget) time_budget=$2; shift 2 ;;
        --resume) resume=1; shift ;;
        --status) status_only=1; shift ;;
        -h|--help)
            sed -n '2,/^$/p' "$0"
            exit 0
            ;;
        *)
            echo "bootstrap_rustc_octagram: unknown argument '$1'" >&2
            exit 2
            ;;
    esac
done

case $time_budget in
    ''|*[!0-9]*)
        echo "bootstrap_rustc_octagram: error: --time-budget takes whole minutes, got '$time_budget'" >&2
        exit 2
        ;;
esac
for grace in "$stop_grace_int" "$stop_grace_term"; do
    case $grace in
        ''|*[!0-9]*)
            echo "bootstrap_rustc_octagram: error: OCTAGRAM_STOP_GRACE_INT and" >&2
            echo "bootstrap_rustc_octagram:        OCTAGRAM_STOP_GRACE_TERM take whole seconds, got '$grace'" >&2
            exit 2
            ;;
    esac
done
if [ "$resume" -eq 1 ] && [ "$clean" -eq 1 ]; then
    echo "bootstrap_rustc_octagram: error: --resume and --clean contradict each other" >&2
    exit 2
fi

# The state file lives inside the build directory, so a CI checkpoint restores
# it together with the tree it describes.
state_file=$rust_checkout/build/octagram-build-state.json

if [ "$status_only" -eq 1 ]; then
    if [ ! -f "$state_file" ]; then
        echo "bootstrap_rustc_octagram: no build state at $state_file" >&2
        exit 1
    fi
    cat "$state_file"
    exit 0
fi

write_state() {
    phase=$1
    mkdir -p "$(dirname "$state_file")"
    cat > "$state_file" <<EOF
{
  "phase": "$phase",
  "rust_pin": "$RUST_PIN",
  "host": "$HOST",
  "target": "$TARGET",
  "jobs": $jobs,
  "time_budget_minutes": $time_budget,
  "stop_signal": "$stop_signal",
  "checkout": "$rust_checkout",
  "stage_dir": "$stage_dir",
  "updated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
}

# The checkpoint travels through the 10 GB repository cache quota, so leave
# behind what the next run rebuilds in a couple of minutes: build/<host>/stage0
# and stage0-sysroot are copies of the pinned rustup toolchain (the sysroot is
# linked from it on every run). Bootstrap's stamps are content hashes and cargo
# fingerprints track source and rlib mtimes, not the stage0 binaries, so the
# stage1/stage2 progress in the same tree stays valid and only stage0 is copied
# again.
prune_checkpoint() {
    for dir in stage0 stage0-sysroot; do
        path=$rust_checkout/build/$HOST/$dir
        if [ -d "$path" ]; then
            echo "dropping build/$HOST/$dir from the checkpoint (reinstalled from the pinned toolchain)"
            rm -rf "$path"
        fi
    done
}

# A budget stop must always end with the state file written and the tree quiet:
# GitHub kills the job at six hours without running the checkpoint save step, so
# a build that ignores the stop signal loses every hour spent on it. The driver
# gets SIGINT first (x.py terminates the cargo/rustc children it started), then
# SIGTERM, then SIGKILL. Its children are signalled too, because killing only
# the driver leaves compilers writing into the tree that is about to be
# checkpointed.
build_tree_pids() {
    ps -e -o pid= -o ppid= 2>/dev/null | awk -v root="$1" '
        { kids[$2] = kids[$2] " " $1 }
        function walk(pid,    i, n, list) {
            n = split(kids[pid], list, " ")
            for (i = 1; i <= n; i++) {
                walk(list[i])
                print list[i]
            }
        }
        END { walk(root) }'
}

kill_build_tree() {
    signal=$1
    root=$2
    for pid in $(build_tree_pids "$root"); do
        kill -"$signal" "$pid" 2>/dev/null || true
    done
    kill -"$signal" "$root" 2>/dev/null || true
}

# The pids were recorded before the signal because a killed driver reparents its
# children at once. Waiting for them keeps the checkpoint from archiving a file
# that is still being written; anything left after the grace is an orphan that
# ignored the stop, so freeze it before the tree is saved.
wait_for_stopped_children() {
    pids_file=$1
    [ -f "$pids_file" ] || return 0
    waited=0
    while [ "$waited" -lt "$stop_grace_children" ]; do
        alive=0
        while read -r pid; do
            if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
                alive=1
                break
            fi
        done < "$pids_file"
        [ "$alive" -eq 0 ] && return 0
        sleep 1
        waited=$((waited + 1))
    done
    echo "warning: build children ignored the stop for ${stop_grace_children}s; killing them" >&2
    while read -r pid; do
        if [ -n "$pid" ]; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    done < "$pids_file"
    sleep 1
    return 0
}

# GitHub cancels a hosted job at six hours and never runs the later steps, so a
# bootstrap that cannot finish in one run stops on its own terms and leaves a
# build directory the next run can continue from.
remaining_budget_seconds() {
    if [ "$time_budget" -le 0 ]; then
        echo 0
        return 0
    fi
    elapsed=$(( $(date +%s) - start_epoch ))
    remaining=$(( time_budget * 60 - elapsed ))
    if [ "$remaining" -gt 0 ]; then
        echo "$remaining"
    else
        echo 0
    fi
}

# Resolve paths against the repository root, never against the process cwd.
# Rust bootstrap changes cwd to the Rust checkout before invoking llvm-config;
# a relative bindir would therefore be resolved under rust/build/ and fail.
absolute_path() {
    input_path=$1
    case $input_path in
        /*) ;;
        *) input_path=$repo_root/$input_path ;;
    esac
    parent_dir=$(CDPATH= cd -- "$(dirname -- "$input_path")" 2>/dev/null && pwd -P) || {
        echo "bootstrap_rustc_octagram: error: cannot resolve path $1" >&2
        return 1
    }
    printf '%s/%s\n' "$parent_dir" "$(basename -- "$input_path")"
}

if [ -z "$llvm_prefix" ]; then
    llvm_prefix=$(find "$build_root" -maxdepth 1 -type d -name 'llvm-*-octagram' 2>/dev/null | sort | tail -n 1)
fi
if [ -n "$llvm_prefix" ]; then
    case $llvm_prefix in
        /*) ;;
        *) llvm_prefix=$repo_root/$llvm_prefix ;;
    esac
    if [ ! -d "$llvm_prefix" ]; then
        echo "bootstrap_rustc_octagram: error: LLVM prefix does not exist: $llvm_prefix" >&2
        exit 1
    fi
    llvm_prefix=$(CDPATH= cd -- "$llvm_prefix" && pwd -P)
fi
llvm_config=
if [ -n "$llvm_prefix" ]; then
    llvm_config=$llvm_prefix/bin/llvm-config
fi
# A present but non-executable llvm-config is not a missing install: artifact
# transports that drop mode bits deliver exactly that, and the generic message
# below would send the reader back to a build that already succeeded.
if [ -n "$llvm_config" ] && [ -e "$llvm_config" ] && [ ! -x "$llvm_config" ]; then
    echo "bootstrap_rustc_octagram: error: $llvm_config exists but is not executable" >&2
    echo "bootstrap_rustc_octagram:        (mode $(ls -ld "$llvm_config" | cut -d' ' -f1))" >&2
    echo "bootstrap_rustc_octagram: the install lost its mode bits in transport; restore" >&2
    echo "bootstrap_rustc_octagram: them (chmod +x $llvm_prefix/bin/*) or ship the tree as" >&2
    echo "bootstrap_rustc_octagram: an archive instead of a directory artifact." >&2
    exit 1
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
host_llvm_real_config=$(absolute_path "$host_llvm_config")
host_llvm_version=$("$host_llvm_real_config" --version 2>/dev/null || true)
if [ -z "$host_llvm_version" ]; then
    echo "bootstrap_rustc_octagram: error: $host_llvm_real_config did not report a version" >&2
    exit 1
fi

# Keep the build-platform LLVM config separate from the OCTAGRAM target config.
# The pinned Rust rustc_llvm build script mishandles absolute .so filenames in
# llvm-config --system-libs (it turns /.../libz3.so into -llibz3.so). The
# adapter normalizes only host --system-libs output; it does not touch target
# LLVM or its static link configuration.
host_llvm_config=$repo_root/tools/host_llvm_config_wrapper.py
if [ ! -f "$host_llvm_config" ]; then
    echo "bootstrap_rustc_octagram: error: missing host llvm-config adapter $host_llvm_config" >&2
    exit 1
fi
chmod +x "$host_llvm_config"
python3 "$host_llvm_config" --self-test
export OCTAGRAM_REAL_LLVM_CONFIG=$host_llvm_real_config
host_wrapper_version=$("$host_llvm_config" --version 2>/dev/null || true)
if [ "$host_wrapper_version" != "$host_llvm_version" ]; then
    echo "bootstrap_rustc_octagram: error: host llvm-config adapter reports '$host_wrapper_version'," >&2
    echo "bootstrap_rustc_octagram:        expected '$host_llvm_version' from $host_llvm_real_config" >&2
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
else
    target_llvm_config=$(absolute_path "$target_llvm_config")
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

# Validate the bindir returned by the wrapper before x.py starts. LLVM's
# bootstrap Assemble step creates links to tools such as llvm-cov; a relative
# bindir is otherwise interpreted from the Rust checkout/build directory.
target_llvm_bindir=$("$target_llvm_config" --bindir 2>/dev/null || true)
if [ -z "$target_llvm_bindir" ]; then
    echo "bootstrap_rustc_octagram: error: $target_llvm_config returned an empty --bindir" >&2
    exit 1
fi
case $target_llvm_bindir in
    /*) ;;
    *)
        if [ -d "$repo_root/$target_llvm_bindir" ]; then
            target_llvm_bindir=$repo_root/$target_llvm_bindir
        elif [ -d "$llvm_prefix/$target_llvm_bindir" ]; then
            target_llvm_bindir=$llvm_prefix/$target_llvm_bindir
        else
            echo "bootstrap_rustc_octagram: error: relative LLVM --bindir cannot be resolved: $target_llvm_bindir" >&2
            echo "bootstrap_rustc_octagram:        repository root: $repo_root" >&2
            echo "bootstrap_rustc_octagram:        LLVM prefix: $llvm_prefix" >&2
            exit 1
        fi
        ;;
esac
if [ ! -d "$target_llvm_bindir" ]; then
    echo "bootstrap_rustc_octagram: error: LLVM --bindir does not exist: $target_llvm_bindir" >&2
    exit 1
fi
target_llvm_bindir=$(CDPATH= cd -- "$target_llvm_bindir" && pwd -P)
if [ ! -x "$target_llvm_bindir/llvm-cov" ]; then
    echo "bootstrap_rustc_octagram: error: LLVM tool llvm-cov is missing or not executable in $target_llvm_bindir" >&2
    echo "bootstrap_rustc_octagram:        the Rust bootstrap Assemble step requires this tool." >&2
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

if [ "$resume" -eq 1 ] && [ ! -d "$rust_checkout/.git" ]; then
    echo "bootstrap_rustc_octagram: error: --resume but no checkout at $rust_checkout" >&2
    echo "bootstrap_rustc_octagram: rerun without --resume to clone and prepare one" >&2
    exit 1
fi

# --clean promises a from-scratch bootstrap, which includes the incremental
# build directory; config.toml is rewritten further down as before.
if [ "$clean" -eq 1 ]; then
    rm -rf "$rust_checkout/build"
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

if [ -z "$libc_checkout" ]; then
    echo "bootstrap_rustc_octagram: error: --libc-checkout is required for a native OCTAGRAM std build" >&2
    exit 1
fi
case $libc_checkout in
    /*) ;;
    *) libc_checkout=$repo_root/$libc_checkout ;;
esac
if [ ! -d "$libc_checkout" ]; then
    echo "bootstrap_rustc_octagram: error: pinned libc checkout does not exist: $libc_checkout" >&2
    exit 1
fi
libc_checkout=$(CDPATH= cd -- "$libc_checkout" && pwd -P)
echo "applying pinned libc OCTAGRAM module to $libc_checkout"
python3 "$repo_root/tools/apply_rust_libc_octagram.py" "$libc_checkout"

# `x.py build-std` does not use the separate upstream-libc checkout above:
# Cargo resolves std's libc dependency from its crates.io registry cache.
# Fetch the exact lockfile version now and patch that extracted source before
# the first stage1 std compilation, otherwise libc exports `unistd` without
# an OCTAGRAM module and fails with E0432.
echo "fetching libc $LIBC_VERSION into Cargo registry cache"
libc_fetch_dir=$rust_checkout/build/octagram-libc-fetch
mkdir -p "$libc_fetch_dir/src"
cat > "$libc_fetch_dir/Cargo.toml" <<EOF
[package]
name = "octagram-libc-fetch"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
libc = { version = "=$LIBC_VERSION", default-features = false }
EOF
: > "$libc_fetch_dir/src/lib.rs"
(
    cd "$libc_fetch_dir"
    cargo fetch
)
cargo_home=$(printenv CARGO_HOME 2>/dev/null || true)
if [ -z "$cargo_home" ]; then cargo_home=$HOME/.cargo; fi
for archive in "$cargo_home"/registry/cache/*/libc-"$LIBC_VERSION".crate; do
    [ -f "$archive" ] || continue
    registry_id=$(basename "$(dirname "$archive")")
    registry_source_root=$cargo_home/registry/src/$registry_id
    registry_source=$registry_source_root/libc-$LIBC_VERSION
    if [ ! -f "$registry_source/Cargo.toml" ]; then
        mkdir -p "$registry_source_root"
        tar -xzf "$archive" -C "$registry_source_root"
    fi
done
registry_patch_count=0
for registry_source in "$cargo_home"/registry/src/*/libc-"$LIBC_VERSION"; do
    [ -f "$registry_source/Cargo.toml" ] || continue
    if ! grep -Eq "^version = \"$LIBC_VERSION\"$" "$registry_source/Cargo.toml"; then
        echo "bootstrap_rustc_octagram: error: unexpected libc package version in $registry_source" >&2
        exit 1
    fi
    echo "patching Cargo registry source: $registry_source"
    python3 "$repo_root/tools/apply_rust_libc_octagram.py" "$registry_source" --allow-unpinned
    registry_patch_count=$((registry_patch_count + 1))
done
if [ "$registry_patch_count" -eq 0 ]; then
    echo "bootstrap_rustc_octagram: error: cargo fetch did not expose libc $LIBC_VERSION in $cargo_home/registry/src" >&2
    echo "bootstrap_rustc_octagram:        registry cache must contain the exact source used by -Z build-std." >&2
    exit 1
fi
echo "RUST-LIBC-CARGO-REGISTRY-PATCH-PASS: patched $registry_patch_count registry source(s)"

# 3. Host triple must exist inside rustc itself ----------------------------
echo "registering $HOST as a builtin rustc target"
python3 "$repo_root/tools/apply_rust_octagram_host_target.py" "$rust_checkout"
host_target_source=$rust_checkout/compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs
if [ ! -f "$host_target_source" ]; then
    echo "bootstrap_rustc_octagram: error: $host_target_source was not installed" >&2
    exit 1
fi
# x.py build-std compiles library/std from this pinned Rust checkout. The
# previous rust-src helper was only used against the host nightly component,
# leaving this checkout without OCTAGRAM os::unix::platform::{fs, raw} and
# the Unix thread set_name implementation. Patch the exact sources x.py uses.
echo "patching pinned Rust library/std PAL for OCTAGRAM"
OCTAGRAM_RUST_STD_ROOT="$rust_checkout/library/std/src" \
    python3 "$repo_root/tools/patch_rust_std_octagram.py"

# 4. bootstrap config.toml -------------------------------------------------
config=$rust_checkout/config.toml
echo "writing $config (refreshing absolute toolchain paths)"
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
# Do not require src/llvm-project/compiler-rt. This keeps the Rust bootstrap
# independent from a second LLVM source checkout; Rust's compiler-builtins
# fallback is functionally equivalent, with only some intrinsics less optimized.
optimized-compiler-builtins = false

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

echo
echo "bootstrap prepared:"
echo "  rust checkout: $rust_checkout"
echo "  libLLVM:       $llvm_config"
echo "  host llvm:     $host_llvm_config -> $host_llvm_real_config ($host_llvm_version)"
echo "  target llvm:   $target_llvm_config ($target_llvm_reported)"
echo "  target LLVM bin: $target_llvm_bindir"
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

budget_seconds=$(remaining_budget_seconds)
if [ "$time_budget" -gt 0 ] && [ "$budget_seconds" -eq 0 ]; then
    prune_checkpoint
    write_state incomplete
    echo "OCTAGRAM-RUSTC-BUILD-INCOMPLETE: the ${time_budget}m budget was spent before x.py" >&2
    echo "bootstrap_rustc_octagram: checkout kept at $rust_checkout; rerun to continue" >&2
    exit 75
fi

if [ "$time_budget" -gt 0 ]; then
    echo "running x.py with ${budget_seconds}s remaining of the ${time_budget}m budget"
fi

# Cargo interprets RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER as executable
# prefixes. If either leaks from the runner/bootstrap environment and points
# at the stage compiler itself, Cargo invokes:
#   rustc rustc <normal arguments>
# The second rustc path is then treated as an input and --print=file-names
# probing becomes invalid. Rust bootstrap already selects the compiler for
# each stage, so external wrapper variables must not be inherited here.
if [ -n "${RUSTC_WRAPPER:-}" ]; then
    echo "bootstrap: ignoring inherited RUSTC_WRAPPER=$RUSTC_WRAPPER"
fi
if [ -n "${RUSTC_WORKSPACE_WRAPPER:-}" ]; then
    echo "bootstrap: ignoring inherited RUSTC_WORKSPACE_WRAPPER=$RUSTC_WORKSPACE_WRAPPER"
fi
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
budget_marker=$rust_checkout/build/budget-exhausted
stop_signal_file=$rust_checkout/build/stop-signal
stop_pids_file=$rust_checkout/build/stop-pids
mkdir -p "$rust_checkout/build"
rm -f "$budget_marker" "$stop_signal_file" "$stop_pids_file"

# x.py installs its own SIGINT handler and terminates the cargo/rustc children
# it started, so the interrupted build leaves a usable incremental directory.
( cd "$rust_checkout" && exec ./x.py build --stage 2 --host "$HOST" \
    compiler/rustc src/tools/cargo -j "$jobs" ) &
build_pid=$!
watcher=
if [ "$budget_seconds" -gt 0 ]; then
    (
        sleep "$budget_seconds"
        : > "$budget_marker"
        # Escalate until the driver is gone: x.py can be stuck waiting on a
        # cargo job that ignores SIGINT, and the job would then be killed at six
        # hours with nothing checkpointed.
        for signal in INT TERM KILL; do
            printf '%s\n' "$signal" > "$stop_signal_file"
            build_tree_pids "$build_pid" > "$stop_pids_file"
            kill_build_tree "$signal" "$build_pid"
            grace=$stop_grace_int
            case $signal in
                TERM) grace=$stop_grace_term ;;
                KILL) grace=$stop_grace_kill ;;
            esac
            waited=0
            while [ "$waited" -lt "$grace" ] && kill -0 "$build_pid" 2>/dev/null; do
                sleep 1
                waited=$((waited + 1))
            done
            kill -0 "$build_pid" 2>/dev/null || exit 0
        done
    ) &
    watcher=$!
fi
build_status=0
wait "$build_pid" || build_status=$?
if [ -n "$watcher" ]; then
    kill "$watcher" 2>/dev/null || true
    wait "$watcher" 2>/dev/null || true
fi
if [ -f "$stop_signal_file" ]; then
    read -r stop_signal < "$stop_signal_file" || stop_signal=
fi
wait_for_stopped_children "$stop_pids_file"

stage_root=$rust_checkout/build/$HOST/stage2
if [ -f "$budget_marker" ]; then
    if [ "$build_status" -eq 0 ] && [ -x "$stage_root/bin/rustc" ]; then
        echo "the budget expired as the build finished; continuing with staging"
    else
        case $stop_signal in
            ''|INT) ;;
            *) echo "bootstrap_rustc_octagram: warning: x.py ignored SIGINT; stopped with SIG$stop_signal" >&2 ;;
        esac
        prune_checkpoint
        write_state incomplete
        echo "OCTAGRAM-RUSTC-BUILD-INCOMPLETE: stopped at the ${time_budget}m time budget" >&2
        echo "bootstrap_rustc_octagram: x.py was interrupted; the build directory is kept at" >&2
        echo "bootstrap_rustc_octagram:   $rust_checkout/build" >&2
        echo "bootstrap_rustc_octagram: rerun the same command to continue from here." >&2
        exit 75
    fi
fi
if [ "$build_status" -ne 0 ]; then
    write_state failed
    echo "bootstrap_rustc_octagram: error: x.py failed with status $build_status" >&2
    exit 1
fi
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

write_state complete

echo
ls -l "$stage_dir/bin"
echo
echo "These are OCTAGRAM ELFs and cannot run on this host. Copy the tree into"
echo "the guest's /toolchain (see docs/rustc-native-bootstrap-runbook.md) and run"
echo "  rustc --version"
echo "  cargo build --offline"
echo "from the OCTAGRAM shell."
