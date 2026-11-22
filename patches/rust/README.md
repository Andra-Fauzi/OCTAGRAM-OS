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
