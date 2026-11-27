#!/bin/sh
# build_llvm_octagram.sh: cross-build a static libLLVM for the OCTAGRAM
# userspace ABI (x86_64, static ET_EXEC, OCTAGRAM libc/libc++).
#
# This is stage M4.3.3 of docs/rust-m4-3-backend-plan.md and the prerequisite
# for the rustc host bootstrap in M4.3.4. It does not run anything on OCTAGRAM:
# the result is a host-side static library set plus llvm-config, llc and
# llvm-nm that are themselves OCTAGRAM executables (to be copied into the
# guest's /toolchain/bin once a toolchain image step exists).
#
# Prerequisites (host):
#   gcc g++ cmake ninja-build curl python3
#   make -C user sysroot
#
# Usage:
#   tools/build_llvm_octagram.sh [--llvm-version 22.1.8] [--prefix DIR]
#                                [--source DIR] [--jobs N] [--time-budget MIN]
#                                [--resume] [--status] [--clean]
#                                [--check-install-inputs]
#
#   --time-budget MIN  stop ninja after MIN minutes, keep the build tree and
#                      exit 75 (EX_TEMPFAIL) with OCTAGRAM-LLVM-BUILD-INCOMPLETE.
#                      Rerun the same command to continue: ninja only rebuilds
#                      what is still missing. The stop is SIGINT, escalating to
#                      SIGTERM/SIGKILL when an edge ignores it, so the state
#                      file is always written.
#   --resume           continue the build tree from a previous run. Fails when
#                      there is nothing to resume instead of silently starting
#                      over, so CI can report what it actually did.
#   --status           print octagram-build-state.json and exit (1 if absent).
#   --clean            remove the build tree and install prefix first.
#   --check-install-inputs
#                      compare the stamp inside the install prefix with the
#                      inputs this run would build from; exit 1 and change
#                      nothing when they differ.
#
# A complete prefix is reused only when the stamp it carries matches the current
# inputs. The stamp covers the LLVM version, the CMake flag vector, the
# toolchain file, tools/octagram_llvm_config.py and the sysroot contents, so a
# tree restored from an older cache (another sysroot, other flags) is discarded
# and rebuilt instead of being linked into rustc.
#
# A run is resumable by default: the CMake build tree is kept between runs, a
# complete install prefix is reused without rebuilding, and a partial source
# download or extraction is repaired instead of restarted. The script records
# what it did in <build-dir>/octagram-build-state.json, which is what the
# "incomplete" marker in CI is derived from.
#
# Pinning: rustc refuses to build against an LLVM older than the revision its
# checkout requires, and bootstrap enforces a hard floor of major 22 at the
# pinned checkout (check_llvm_version in src/bootstrap/.../llvm.rs). Before
# running, confirm the version from the Rust source you will bootstrap:
#   grep -m1 LLVM_VERSION_MAJOR build/toolchain/rust/src/llvm-project/llvm/CMakeLists.txt
# The default below must be bumped together with that checkout; there is no
# version sniffing because a mismatch must fail loudly, not silently rebuild.
#
# Status: scaffolding. Not yet executed in this workspace (no cmake/ninja and
# no OCTAGRAM sysroot build here), so treat any result as unverified until the
# guest-side gates in docs/rust-llvm-octagram-build.md pass.

set -eu

start_epoch=$(date +%s)

