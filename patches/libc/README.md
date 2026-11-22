# Upstream libc patch series for OCTAGRAM

Initial audit base: `rust-lang/libc`
`a03dd7c2e298b0ca482f30ce07efd3115938328e` (upstream main, 22 November 2026).

## Patch 0001

Adds an OCTAGRAM-specific Unix module and routes `target_os = "octagram"`
to it. The first ABI slice describes the scalar aliases, public `struct stat`,
`struct dirent`, pthread object layouts, and basic file/memory/time constants.

The definitions are based on OCTAGRAM's own public headers, not Linux or
Redox. In particular, the kernel's 16-byte `oct_stat` is not the public
`struct stat`; the module models the public C `struct stat` translated by
the runtime.

This patch is an initial port slice only. It does not yet implement all
platform-specific libc structures, constants, or the full set of symbols
needed by the generic Unix PAL. It has not yet been applied or compiled against
the pinned upstream checkout. CI validation and a `cargo check` are the next
gates. Do not claim Rust `std` works from this patch.
