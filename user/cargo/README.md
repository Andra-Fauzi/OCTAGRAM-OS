# octagram-cargo — Cargo bootstrap driver

This is **not** upstream Cargo. Upstream Cargo is a Rust program; it can only be
built for `x86_64-unknown-octagram` after a native `rustc` and sysroot exist
(M4.3.4 → M4.4). What this program provides is the Cargo-side half of the M4.4
gate, written in C++ against the libc++ and libc that OCTAGRAM already runs, so
that the manifest/lockfile/planning/invocation logic is native and tested
before a compiler lands.

## What it does

| Command | Behaviour |
| --- | --- |
| `plan` | resolve workspace members and path dependencies, print build order, targets, and dependencies |
| `build` | resolve, fingerprint sources, invoke `rustc` per target with Cargo-compatible arguments |
| `check` | same resolution, `--emit=metadata` instead of an output file |
| `clean` | remove the target directory |
| `metadata` | print a minimal JSON description (packages, targets, source paths) |
| `version` | print the driver version |

Options: `--manifest-path`, `--target-dir`, `--profile`, `--release`,
`--rustc`, `--locked`, `--offline`, `-v`.

## Deliberate limits

- **Offline only.** A dependency must be a `path` dependency. Registry and
  `git` dependencies are rejected with an explicit message rather than being
  silently dropped; vendoring is the only supported model until a registry
  cache exists.
- **Manifest subset.** `[package]`, `[workspace] members`, `[lib]`, `[[bin]]`,
  `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]` with string or
  inline-table values. No features resolution, no target-specific tables, no
  build scripts, no proc macros, no profiles from `Cargo.toml`.
- **`--locked` validates.** It checks that `Cargo.lock` covers every resolved
  package; it does not rewrite the lockfile.
- **Fingerprints are mtime-based** (newest `.rs` in `src/` and `Cargo.toml`
  against the output). Cargo's own fingerprint hashing is not reproduced.
- **`rustc` is external.** The driver never links a compiler; it execs
  `$RUSTC`, `$OCTAGRAM_RUSTC`, or `rustc` from `PATH` (the shell searches
  `/toolchain/bin`, so a native compiler dropped there is picked up). The
  target triple comes from `$OCTAGRAM_RUST_TARGET`, defaulting to
  `x86_64-unknown-octagram`.

Because of those limits, `octagram-cargo build` is not evidence that native
Cargo works. The M4.4 gate still requires upstream Cargo running in the guest
and building an offline workspace with a native `rustc`.

## Build and run

```sh
make -C user build/octagram-cargo.elf
# in the guest
octagram-cargo --locked plan --manifest-path /cargo-demo/Cargo.toml
octagram-cargo metadata --manifest-path /cargo-demo/Cargo.toml
```

`user/cargo/demo/` is the tiny workspace the guest probe uses: one binary
package with a path dependency on `demo-lib`, plus a matching `Cargo.lock`.
It is never compiled in QEMU because no `rustc` exists in the image yet.

## Composability

The driver is the piece that will drive the native toolchain once M4.3.4
produces `rustc` + sysroot:

```sh
OCTAGRAM_RUSTC=/toolchain/bin/rustc \
OCTAGRAM_RUST_TARGET=x86_64-unknown-none \
octagram-cargo build --manifest-path /src/octagram/Cargo.toml --release
```