script=$0
case $script in
    */*) tools_dir=$(cd "$(dirname "$script")" && pwd) ;;
    *) tools_dir=$(cd "$(dirname "$(command -v -- "$script")")" && pwd) ;;
esac
repo_root=$(cd "$tools_dir/.." && pwd)

llvm_version=${LLVM_VERSION:-22.1.8}
jobs=${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}
time_budget=${OCTAGRAM_LLVM_TIME_BUDGET:-0}
# How long the build gets to react to each signal before the next one. A budget
# stop must reach the state file: a job still running when GitHub kills it at
# six hours never runs the checkpoint save step.
stop_grace_int=${OCTAGRAM_STOP_GRACE_INT:-120}
stop_grace_term=${OCTAGRAM_STOP_GRACE_TERM:-60}
stop_grace_kill=15
stop_grace_children=30
stop_signal=
build_root=${OCTAGRAM_TOOLCHAIN_BUILD:-$repo_root/build/toolchain}
sysroot=${OCTAGRAM_SYSROOT:-$repo_root/user/build/sysroot-octagram}
toolchain_file=$repo_root/user/toolchain/octagram-toolchain.cmake
prefix=
source_dir=
clean=0
resume=0
status_only=0
check_install_inputs=0

while [ $# -gt 0 ]; do
    case $1 in
        --llvm-version) llvm_version=$2; shift 2 ;;
        --prefix) prefix=$2; shift 2 ;;
        --source) source_dir=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        --time-budget) time_budget=$2; shift 2 ;;
        --resume) resume=1; shift ;;
        --status) status_only=1; shift ;;
        --clean) clean=1; shift ;;
        --check-install-inputs) check_install_inputs=1; shift ;;
        -h|--help)
            sed -n '2,/^$/p' "$0"
            exit 0
            ;;
        *)
            echo "build_llvm_octagram: unknown argument '$1'" >&2
            exit 2
            ;;
    esac
done

case $time_budget in
    ''|*[!0-9]*)
        echo "build_llvm_octagram: error: --time-budget takes whole minutes, got '$time_budget'" >&2
        exit 2
        ;;
esac
case $jobs in
    ''|*[!0-9]*)
        echo "build_llvm_octagram: error: --jobs takes a number, got '$jobs'" >&2
        exit 2
        ;;
esac
for grace in "$stop_grace_int" "$stop_grace_term"; do
    case $grace in
        ''|*[!0-9]*)
            echo "build_llvm_octagram: error: OCTAGRAM_STOP_GRACE_INT and" >&2
            echo "build_llvm_octagram:        OCTAGRAM_STOP_GRACE_TERM take whole seconds, got '$grace'" >&2
            exit 2
            ;;
    esac
done
if [ "$resume" -eq 1 ] && [ "$clean" -eq 1 ]; then
    echo "build_llvm_octagram: error: --resume and --clean contradict each other" >&2
    exit 2
fi

llvm_short=$(printf '%s' "$llvm_version" | cut -d. -f1)
install_prefix=${prefix:-$build_root/llvm-$llvm_version-octagram}
work_dir=$build_root/llvm-$llvm_version
cmake_build=$work_dir/build-octagram
state_file=$cmake_build/octagram-build-state.json
install_stamp_file=$install_prefix/.octagram-build-inputs

if [ "$status_only" -eq 1 ]; then
    if [ ! -f "$state_file" ]; then
        echo "build_llvm_octagram: no build state at $state_file" >&2
        exit 1
    fi
    cat "$state_file"
    exit 0
fi

write_state() {
    phase=$1
    mkdir -p "$cmake_build"
    cat > "$state_file" <<EOF
{
  "llvm_version": "$llvm_version",
  "phase": "$phase",
  "jobs": $jobs,
  "time_budget_minutes": $time_budget,
  "stop_signal": "$stop_signal",
  "source_dir": "$source_dir",
  "build_dir": "$cmake_build",
  "install_prefix": "$install_prefix",
  "updated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
}

# GitHub cancels a job at six hours and never runs its later steps, so a build
# that cannot finish in one run has to stop on its own terms, keep its tree and
# let the next run continue. Everything up to the build counts against the
# budget, because the caller sized it for the whole job.
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

# A prefix counts as finished only when every artefact the bootstrap and the CI
# verifier need is there. A partially installed tree (ninja interrupted during
# the install target) must not be mistaken for a complete one.
install_complete() {
    for tool in llvm-config octagram-llvm-config llc llvm-nm; do
        [ -x "$install_prefix/bin/$tool" ] || return 1
    done
    set +e
    set -- "$install_prefix"/lib/libLLVM*.a
    set -e
    [ -e "$1" ] || return 1
    return 0
}

# The CMake flag vector is the single source for both the configure step and
# the build-input stamp: an orchestration-only edit (budget, signals,
# checkpoints) cannot change the stamp, while a changed flag does.
cmake_flags() {
    cat <<EOF
-G
Ninja
-DCMAKE_TOOLCHAIN_FILE=$toolchain_file
-DCMAKE_BUILD_TYPE=Release
-DCMAKE_INSTALL_PREFIX=$install_prefix
-DLLVM_TARGETS_TO_BUILD=X86
-DLLVM_ENABLE_PROJECTS=
-DLLVM_ENABLE_RUNTIMES=
-DLLVM_BUILD_LLVM_DYLIB=OFF
-DLLVM_LINK_LLVM_DYLIB=OFF
-DLLVM_BUILD_SHARED_LIBS=OFF
-DBUILD_SHARED_LIBS=OFF
-DLLVM_ENABLE_THREADS=OFF
-DLLVM_ENABLE_EH=OFF
-DLLVM_ENABLE_RTTI=OFF
-DLLVM_ENABLE_TERMINFO=OFF
-DLLVM_ENABLE_ZLIB=OFF
-DLLVM_ENABLE_ZSTD=OFF
-DLLVM_ENABLE_LIBXML2=OFF
-DLLVM_ENABLE_ASSERTIONS=OFF
-DLLVM_ENABLE_UNWIND_TABLES=OFF
-DLLVM_ENABLE_BINDINGS=OFF
-DLLVM_INCLUDE_TESTS=OFF
-DLLVM_INCLUDE_BENCHMARKS=OFF
-DLLVM_INCLUDE_EXAMPLES=OFF
-DLLVM_INCLUDE_DOCS=OFF
-DLLVM_INCLUDE_UTILS=OFF
-DLLVM_BUILD_TOOLS=ON
-DLLVM_TOOL_LLVM_EXEGESIS_BUILD=OFF
-DLLVM_TOOL_LLI_BUILD=OFF
-DLLVM_TOOL_LLVM_JITLINK_BUILD=OFF
-DLLVM_TOOL_LLVM_RTDYLD_BUILD=OFF
-DLLVM_INSTALL_UTILS=OFF
-DLLVM_OPTIMIZED_TABLEGEN=ON
-DLLVM_BUILD_EXTERNAL_COMPILER_RT=OFF
-DLLVM_INSTALL_TOOLCHAIN_ONLY=OFF
EOF
}

file_digest() {
    if [ ! -f "$1" ]; then
        echo missing
        return 0
    fi
    sha256sum "$1" | cut -d' ' -f1
}

# Content digest of a directory, independent of mtimes and walk order.
tree_digest() {
    if [ ! -d "$1" ]; then
        echo absent
        return 0
    fi
    ( cd "$1" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum ) \
        | sha256sum | cut -d' ' -f1
}

# Everything that decides what the install tree contains. The workflow restores
# whatever libLLVM cache key it finds, so this stamp -- written into the prefix
# by the build -- is what tells a tree built for these inputs apart from one
# linked against an older sysroot or built with other flags.
build_inputs_stamp() {
    {
        echo "llvm_version=$llvm_version"
        echo "toolchain_file=$(file_digest "$toolchain_file")"
        echo "llvm_config_generator=$(file_digest "$repo_root/tools/octagram_llvm_config.py")"
        echo "sysroot=$(tree_digest "$sysroot")"
        echo "cmake_flags:"
        cmake_flags
    } | sha256sum | cut -d' ' -f1
}

expected_input_stamp=$(build_inputs_stamp)

installed_input_stamp() {
    sed -n 's/^stamp=//p' "$install_stamp_file" 2>/dev/null | head -n 1
}

write_input_stamp() {
    cat > "$install_stamp_file" <<EOF
stamp=$expected_input_stamp
llvm_version=$llvm_version
toolchain_file=$toolchain_file
sysroot=$sysroot
updated_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF
}

# --check-install-inputs: answer the workflow's question without building.
if [ "$check_install_inputs" -eq 1 ]; then
    if ! install_complete; then
        echo "build_llvm_octagram: $install_prefix is not a complete install" >&2
        exit 1
    fi
    found_stamp=$(installed_input_stamp)
    if [ "$found_stamp" = "$expected_input_stamp" ]; then
        echo "OCTAGRAM-LLVM-INSTALL-INPUTS-MATCH: $install_prefix ($found_stamp)"
        exit 0
    fi
    echo "build_llvm_octagram: $install_prefix was built from different inputs" >&2
    echo "build_llvm_octagram:   expected stamp $expected_input_stamp" >&2
    echo "build_llvm_octagram:   found    stamp ${found_stamp:-none}" >&2
    exit 1
fi

# A budget stop must always end with the state file written and the tree quiet:
# GitHub kills the job at six hours without running the checkpoint save step, so
# a build that ignores the stop signal loses every hour spent on it. The driver
# gets SIGINT first (ninja finishes the edge it already started), then SIGTERM,
# then SIGKILL. Its children are signalled too, because killing only the driver
# leaves compilers writing into the tree that is about to be checkpointed.
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

for tool in cmake ninja gcc g++ curl tar python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "build_llvm_octagram: error: '$tool' is required on the host" >&2
        exit 1
    fi
done

sysroot=${OCTAGRAM_SYSROOT:-$repo_root/user/build/sysroot-octagram}
if [ ! -f "$sysroot/usr/lib/libc++.a" ]; then
    echo "build_llvm_octagram: error: no OCTAGRAM sysroot at $sysroot" >&2
    echo "build_llvm_octagram: run 'make -C user sysroot' first" >&2
    exit 1
fi

# A missing libc/libc++ entry point is much cheaper to find now than an hour
# into the LLVM build, so link the runtime probes first.
echo "checking OCTAGRAM runtime link probes before the LLVM build"
if ! "$repo_root/tools/check_octagram_runtime_probes.sh"; then
    echo "build_llvm_octagram: error: the runtime probes did not link" >&2
    echo "build_llvm_octagram: fix the reported libc/libc++ gap first" >&2
    exit 1
fi

if [ ! -f "$toolchain_file" ]; then
    echo "build_llvm_octagram: error: missing $toolchain_file" >&2
    exit 1
fi

if [ "$clean" -eq 1 ]; then
    rm -rf "$cmake_build" "$install_prefix" "$work_dir/build-state.json"
fi

mkdir -p "$work_dir"

# A complete prefix is reused only when the stamp it carries names these exact
# inputs. A tree restored from an older cache (another sysroot, another flag
# set) is deleted together with the build tree it was produced from, because a
# rebuild against the old CMakeCache would mix the two.
if install_complete && [ "$clean" -eq 0 ]; then
    found_stamp=$(installed_input_stamp)
    if [ "$found_stamp" = "$expected_input_stamp" ]; then
        # Rerunning after a complete build is cheap: keep the tree, re-check the
        # wrapper, and report what was reused instead of spending hours again.
        echo "libLLVM $llvm_version is already installed at $install_prefix"
        write_state complete
        reused=1
    else
        echo "build_llvm_octagram: $install_prefix was built from different inputs" >&2
        echo "build_llvm_octagram:   expected stamp $expected_input_stamp" >&2
        echo "build_llvm_octagram:   found    stamp ${found_stamp:-none}" >&2
        echo "build_llvm_octagram: discarding the install and build trees; rebuilding" >&2
        rm -rf "$install_prefix" "$cmake_build"
        resume=0
        reused=0
    fi
else
    reused=0
fi

if [ "$resume" -eq 1 ] && [ ! -f "$cmake_build/CMakeCache.txt" ]; then
    echo "build_llvm_octagram: error: --resume but no configured build tree at $cmake_build" >&2
    echo "build_llvm_octagram: rerun without --resume to configure a new one" >&2
    exit 1
fi

if [ "$reused" -eq 0 ]; then
    if [ -z "$source_dir" ]; then
        source_dir=$work_dir/llvm-project-$llvm_version.src
        if [ ! -f "$source_dir/llvm/CMakeLists.txt" ]; then
            tarball=$work_dir/llvm-project-$llvm_version.src.tar.xz
            if [ ! -f "$tarball" ]; then
                url="https://github.com/llvm/llvm-project/releases/download/llvmorg-$llvm_version/llvm-project-$llvm_version.src.tar.xz"
                echo "downloading $url"
                # -C - continues an interrupted download; the .part file is
                # kept between runs so a dropped connection does not restart
                # the whole 1.5 GiB transfer.
                if ! curl -fL --retry 3 --retry-all-errors -C - \
                        -o "$tarball.part" "$url"; then
                    echo "build_llvm_octagram: download failed; discarding the partial file" >&2
                    rm -f "$tarball.part"
                    curl -fL --retry 3 --retry-all-errors -o "$tarball.part" "$url"
                fi
                if [ -n "${LLVM_SHA256:-}" ]; then
                    actual=$(sha256sum "$tarball.part" | cut -d' ' -f1)
                    if [ "$actual" != "$LLVM_SHA256" ]; then
                        rm -f "$tarball.part"
                        echo "build_llvm_octagram: sha256 mismatch for LLVM $llvm_version" >&2
                        echo "  expected $LLVM_SHA256" >&2
                        echo "  actual   $actual" >&2
                        exit 1
                    fi
                else
                    echo "build_llvm_octagram: LLVM_SHA256 not set; pin it for reproducible builds" >&2
                fi
                mv "$tarball.part" "$tarball"
            fi
            # An interrupted tar leaves a partial tree; remove it before
            # extracting again, otherwise the next run builds against it.
            if [ -d "$source_dir" ]; then
                echo "removing incomplete source tree $source_dir"
                rm -rf "$source_dir"
            fi
            echo "extracting $tarball"
            tar -xJf "$tarball" -C "$work_dir"
        fi
    fi

    if [ ! -f "$source_dir/llvm/CMakeLists.txt" ]; then
        echo "build_llvm_octagram: error: $source_dir is not an LLVM source tree" >&2
        exit 1
    fi

    actual_version=$(sed -n 's/^set(LLVM_VERSION_MAJOR \([0-9]*\)).*/\1/p' "$source_dir/llvm/CMakeLists.txt" | head -n 1)
    if [ -n "$actual_version" ] && [ "$actual_version" != "$llvm_short" ]; then
        echo "build_llvm_octagram: error: source is LLVM $actual_version, expected $llvm_short" >&2
        exit 1
    fi

    # The guest has no dynamic loader for the toolchain and no TLS in shared
    # objects, so everything is static. Threads and terminfo are off because the
    # guest's pthread/TLS surface is still being hardened; LLVM only needs them to
    # parallelise work, and a single-threaded rustc is acceptable for bootstrap.
    # llvm-exegesis is a benchmarking utility, not part of the OCTAGRAM
    # compiler/linker toolchain. It requires host tracing APIs (sys/ptrace.h)
    # that the target sysroot intentionally does not provide.
    #
    # The target ABI intentionally has no libgcc_eh / frame-registration runtime.
    # Keep the static compiler libraries, but skip standalone JIT executables:
    # lli, llvm-jitlink, and llvm-rtdyld pull ORC/RuntimeDyld EH-frame registration
    # and fail to link __register_frame/__deregister_frame under OCTAGRAM -nostdlib.
    # They are not required by rustc's code-generation backend or the tools below.
    #
    # Build the argument vector directly instead of word-splitting a multiline
    # string. This keeps paths with spaces intact and prevents shell comments or
    # explanatory prose from being passed to CMake as unexpected arguments.
    # cmake_flags() is also what the build-input stamp hashes, so the flag set
    # and the stamp can never drift apart. One flag per line keeps paths with
    # spaces intact.
    set --
    while IFS= read -r cmake_flag; do
        set -- "$@" "$cmake_flag"
    done <<CMAKE_ARGUMENTS
