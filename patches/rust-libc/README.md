# Upstream Rust libc: OCTAGRAM ABI integration

Pinned upstream repository: https://github.com/rust-lang/libc
Pinned release: `0.2.191`
Pinned commit: `ccc2ff4aaab962f7fde7b8edda7379c045b1ca12`

This is the exact `libc` version required by the pinned Rust source's
`library/std/Cargo.toml`. Do not substitute upstream `main` or the
`1.0.0-alpha` development line for the standard-library dependency.

## Files

- `octagram/mod.rs` is the target-specific Rust ABI module copied into
  upstream `src/unix/octagram/mod.rs`.
- `tools/apply_rust_libc_octagram.py` applies source edits using exact,
  uniqueness-checked anchors and verifies the pinned upstream commit.
- Rust no_std CI checks out this exact release, runs the applicator, and
  compiles the dependency for the OCTAGRAM custom target.

## Integration details

The integration registers `target_os = "octagram"`, selects a dedicated
Unix target module, exports the common POSIX `unistd` constants, and adds
OCTAGRAM scalar aliases, public `stat` and `dirent` layouts, pthread object
layouts, constants, and clock/pthread declarations.

The target module follows OCTAGRAM public C headers. The public C `struct
stat` is separate from the kernel's 16-byte `struct oct_stat`. Generic
upstream declarations for pthread rwlocks, `statvfs`, and `dladdr` are
excluded because OCTAGRAM does not currently expose those interfaces.

## Validation scope and limitations

CI applies the target module to the exact `0.2.191` release and compiles the
crate for `x86_64-unknown-octagram` with nightly and `-Zbuild-std=core`.
This verifies that the dependency version used by Rust `std` accepts the
initial target ABI module.

It does not yet prove that Rust `std` builds or that every declared C symbol
is implemented and linked by OCTAGRAM. Signal/process semantics, sockets,
dynamic loading, and filesystem extensions still need target-specific
conformance tests. No claim is made that upstream `std`, `rustc`, or Cargo
is already native.
