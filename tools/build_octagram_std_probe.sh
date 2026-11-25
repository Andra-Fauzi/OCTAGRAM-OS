#!/usr/bin/env bash
# build_octagram_std_probe.sh: build and link a std probe for x86_64-unknown-octagram.
#
# This is the end-to-end check for "std builds for OCTAGRAM": it compiles std
# from rust-src with -Z build-std, links against the guest sysroot built by
# `make -C user sysroot`, and reports the resulting ELF.
#
# Prerequisites (the script stops with a hint if one is missing):
#   * a nightly toolchain with the rust-src component;
#   * tools/patch_rust_std_octagram.py applied to that rust-src;
#   * tools/apply_rust_libc_octagram.py applied to the libc crate cargo uses;
#   * user/build/libc.a, user/build/crt0.o and user/build/sysroot-octagram.
#
# Usage: tools/build_octagram_std_probe.sh [--check-only]

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
check_only=0
if [[ ${1:-} == "--check-only" ]]; then
    check_only=1
fi

export PATH="$HOME/.cargo/bin:$PATH"

toolchain="${OCTAGRAM_RUST_TOOLCHAIN:-nightly}"
sysroot=$(rustc "+$toolchain" --print sysroot)
rust_src="$sysroot/lib/rustlib/src/rust"
target_json="$repo_root/user/rust/targets/x86_64-unknown-octagram.json"
toolchain_bin="$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin"

if [[ ! -f "$rust_src/library/std/src/os/octagram/mod.rs" ]]; then
    echo "build_octagram_std_probe: run tools/patch_rust_std_octagram.py first" >&2
    exit 1
fi

libc_src=$(ls -d "$HOME"/.cargo/registry/src/*/libc-0.2.191 2>/dev/null | head -1 || true)
if [[ -z $libc_src || ! -f "$libc_src/src/unix/octagram/mod.rs" ]]; then
    echo "build_octagram_std_probe: run tools/apply_rust_libc_octagram.py on libc-0.2.191 first" >&2
    exit 1
fi

if [[ ! -f "$repo_root/user/build/libc.a" || ! -f "$repo_root/user/build/crt0.o" ]]; then
    echo "build_octagram_std_probe: run 'make -C user sysroot' first" >&2
    exit 1
fi

# The guest folds libm/libpthread/librt into libc, but rustc asks for them by
# name, so expose them as aliases in a scratch directory.
libdir=$(mktemp -d)
trap 'rm -rf "$libdir"' EXIT
cp "$repo_root/user/build/libc.a" "$libdir/libc.a"
for alias in m rt pthread; do
    ln -s libc.a "$libdir/lib$alias.a"
done

# .cargo/config.toml already carries the octagram flags, but its linker script
# and crt0 paths are relative to the `user` tree. CARGO_ENCODED_RUSTFLAGS is the
# only setting that replaces the configured flags instead of adding to them.
sep=$'\x1f'
encoded="-C${sep}relocation-model=static"
encoded+="${sep}-C${sep}no-redzone=yes"
encoded+="${sep}-C${sep}target-feature=+sse,+sse2"
encoded+="${sep}-C${sep}link-arg=-T$repo_root/user/linker.ld"
encoded+="${sep}-C${sep}link-arg=$repo_root/user/build/crt0.o"
encoded+="${sep}-C${sep}link-arg=-L$libdir"
encoded+="${sep}-C${sep}link-arg=-u${sep}-C${sep}link-arg=__oct_tls_initialize"

export CARGO_TARGET_X86_64_UNKNOWN_OCTAGRAM_LINKER=rust-lld
export CARGO_ENCODED_RUSTFLAGS="$encoded"
export PATH="$toolchain_bin:$PATH"

cd "$repo_root/user/rust"

if [[ $check_only -eq 1 ]]; then
    cargo "+$toolchain" -Z unstable-options -Z build-std=std -Z json-target-spec check \
        --manifest-path std-probe/Cargo.toml --target "$target_json"
    echo "build_octagram_std_probe: std type-checks for x86_64-unknown-octagram"
    exit 0
fi

cargo "+$toolchain" -Z unstable-options -Z build-std=std -Z json-target-spec build \
    --manifest-path std-probe/Cargo.toml --target "$target_json"

probe="$repo_root/user/rust/std-probe/target/x86_64-unknown-octagram/debug/octagram-std-probe"
if [[ ! -f $probe ]]; then
    echo "build_octagram_std_probe: expected $probe" >&2
    exit 1
fi

echo "build_octagram_std_probe: built $probe"
file "$probe" || true
