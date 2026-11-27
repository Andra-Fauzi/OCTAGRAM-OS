#!/usr/bin/env python3
"""Guard the resumable LLVM CI contract.

GitHub terminates a hosted job at six hours and never runs its remaining
steps, and its dependency cache is 10 GB per repository by default. A full
libLLVM cross-build does not fit in one run, so the workflow must:

  * stop itself before the six-hour ceiling (--time-budget < timeout-minutes);
  * keep the CMake build tree and save it after a budget stop or safe external cancellation;
  * key install and build caches by the exact build-input fingerprint;
  * restore that checkpoint and pass --resume on the next run;
  * never cache a partial install tree under the "install" key.

A stop that hangs is as bad as no stop: GitHub would kill the job at six hours
and the checkpoint save step would never run. Budget stops escalate from SIGINT
to SIGTERM and SIGKILL; explicit cancellation uses a bounded signal handler to
quiesce Ninja before the always-run cache-save step. The rustc checkpoint also drops the stage0
compiler tree, which the next run re-copies from the pinned toolchain, so the
multi-GB checkpoint fits the 10 GB repository cache quota.

These are text assertions on the workflow and the build script. They catch the
regression that matters: an edit that silently makes the build start over, or
that reports an unfinished build as done. No LLVM build is needed.

Usage:
    tools/check_octagram_llvm_ci.py
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
WORKFLOW = REPO / ".github" / "workflows" / "octagram-native-toolchain.yml"
BUILD = REPO / "tools" / "build_llvm_octagram.sh"
BOOTSTRAP = REPO / "tools" / "bootstrap_rustc_octagram.sh"
STD_PATCH = REPO / "tools" / "patch_rust_std_octagram.py"
HOST_TARGET_PATCH = REPO / "tools" / "apply_rust_octagram_host_target.py"
GETRANDOM_PATCH = REPO / "tools" / "patch_getrandom_octagram.py"
GITHUB_JOB_LIMIT_MINUTES = 360

failures: list[str] = []


def fail(message: str) -> None:
    failures.append(message)
    print(f"FAIL: {message}")


def job_block(text: str, name: str) -> str:
    """Return the YAML text of one top-level job."""
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [a-z][a-z0-9-]*:\n|\Z)", text, re.M | re.S)
    if not match:
        fail(f"workflow has no '{name}' job")
        return ""
    return match.group(1)


def require(haystack: str, needle: str, message: str) -> None:
    if needle not in haystack:
        fail(message)


def step_block(job: str, name: str) -> str:
    """Return one step's YAML, from its name to the next step."""
    start = job.find(f"name: {name}")
    if start < 0:
        return ""
    following = job.find("\n      - ", start)
    return job[start:] if following < 0 else job[start:following]


