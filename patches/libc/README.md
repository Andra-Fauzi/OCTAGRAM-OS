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

## Current validation result

The patch applies cleanly to the pinned commit and the CI job cross-compiles the
upstream `libc` crate for OCTAGRAM with:

```sh
cargo +nightly check --lib \
  --target user/rust/targets/x86_64-unknown-octagram.json \
  --no-default-features -Zjson-target-spec -Zbuild-std=core
```

The compile-time layout assertions cover `stat`, `dirent`, pthread objects,
`passwd`, `sockaddr`, `addrinfo`, `tm`, `fd_set`, `lconv`, `sem_t`,
`termios`, and `sigaction`. The generic Unix bindings for pthread rwlocks,
`statvfs`, `dladdr`, and `sigaction()` are gated off because OCTAGRAM does
not currently expose those APIs.

This is a successful compile of the upstream `libc` crate in no-default-
features mode, not a build of Rust `std`. It does not establish runtime
compatibility for every declaration still exported by the generic Unix module.