$(cmake_flags)
CMAKE_ARGUMENTS

    if [ "$resume" -eq 1 ] && [ -f "$cmake_build/CMakeCache.txt" ]; then
        # Reconfiguring a resumed tree is wasted budget: ninja re-runs CMake on
        # its own when any CMakeLists.txt changed, so the cached configuration
        # is still checked for staleness before anything is compiled.
        echo "resuming the configured build tree: $cmake_build"
    else
        echo "configuring LLVM $llvm_version for x86_64-unknown-octagram"
        echo "  source:  $source_dir"
        echo "  build:   $cmake_build"
        echo "  prefix:  $install_prefix"
        cmake -S "$source_dir/llvm" -B "$cmake_build" "$@"
    fi

    # bin/llvm-config is an OCTAGRAM ELF and cannot run on the build machine, but
    # bootstrap executes llvm-config while linking rustc. Generate a host-runnable
    # replacement from the build tree's own component manifest so the reported
    # flags and library order match this install exactly.
    echo "generating host-runnable octagram-llvm-config"
    python3 "$repo_root/tools/octagram_llvm_config.py" \
        --build-dir "$cmake_build" \
        --prefix "$install_prefix" \
        --output "$install_prefix/bin/octagram-llvm-config"

    budget_seconds=$(remaining_budget_seconds)
    if [ "$time_budget" -gt 0 ] && [ "$budget_seconds" -eq 0 ]; then
        write_state incomplete
        echo "OCTAGRAM-LLVM-BUILD-INCOMPLETE: the ${time_budget}m budget was spent before compiling" >&2
        echo "build_llvm_octagram: build tree kept at $cmake_build; rerun to continue" >&2
        exit 75
    fi

    echo "building libLLVM, llc, llvm-nm, llvm-config (-j$jobs)"
    if [ "$time_budget" -gt 0 ]; then
        echo "  time budget: ${budget_seconds}s remaining of ${time_budget}m"
    fi
    budget_marker=$work_dir/budget-exhausted
    stop_signal_file=$work_dir/stop-signal
    stop_pids_file=$work_dir/stop-pids
    rm -f "$budget_marker" "$stop_signal_file" "$stop_pids_file"
    # ninja handles SIGINT by finishing the edges it already started and
    # stopping, which leaves a tree the next run can continue. cmake --build
    # does not forward the signal to ninja, so run ninja directly.
    ( cd "$cmake_build" && exec ninja -j "$jobs" install ) &
    build_pid=$!
    watcher=
    if [ "$budget_seconds" -gt 0 ]; then
        (
            sleep "$budget_seconds"
            : > "$budget_marker"
            # Escalate until the driver is gone: an edge stuck in one long
            # compile ignores SIGINT, and the job would then be killed at six
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

    if [ -f "$budget_marker" ] && [ "$build_status" -ne 0 ]; then
        write_state incomplete
        case $stop_signal in
            ''|INT) ;;
            *) echo "build_llvm_octagram: warning: ninja ignored SIGINT; stopped with SIG$stop_signal" >&2 ;;
        esac
        echo "OCTAGRAM-LLVM-BUILD-INCOMPLETE: stopped at the ${time_budget}m time budget" >&2
        echo "build_llvm_octagram: ninja was stopped; the build tree is kept at" >&2
        echo "build_llvm_octagram:   $cmake_build" >&2
        echo "build_llvm_octagram: rerun the same command to continue from here." >&2
        exit 75
    fi
    if [ "$build_status" -ne 0 ]; then
        write_state failed
        echo "build_llvm_octagram: error: ninja failed with status $build_status" >&2
        exit 1
    fi

    if ! install_complete; then
        write_state incomplete
        echo "build_llvm_octagram: error: ninja finished but the install is incomplete" >&2
        echo "build_llvm_octagram: expected bin/{llvm-config,octagram-llvm-config,llc,llvm-nm}" >&2
        echo "build_llvm_octagram: and lib/libLLVM*.a under $install_prefix" >&2
        exit 1
    fi
    # The tree now provably comes from these inputs; the workflow may cache it.
    write_input_stamp