def main() -> int:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    build = BUILD.read_text(encoding="utf-8")
    bootstrap = BOOTSTRAP.read_text(encoding="utf-8")
    std_patch = STD_PATCH.read_text(encoding="utf-8")
    host_target_patch = HOST_TARGET_PATCH.read_text(encoding="utf-8")
    getrandom_patch = GETRANDOM_PATCH.read_text(encoding="utf-8")

    # --- the build script ---------------------------------------------------
    for flag in ("--time-budget", "--resume", "--status", "--print-build-inputs-stamp"):
        require(build, flag, f"build_llvm_octagram.sh no longer accepts {flag}")
    require(
        build,
        "exit 75",
        "build_llvm_octagram.sh must exit 75 (EX_TEMPFAIL) when the budget runs out",
    )
    require(
        build,
        "octagram-build-state.json",
        "build_llvm_octagram.sh must record build state for the incomplete marker",
    )
    # A budget stop must always reach the state file. SIGINT alone is not enough
    # when an edge ignores it: the job would then run into GitHub's six-hour
    # limit and never save the checkpoint.
    require(
        build,
        "for signal in INT TERM KILL",
        "build_llvm_octagram.sh must escalate the budget stop from SIGINT to SIGTERM and SIGKILL",
    )
    require(
        build,
        "wait_for_stopped_children",
        "build_llvm_octagram.sh must wait for the stopped build children before checkpointing",
    )
    require(
        build,
        '"stop_signal": "$stop_signal"',
        "build_llvm_octagram.sh must record which signal stopped the build",
    )
    require(
        build,
        "external_cancel_checkpoint",
        "build_llvm_octagram.sh must quiesce Ninja when the GitHub job is cancelled",
    )
    require(
        build,
        "trap 'external_cancel_checkpoint INT' INT",
        "build_llvm_octagram.sh must trap external SIGINT while Ninja is running",
    )
    require(
        build,
        "trap 'external_cancel_checkpoint TERM' TERM",
        "build_llvm_octagram.sh must trap external SIGTERM while Ninja is running",
    )
    require(
        build,
        "-C -",
        "build_llvm_octagram.sh must resume an interrupted LLVM tarball download",
    )

    # --- the llvm job -------------------------------------------------------
    llvm = job_block(workflow, "llvm")
    timeout = re.search(r"^    timeout-minutes: (\d+)$", llvm, re.M)
    if not timeout:
        fail("llvm job has no timeout-minutes")
        timeout_minutes = GITHUB_JOB_LIMIT_MINUTES
    else:
        timeout_minutes = int(timeout.group(1))
        if timeout_minutes > GITHUB_JOB_LIMIT_MINUTES:
            fail(
                f"llvm timeout-minutes is {timeout_minutes}; GitHub kills hosted jobs at "
                f"{GITHUB_JOB_LIMIT_MINUTES} and the checkpoint save would never run"
            )
        if timeout_minutes < 60:
            fail(f"llvm timeout-minutes is only {timeout_minutes}; a checkpoint save needs room")

    budget = re.search(r"^\s+time_budget:\n(?:.*\n)*?\s+default: \"(\d+)\"", workflow, re.M)
    if not budget:
        fail("workflow_dispatch has no time_budget input")
    else:
        budget_minutes = int(budget.group(1))
        if budget_minutes >= timeout_minutes:
            fail(
                f"default time_budget {budget_minutes}m is not below timeout-minutes "
                f"{timeout_minutes}m; the job would be killed before it saves a checkpoint"
            )
        if timeout_minutes - budget_minutes < 20:
            fail(
                f"only {timeout_minutes - budget_minutes}m between the time budget and the "
                "job timeout; cache restore and the checkpoint save need more room"
            )
    require(workflow, "fresh_build:", "workflow_dispatch has no fresh_build input")

    # Compute one fingerprint and use it for both LLVM install and build caches.
    fingerprint = step_block(llvm, "Compute exact LLVM build input fingerprint")
    if not fingerprint:
        fail("llvm job does not compute the exact build-input fingerprint before cache restore")
    else:
        require(
            fingerprint,
            "--print-build-inputs-stamp",
            "cache fingerprint step must call the build script's stamp function",
        )
    # The build step must pass the budget, resume a restored tree, and turn the
    # script's exit 75 into a successful job with incomplete=true.
    require(llvm, "--time-budget", "the llvm build step does not pass --time-budget")
    require(llvm, "--resume", "the llvm build step never resumes a restored build tree")
    require(
        llvm,
        '"$status" -eq 75',
        "the llvm build step does not handle the script's exit 75",
    )
    require(
        llvm,
        "incomplete=true",
        "the llvm build step does not publish incomplete=true to the job output",
    )
    if re.search(r'"\$status" -eq 75\)\n(?:\s+.*\n){0,6}?\s+exit 1\n', llvm):
        fail("the llvm build step turns exit 75 into a failure instead of a checkpoint")
    # --- the rustc bootstrap script ----------------------------------------
    for flag in ("--time-budget", "--resume", "--status"):
        require(bootstrap, flag, f"bootstrap_rustc_octagram.sh no longer accepts {flag}")
    require(
        bootstrap,
        "exit 75",
        "bootstrap_rustc_octagram.sh must exit 75 (EX_TEMPFAIL) when the budget runs out",
    )
    require(
        bootstrap,
        "octagram-build-state.json",
        "bootstrap_rustc_octagram.sh must record build state for the incomplete marker",
    )
    require(
        bootstrap,
        "for signal in INT TERM KILL",
        "bootstrap_rustc_octagram.sh must escalate the budget stop from SIGINT to SIGTERM and SIGKILL",
    )
    require(
        bootstrap,
        "wait_for_stopped_children",
        "bootstrap_rustc_octagram.sh must wait for the stopped build children before checkpointing",
    )
    require(
        bootstrap,
        '"stop_signal": "$stop_signal"',
        "bootstrap_rustc_octagram.sh must record which signal stopped the build",
    )
    # The rustc checkpoint is the largest cache entry: drop the stage0 compiler
    # tree, which bootstrap re-copies from the pinned toolchain, instead of
    # pushing the repository past its 10 GB cache quota.
    require(
        bootstrap,
        "prune_checkpoint",
        "bootstrap_rustc_octagram.sh must prune the checkpoint before saving it",
    )
    require(
        bootstrap,
        "submodules = false",
        "Rust bootstrap should not initialize the full llvm-project source submodule",
    )
    require(
        bootstrap,
        "optimized-compiler-builtins = false",
        "Rust bootstrap must use the Rust-only compiler-builtins implementation when llvm-project is intentionally absent",
    )

    require(
        bootstrap,
        "cargo fetch",
        "Rust bootstrap must fetch the exact libc crate before patching its registry source",
    )
    require(
        bootstrap,
        'libc = { version = "=$LIBC_VERSION", default-features = false }',
        "the libc registry prefetch must pin the same version used by Rust std",
    )
    require(
        bootstrap,
        '"$cargo_home"/registry/src/*/libc-"$LIBC_VERSION"',
        "Rust bootstrap must locate the extracted libc source Cargo actually compiles",
    )
    require(
        bootstrap,
        '"$registry_source" --allow-unpinned',
        "Rust bootstrap must apply the OCTAGRAM patch to the Cargo registry source, not only the upstream checkout",
    )
    require(
        bootstrap,
        "RUST-LIBC-CARGO-REGISTRY-PATCH-PASS",
        "Rust bootstrap must fail unless at least one Cargo registry libc source was patched",
    )
    require(
        bootstrap,
        "libc_checkout=$repo_root/$libc_checkout",
        "relative upstream libc checkout paths must resolve from the repository root",
    )

    require(
        bootstrap,
        'OCTAGRAM_RUST_STD_ROOT="$rust_checkout/library/std/src"',
        "Rust bootstrap must patch the pinned library/std sources that x.py build-std actually compiles",
    )
    require(
        bootstrap,
        'python3 "$repo_root/tools/patch_rust_std_octagram.py"',
        "Rust bootstrap must invoke the OCTAGRAM std PAL patch before building std",
    )
    require(
        std_patch,
        "OCTAGRAM_RUST_STD_ROOT",
        "the std PAL patcher must accept an explicit source checkout, not only installed rust-src",
    )
    require(
        std_patch,
        "if new in source:",
        "the std PAL patcher must be safe to rerun after a restored bootstrap checkpoint",
    )
    require(
        std_patch,
        'pub(crate) mod octagram;',
        "the std PAL patcher must register the OCTAGRAM os module for Unix fs/raw extensions",
    )
    require(
        std_patch,
        'pub fn set_name(_name: &CStr)',
        "the std PAL patcher must provide the OCTAGRAM Unix thread set_name implementation",
    )
    # Do not enable Linux-only helpers whose consumers are intentionally not
    # compiled for OCTAGRAM. Otherwise they trigger -D warnings or accidentally
    # rely on Linux-only syscalls.
    for excluded in (
        "io/stdio.rs",
        "sys/fs/mod.rs",
        "sys/pal/unix/weak/mod.rs",
        "sys/process/mod.rs",
        "sys/process/unix/mod.rs",
        "sys/process/unix/unix.rs",
        "sys/process/unix/common.rs",
        "os/net/linux_ext/mod.rs",
    ):
        require(
            std_patch,
            f'"{excluded}"',
            f"the std PAL patcher must keep Linux-only code out of {excluded}",
        )
    require(
        std_patch,
        "def restore_linux_only_cfg_gates(std: Path)",
        "the std patcher must restore Linux-only cfgs in cached OCTAGRAM std checkouts",
    )
    require(
        std_patch,
        'for relative in ("os/net/linux_ext/mod.rs", "sys/process/unix/common.rs")',
        "the std patcher must keep Linux networking extensions and pidfd process state Linux-only",
    )
    require(
        std_patch,
        "restore_linux_only_cfg_gates(std)",
        "the std patcher must restore cached cfgs before applying the global Linux PAL widening",
    )
    require(
        std_patch,
        "def gate_linux_extension_modules(std: Path)",
        "the std patcher must keep Linux ancillary/TCP extension modules out of OCTAGRAM",
    )
    require(
        std_patch,
        "gate_linux_extension_modules(std)",
        "the std patcher must gate Linux-only networking extension modules",
    )
    require(
        std_patch,
        "def allow_octagram_internal_socket_addr_ext(std: Path)",
        "the std patcher must handle the internal abstract-socket trait without disabling it",
    )
    require(
        std_patch,
        'cfg_attr(target_os = "octagram", allow(dead_code))',
        "only the intentionally internal OCTAGRAM SocketAddrExt helper may allow dead_code",
    )
    require(
        std_patch,
        "allow_octagram_internal_socket_addr_ext(std)",
        "the std patcher must apply the OCTAGRAM abstract-socket helper visibility fix",
    )
    require(
        std_patch,
        "RUST-STD-OCTAGRAM-LINUX-EXTENSIONS-PASS",
        "the std patcher must report that Linux-only extension modules remain gated",
    )
    require(
        std_patch,
        "def preserve_linux_socket_option_cfgs(std: Path)",
        "the std patcher must keep unsupported Linux socket-option helpers out of OCTAGRAM",
    )
    for method in (
        "set_quickack",
        "quickack",
        "set_deferaccept",
        "deferaccept",
        "set_passcred",
        "passcred",
    ):
        require(
            std_patch,
            f'("{method}",',
            f"the std patcher must retain the Linux-only cfg gate for Socket::{method}",
        )
    require(
        std_patch,
        "preserve_linux_socket_option_cfgs(std)",
        "the std patcher must apply Linux-only socket-option gates after widening shared Unix cfgs",
    )
    require(
        std_patch,
        "RUST-STD-OCTAGRAM-LINUX-SOCKET-OPTIONS-PASS",
        "the std patcher must report that unsupported Linux socket-option methods were excluded",
    )
    require(
        std_patch,
        "RUST-STD-OCTAGRAM-LINUX-ONLY-CFG-PASS",
        "the std patcher must report restoration of cached Linux-only cfgs",
    )
    if 'pub(in crate::sys) use cfm::CachedFileMetadata;' in std_patch:
        fail("OCTAGRAM must use generic io::copy, not export Linux-only CachedFileMetadata")
    require(
        std_patch,
        '"sys/process/mod.rs"',
        "the std PAL patcher must not enable the Linux-only PidFd re-export for OCTAGRAM",
    )
    require(
        std_patch,
        'target_os = "octagram" => unsafe {',
        "std socket accept must use the OCTAGRAM accept()+fcntl fallback, not accept4",
    )
    require(
        std_patch,
        "widened_accept_select = octagram_accept_select.replace(",
        "the std socket accept patch must remain idempotent after Linux cfg widening on checkpoint resume",
    )
    require(
        std_patch,
        "if octagram_accept_select in socket_source or widened_accept_select in socket_source:",
        "the std socket accept patch must recognize both pristine and already-widened checkpoint states",
    )
    require(
        std_patch,
        'RUST-STD-OCTAGRAM-ACCEPT-FALLBACK-PASS',
        "the std PAL patcher must verify the accept4 fallback for OCTAGRAM",
    )
    require(
        std_patch,
        'stack_overflow_mod = std / "sys/pal/unix/stack_overflow/mod.rs"',
        "the std PAL patcher must avoid Linux pthread stack introspection on OCTAGRAM",
    )
    require(
        std_patch,
        'RUST-STD-OCTAGRAM-STACK-OVERFLOW-NONE-PASS',
        "the std PAL patcher must report the OCTAGRAM stack-overflow fallback",
    )
    require(
        std_patch,
        'os_mod = std / "os/mod.rs"',
        "the std PAL patcher must keep the Linux-specific public os module under review",
    )
    require(
        std_patch,
        'RUST-STD-OCTAGRAM-LINUX-OS-MODULE-PASS',
        "the std PAL patcher must keep std::os::linux unselected on OCTAGRAM",
    )
    require(
        std_patch,
        "condition == '#[cfg(target_os = \"linux\")]'",
        "the std PAL patcher must recognize the restored Linux-only os::linux cfg",
    )
    # Cargo's --print=file-names probe parses rustc's unsupported-crate-type
    # diagnostic to skip Dylib/ProcMacro on this static-only target. Never
    # suppress that warning: doing so shifts Cargo's output-line parser.
    require(
        host_target_patch,
        'crate-type = ["rlib"]',
        "the OCTAGRAM rustc_driver must be an rlib linked into the static rustc-main executable",
    )
    require(
        host_target_patch,
        "RUST-OCTAGRAM-STATIC-RUSTC-DRIVER-PASS",
        "the Rust target patch must report the static rustc_driver strategy",
    )
    require(
        host_target_patch,
        "if rustc_driver.is_file() {",
        "bootstrap must strip rustc_driver only when a shared driver artifact exists",
    )
    require(
        host_target_patch,
        "RUST-OCTAGRAM-STATIC-BOOTSTRAP-PASS",
        "the Rust target patch must report the static driver bootstrap adjustment",
    )
    require(
        host_target_patch,
        "Keep rustc's unsupported-crate-type warning intact.",
        "the Rust target patch must preserve the diagnostic Cargo uses for unsupported crate types",
    )
    require(
        host_target_patch,
        "RUST-OCTAGRAM-CRATE-TYPE-DIAGNOSTICS-PASS",
        "the Rust target patch must report that Cargo crate-type diagnostics are preserved",
    )
    require(
        host_target_patch,
        "if new_diagnostic in passes_source:",
        "the Rust target patch must restore the original diagnostic in cached checkouts",
    )
    require(
        host_target_patch,
        "elif old_diagnostic not in passes_source:",
        "the Rust target patch must fail safely when neither known diagnostic form is present",
    )
    require(
        host_target_patch,
        "passes_source = passes_source.replace(new_diagnostic, old_diagnostic, 1)",
        "the Rust target patch must remove the stale OCTAGRAM-only Dylib warning suppression",
    )

    # getrandom 0.3.3 must use OCTAGRAM libc's real getrandom() API,
    # not an unsupported-target bypass or a CPU-specific substitute.
    require(
        getrandom_patch,
        'target_os = "octagram"',
        "the getrandom patcher must add an OCTAGRAM-specific backend",
    )
    require(
        getrandom_patch,
        "mod getrandom;",
        "the getrandom patcher must select the upstream libc-backed implementation",
    )
    require(
        getrandom_patch,
        "libc::getrandom",
        "the getrandom patcher must use OCTAGRAM libc getrandom()",
    )
    require(
        getrandom_patch,
        "RUST-GETRANDOM-OCTAGRAM-BACKEND-PASS",
        "the getrandom patcher must emit a verifiable pass marker",
    )
    require(
        bootstrap,
        'GETRANDOM_VERSION=0.3.3',
        "the bootstrap must pin the getrandom source version it patches",
    )
    require(
        bootstrap,
        'octagram-getrandom-fetch',
        "the bootstrap must fetch getrandom before stage-2 compilation",
    )
    require(
        bootstrap,
        'patch_getrandom_octagram.py',
        "the bootstrap must apply the OCTAGRAM getrandom backend patch",
    )
    require(
        bootstrap,
        'RUST-GETRANDOM-OCTAGRAM-BACKEND-PASS',
        "the bootstrap must verify that the registry getrandom backend was patched",
    )

    # --- the rustc-cargo job -----------------------------------------------
    rustc = job_block(workflow, "rustc-cargo")
    require(
        rustc,
        "needs.llvm.outputs.incomplete != 'true'",
        "rustc-cargo must not run on an incomplete libLLVM",
    )
    rustc_timeout = re.search(r"^    timeout-minutes: (\d+)$", rustc, re.M)
    if not rustc_timeout:
        fail("rustc-cargo job has no timeout-minutes")
    elif int(rustc_timeout.group(1)) > GITHUB_JOB_LIMIT_MINUTES:
        fail(
            f"rustc-cargo timeout-minutes is {rustc_timeout.group(1)}; GitHub kills hosted "
            "jobs at 360 and the checkpoint save would never run"
        )
    require(rustc, "TIME_BUDGET", "rustc-cargo does not pass a time budget to the bootstrap")
    require(rustc, "--time-budget", "the rustc build step does not pass --time-budget")
    require(rustc, "--resume", "the rustc build step never resumes a restored checkout")
    require(
        rustc,
        '"$status" -eq 75',
        "the rustc build step does not handle the bootstrap's exit 75",
    )
    require(
        rustc,
        "incomplete=true",
        "the rustc build step does not publish incomplete=true to the job output",
    )

    rustc_restore = step_block(rustc, "Restore rustc bootstrap checkpoint")
    if not rustc_restore:
        fail("rustc-cargo job does not restore a bootstrap checkpoint")
    else:
        require(
            rustc_restore,
            "actions/cache/restore@v4",
            "rustc checkpoint must be restored with actions/cache/restore@v4",
        )
        rustc_prefix = re.search(r"restore-keys: \|\n\s+(\S.*)$", rustc_restore, re.M)
        if not rustc_prefix:
            fail("rustc checkpoint restore has no restore-keys prefix")
        elif not rustc_prefix.group(1).strip().endswith("-"):
            fail("rustc checkpoint restore-keys must be a prefix ending in '-'")

    rustc_save = step_block(rustc, "Save rustc bootstrap checkpoint")
    if not rustc_save:
        fail("rustc-cargo job has no bootstrap checkpoint save step")
    else:
        require(
            rustc_save,
            "incomplete == 'true'",
            "rustc checkpoint must only be saved for an incomplete bootstrap",
        )
        require(
            rustc_save,
            "actions/cache/save@v4",
            "rustc checkpoint must be saved with actions/cache/save@v4",
        )
    require(
        step_block(rustc, "Verify staged toolchain"),
        "incomplete != 'true'",
        "rustc-cargo must not verify or publish a toolchain from an incomplete bootstrap",
    )

    # A checkpoint must be restorable by exact-input prefix and saved after a
    # budget stop or a safely handled external cancellation.
    restore = step_block(llvm, "Restore libLLVM build checkpoint")
    if not restore:
        fail("llvm job does not restore a build checkpoint")
    else:
        require(
            restore,
            "steps.llvm-inputs.outputs.stamp",
            "LLVM build checkpoint key must use the exact build-input fingerprint",
        )
        require(
            restore,
            "actions/cache/restore@v4",
            "build checkpoint must be restored with actions/cache/restore@v4",
        )
        prefix = re.search(r"restore-keys: \|\n\s+(\S.*)$", restore, re.M)
        if not prefix:
            fail("build checkpoint restore has no restore-keys prefix")
        elif not prefix.group(1).strip().endswith("-"):
            fail("build checkpoint restore-keys must be a prefix ending in '-'")

    save = step_block(llvm, "Save libLLVM build checkpoint")
    if not save:
        fail("llvm job has no build checkpoint save step")
    else:
        require(
            save,
            "actions/cache/save@v4",
            "build checkpoint must be saved with actions/cache/save@v4",
        )
        require(
            save,
            "incomplete == 'true'",
            "budget-stopped LLVM checkpoints must still be saved",
        )
        require(
            save,
            "steps.llvm-build.outcome == 'cancelled'",
            "a safely stopped LLVM build must also be saved after user cancellation",
        )
        require(
            save,
            "cancelled()",
            "checkpoint save must run in the cancelled workflow context",
        )

    # The install tree is the "done" marker: it may only be saved after the
    # verifier accepted it, otherwise the next run skips the build entirely.
    install_restore = step_block(llvm, "Restore libLLVM install tree")
    if "actions/cache/restore@v4" not in install_restore:
        fail("libLLVM install tree must be restored with actions/cache/restore@v4")
    require(
        install_restore,
        "steps.llvm-inputs.outputs.stamp",
        "libLLVM install cache key must use the exact build-input fingerprint",
    )
    if "restore-keys:" in install_restore:
        fail("libLLVM install cache must not use a broad restore prefix")
    install_save = step_block(llvm, "Save libLLVM install tree")
    if not install_save:
        fail("libLLVM install tree has no explicit save step")
    else:
        require(
            install_save,
            "actions/cache/save@v4",
            "libLLVM install tree must be saved with actions/cache/save@v4",
        )
        require(
            install_save,
            "incomplete != 'true'",
            "install tree must not be cached when the build is incomplete",
        )
    verify_at = llvm.find("name: Verify libLLVM install")
    install_save_at = llvm.find("name: Save libLLVM install tree")
    if verify_at < 0:
        fail("llvm job has no install verifier")
    elif install_save_at < verify_at:
        fail("install tree is cached before it is verified")

    # --- the build-input stamp ---------------------------------------------
    # The cache key itself is the stamp; the install verifier remains a second
    # line of defence against a damaged or manually populated cache.
    require(
        build,
        ".octagram-build-inputs",
        "build_llvm_octagram.sh must write the build-input stamp into the install prefix",
    )
    require(
        build,
        "write_input_stamp",
        "build_llvm_octagram.sh must write the stamp after a successful install",
    )
    require(
        build,
        "OCTAGRAM-LLVM-INSTALL-INPUTS-MATCH",
        "build_llvm_octagram.sh must answer --check-install-inputs with a pass marker",
    )
    require(
        build,
        "--check-install-inputs",
        "build_llvm_octagram.sh must accept --check-install-inputs",
    )
    require(
        build,
        "required_install_tools=llvm-config,octagram-llvm-config,llc,llvm-nm,llvm-cov",
        "the install stamp must change when the required LLVM tool set changes",
    )
    require(
        build,
        "llvm-cov",
        "a complete LLVM install must include llvm-cov for Rust bootstrap",
    )
    stamp_fn = re.search(r"^build_inputs_stamp\(\) \{\n(.*?)^\}\n", build, re.M | re.S)
    if not stamp_fn:
        fail("build_llvm_octagram.sh has no build_inputs_stamp()")
    else:
        for needle, what in (
            ("$llvm_version", "the LLVM version"),
            ("$toolchain_file", "the toolchain file"),
            ("$sysroot", "the sysroot contents"),
            ("cmake_flags", "the CMake flag vector"),
        ):
            require(
                stamp_fn.group(1),
                needle,
                f"the build-input stamp must cover {what}",
            )
    # Reusing a prefix is decided by the stamp, and a mismatch has to discard the
    # build tree too: an old CMakeCache would mix the old and new inputs.
    reuse = re.search(r"if install_complete && \[ \"\$clean\" -eq 0 \]; then(.*?)\nfi\n", build, re.S)
    if not reuse:
        fail("build_llvm_octagram.sh no longer decides reuse from install_complete")
    else:
        require(
            reuse.group(1),
            "installed_input_stamp",
            "build_llvm_octagram.sh must compare the installed stamp before reusing a prefix",
        )
        require(
            reuse.group(1),
            'rm -rf "$install_prefix" "$cmake_build"',
            "a prefix whose stamp does not match must be discarded with its build tree",
        )

    install_check = step_block(llvm, "Verify the restored libLLVM inputs")
    if not install_check:
        fail("llvm job does not verify a restored tree against its build-input stamp")
    else:
        require(
            install_check,
            "--check-install-inputs",
            "the restored libLLVM tree must be checked with --check-install-inputs",
        )
        require(
            install_check,
            'rm -rf "$prefix"',
            "a restored libLLVM tree whose stamp does not match must be deleted",
        )
        require(install_check, "usable=true", "the install check must publish usable=true")
        require(install_check, "usable=false", "the install check must publish usable=false")
    check_at = llvm.find("name: Verify the restored libLLVM inputs")
    build_at = llvm.find("name: Build libLLVM")
    save_at = llvm.find("name: Save libLLVM install tree")
    if check_at < 0 or build_at < 0:
        fail("llvm job has no install-input check or no build step")
    else:
        if check_at > build_at:
            fail("the install-input stamp is checked after the build starts")
        if save_at >= 0 and check_at > save_at:
            fail("the install tree is saved before its build-input stamp is checked")
    for step_name in ("Build libLLVM", "Save libLLVM install tree"):
        require(
            step_block(llvm, step_name),
            "steps.llvm-install.outputs.usable != 'true'",
            f"'{step_name}' must only run for an install tree this run can trust",
        )

    # --- behaviour that does not need a build -------------------------------
    cases = [
        (BUILD, "build_llvm_octagram.sh", ["--help"], 0, "--time-budget"),
        (BUILD, "build_llvm_octagram.sh", ["--status"], 1, "no build state"),
        (BUILD, "build_llvm_octagram.sh", ["--time-budget", "soon"], 2, "whole minutes"),
        (BUILD, "build_llvm_octagram.sh", ["--clean", "--resume"], 2, "contradict"),
        (BOOTSTRAP, "bootstrap_rustc_octagram.sh", ["--help"], 0, "--time-budget"),
        (BOOTSTRAP, "bootstrap_rustc_octagram.sh", ["--status"], 1, "no build state"),
        (BOOTSTRAP, "bootstrap_rustc_octagram.sh", ["--time-budget", "soon"], 2, "whole minutes"),
        (BOOTSTRAP, "bootstrap_rustc_octagram.sh", ["--clean", "--resume"], 2, "contradict"),
    ]
    # Point both scripts at an empty build root: a developer machine that has
    # already run a build must not change what these cases observe.
    with tempfile.TemporaryDirectory() as empty_build:
        environment = dict(os.environ, OCTAGRAM_TOOLCHAIN_BUILD=empty_build)
        for script, label, arguments, expected_status, expected_text in cases:
            result = subprocess.run(
                ["sh", str(script), *arguments],
                cwd=REPO,
                capture_output=True,
                text=True,
                check=False,
                env=environment,
            )
            output = result.stdout + result.stderr
            if result.returncode != expected_status:
                fail(
                    f"{label} {' '.join(arguments)} exited "
                    f"{result.returncode}, expected {expected_status}"
                )
            if expected_text not in output:
                fail(
                    f"{label} {' '.join(arguments)} did not mention "
                    f"'{expected_text}'"
                )

    # The cache fingerprint is content-based: identical sysroots must
    # produce the same key, while a sysroot content change must produce a new one.
    with tempfile.TemporaryDirectory() as temp_root:
        temp = Path(temp_root)
        sysroot = temp / "sysroot"
        library = sysroot / "usr" / "lib" / "libc++.a"
        library.parent.mkdir(parents=True)
        library.write_bytes(b"fixture-libcxx-v1")
        fingerprint_env = dict(os.environ, OCTAGRAM_SYSROOT=str(sysroot))
        # Keep the default build root so the relative cache-restore prefix
        # resolves to the same install directory as the build's absolute path.
        fingerprint_env.pop("OCTAGRAM_TOOLCHAIN_BUILD", None)

        def fingerprint(*extra_args: str) -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                [
                    "sh", str(BUILD), "--print-build-inputs-stamp",
                    "--llvm-version", "22.1.8", *extra_args,
                ],
                cwd=REPO,
                capture_output=True,
                text=True,
                check=False,
                env=fingerprint_env,
            )

        first = fingerprint()
        repeated = fingerprint()
        relative_prefix = fingerprint("--prefix", "build/toolchain/llvm-22.1.8-octagram")
        if first.returncode != 0 or not re.fullmatch(r"[0-9a-f]{64}", first.stdout.strip()):
            fail(
                f"LLVM input fingerprint command failed (status {first.returncode}): "
                f"{first.stdout}{first.stderr}"
            )
        if repeated.returncode != 0 or repeated.stdout != first.stdout:
            fail("LLVM input fingerprint is not deterministic for an unchanged sysroot")
        if relative_prefix.returncode != 0 or relative_prefix.stdout != first.stdout:
            fail("relative --prefix produced a different LLVM cache fingerprint")
        library.write_bytes(b"fixture-libcxx-v2")
        changed = fingerprint()
        if changed.returncode != 0 or changed.stdout == first.stdout:
            fail("LLVM input fingerprint did not change when sysroot contents changed")

    # The getrandom patcher is tested with a synthetic registry package so
    # its source edits and resume/idempotency behaviour are checked without
    # network access or a Rust rebuild.
    with tempfile.TemporaryDirectory() as temp_root:
        registry_root = Path(temp_root) / "registry" / "src"
        package = registry_root / "index.test" / "getrandom-0.3.3"
        (package / "src" / "backends").mkdir(parents=True)
        (package / "Cargo.toml").write_text(
            "[package]\n"
            'name = "getrandom"\n'
            'version = "0.3.3"\n'
            "\n"
            "[lints.rust.unexpected_cfgs]\n"
            'level = "warn"\n',
            encoding="utf-8",
        )
        (package / "src" / "backends.rs").write_text(
            "cfg_if! {\n"
            "    } else if #[cfg(any(\n"
            '        target_os = "android",\n'
            '        target_os = "linux",\n'
            '        target_os = "dragonfly",\n',
            encoding="utf-8",
        )
        (package / "src" / "backends" / "getrandom.rs").write_text(
            "pub fn fill_inner() {}\n", encoding="utf-8"
        )
        patch_runs = []
        for _ in range(2):
            patch_runs.append(
                subprocess.run(
                    [sys.executable, str(GETRANDOM_PATCH), "--registry-root", str(registry_root)],
                    cwd=REPO,
                    capture_output=True,
                    text=True,
                    check=False,
                )
            )
        if any(result.returncode != 0 for result in patch_runs):
            fail(
                "getrandom patcher fixture failed: "
                + " | ".join(result.stdout + result.stderr for result in patch_runs)
            )
        patched_manifest = (package / "Cargo.toml").read_text(encoding="utf-8")
        patched_backends = (package / "src" / "backends.rs").read_text(encoding="utf-8")
        if patched_manifest.count('target_os = "octagram"') != 1:
            fail("getrandom patcher did not add exactly one OCTAGRAM libc dependency")
        if patched_backends.count("mod getrandom;") != 1:
            fail("getrandom patcher did not add exactly one OCTAGRAM backend selection")

    if failures:
        print(f"OCTAGRAM-LLVM-CI-FAIL ({len(failures)})")
        return 1
    print(
        "OCTAGRAM-LLVM-CI-PASS: build jobs stay below the six-hour cap, "
        "escalate budget stops, quiesce external cancellations, restore "
        "checkpoints by exact-input prefix, save cancelled checkpoints, and "
        "cache only verified installs whose input fingerprint matches"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
