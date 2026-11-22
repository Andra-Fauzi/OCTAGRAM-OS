# Upstream Rust libc: OCTAGRAM ABI patch

Patch: `0001-add-octagram-abi.patch`

Pinned upstream repository: https://github.com/rust-lang/libc
Pinned base commit: `a03dd7c2e298b0ca482f30ce07efd3115938328e`

## What this patch does

- Adds a dedicated `src/unix/octagram` target module selected for
  `target_os = "octagram"`.
- Registers `octagram` as a recognized `target_os` value in the crate build
  script's check-cfg allowlist.
- Defines initial OCTAGRAM scalar aliases, public `stat` and `dirent`
  layouts, pthread object layouts, and constants based on OCTAGRAM headers.
- Declares the current C runtime pthread and clock entry points needed by
  consumers.

The module deliberately does not alias OCTAGRAM to Linux, Redox, or another
Unix platform. In particular, the public C `struct stat` is distinct from the
kernel's 16-byte `struct oct_stat`; the former is 88 bytes on x86_64 with the
field order declared in `user/include/sys/stat.h`.

## Validation

The Rust no_std CI checks out the exact upstream commit, runs `git apply
--check`, applies the patch, and runs `cargo check` for the custom target
without default features. This verifies the libc crate's initial target
module compiles; it does not prove that Rust `std` builds or that all declared
functions are implemented by the kernel.

## Known limitations

- This is an initial ABI surface, not full POSIX coverage.
- Some POSIX functions declared by generic libc code may not yet be provided
  by OCTAGRAM's runtime.
- Signal, process-status, socket, dynamic-loader, and filesystem semantics
  need further PAL-specific audit.
- The target module must be kept synchronized with
  `tools/rust_libc_abi_surface.json` and the public C headers.
- No claim is made that upstream `std`, `rustc`, or Cargo is already native.