fi

llvm_config=$install_prefix/bin/llvm-config
if [ ! -x "$llvm_config" ]; then
    echo "build_llvm_octagram: error: install produced no llvm-config" >&2
    exit 1
fi

# Runs on this machine, so it can be checked here rather than in the guest.
host_llvm_config=$install_prefix/bin/octagram-llvm-config
if [ ! -x "$host_llvm_config" ]; then
    echo "build_llvm_octagram: error: no host-runnable octagram-llvm-config" >&2
    exit 1
fi
reported_version=$("$host_llvm_config" --version 2>/dev/null || true)
if [ "$reported_version" != "$llvm_version" ]; then
    echo "build_llvm_octagram: error: octagram-llvm-config reports '$reported_version'," >&2
    echo "build_llvm_octagram:        expected $llvm_version" >&2
    exit 1
fi
library_count=$("$host_llvm_config" --libs 2>/dev/null | wc -w | tr -d ' ')
if [ "${library_count:-0}" -eq 0 ]; then
    echo "build_llvm_octagram: error: octagram-llvm-config reported no libraries" >&2
    exit 1
fi

write_state complete

echo
if [ "$reused" -eq 1 ]; then
    echo "OCTAGRAM-LLVM-BUILD-REUSED: libLLVM for x86_64-unknown-octagram already at $install_prefix"
else
    echo "libLLVM for x86_64-unknown-octagram installed at $install_prefix"
fi
# Do not execute the target llvm-config on the Linux build runner.
echo "  host llvm-config:     $host_llvm_config ($reported_version, $library_count libraries)"
echo "  target llvm-config:   $llvm_config (OCTAGRAM ELF; not executed on host)"
echo "  static libs:          $("$host_llvm_config" --libdir)"
echo "  build state:          $state_file"
echo
echo "Next: tools/bootstrap_rustc_octagram.sh --llvm-prefix $install_prefix"
echo "The produced binaries are OCTAGRAM ELFs: they cannot be executed on this"
echo "host. Guest verification requires copying them into an image and running"
echo "them from the OCTAGRAM shell."
