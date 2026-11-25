# Upstream Rust patch series for OCTAGRAM

This directory contains small, reviewable patches intended for a pinned
upstream Rust checkout. It is not a vendored Rust source tree and does not
claim that the patches have already been applied or that `std` builds.

## Base revision

Initial audit base: `rust-lang/rust`
`342d74463af7dd3018ef6aa04dd5500c2deb5bd1` (upstream `main` as inspected
on 22 November 2026).

Always check the patch against the exact checkout before applying:

```sh
git -C /path/to/rust rev-parse HEAD
git -C /path/to/rust apply --check /path/to/OCTAGRAM-OS/patches/rust/0001-std-recognize-octagram-target.patch
git -C /path/to/rust apply /path/to/OCTAGRAM-OS/patches/rust/0001-std-recognize-octagram-target.patch
```

## Patch 0001

Adds `target_os == "octagram"` to the supported-OS allowlist in
`library/std/build.rs`. The custom target already declares
`target_family = ["unix"]`, so upstream `library/std/src/sys/pal/mod.rs`
selects the generic Unix PAL branch.

**This is only the build gate.** It does not provide OCTAGRAM's libc crate
target definitions, errno/constants ABI, Unix PAL syscall semantics, startup
integration, or prove that `std` compiles. Generic Unix PAL code often
requires libc APIs that OCTAGRAM does not yet expose. The next implementation
gate is a pinned `libc` target port and a minimal `std` build attempt that
reports missing symbols/modules without weakening unrelated targets.

Do not mark `rust_std`, `rustc_host`, or `cargo_host` as implemented from
this patch alone.

## OCTAGRAM host target (applied by tool, not by diff)

`octagram/x86_64_unknown_octagram.rs` is the built-in target definition for the
compiler host triple `x86_64-unknown-octagram`. Bootstrap cannot build a
compiler for a triple that only exists as a JSON spec, so the triple has to be
known to `rustc_target` itself.

`tools/apply_rust_octagram_host_target.py` installs the file and makes the two
registrations in `compiler/rustc_target/src/spec/mod.rs`:

- `Octagram = "octagram"` in the `Os` enum (so `target_os = "octagram"` and
the std build gate agree), and
- `("x86_64-unknown-octagram", x86_64_unknown_octagram)` in the
`supported_targets!` list.

A plain `.patch` is not used: the three edits are small, and the applicator can
assert exact anchors, verify the pinned commit, and stay idempotent. Keep the
definition in sync with `user/rust/targets/x86_64-unknown-octagram.json`
(the JSON spec used for cross builds from Linux).

`tools/check_octagram_rust_target_sync.py` memverifikasi kedua spec tetap
konsisten (tanpa rustc); jalankan setiap kali salah satu berubah.

This is a target definition only. It does not provide libc symbols, std PAL
implementations, a linker, or a working `rustc` binary; see
[docs/rustc-native-bootstrap-runbook.md](../../docs/rustc-native-bootstrap-runbook.md).

## Automated patch validation

The `Rust upstream patch validation` GitHub Actions workflow checks this patch
against the exact upstream Rust commit listed above. It performs both
`git apply --check` and an application followed by a source assertion. This
only validates patch context and application; it does not build Rust
`std`, prove PAL compatibility, or validate the target's runtime behavior.
