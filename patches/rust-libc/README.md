# Upstream Rust libc: OCTAGRAM ABI integration

Pinned upstream repository: https://github.com/rust-lang/libc
Pinned base commit: `a03dd7c2e298b0ca482f30ce07efd3115938328e`

## Files

- `octagram/mod.rs` is the target-specific Rust ABI module copied into
  upstream `src/unix/octagram/mod.rs`.
- `tools/apply_rust_libc_octagram.py` applies all source edits using exact,
  uniqueness-checked anchors and verifies the pinned upstream commit.
- Rust no_std CI runs that applicator and then compiles upstream libc for the
  OCTAGRAM custom target with `-Zbuild-std=core`.

## Integration details

The integration registers `target_os = "octagram"`, selects the dedicated
Unix target module, exports the common POSIX `unistd` constants, and adds
OCTAGRAM scalar aliases, public `stat` and `dirent` layouts, pthread object
layouts, constants, and clock/pthread declarations.

The target module follows OCTAGRAM public C headers. The public C `struct
stat` is separate from the kernel's 16-byte `struct oct_stat`. Generic
upstream declarations for pthread rwlocks, `statvfs`, and `dladdr` are
excluded because OCTAGRAM does not currently expose those interfaces.

## Validation scope and limitations

The CI check verifies that the exact upstream source edits apply and that the
upstream `libc` crate compiles for `x86_64-unknown-octagram`. It does not yet
prove that Rust `std` builds or that every declared C symbol is implemented
and linked by OCTAGRAM.

This is an initial ABI surface, not full POSIX coverage. Signal/process
semantics, sockets, dynamic loading, and filesystem extensions still need
target-specific conformance tests. No claim is made that upstream `std`,
`rustc`, or Cargo is already native.
