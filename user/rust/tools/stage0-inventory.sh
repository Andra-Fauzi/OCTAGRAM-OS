#!/bin/sh
# Audit the host-side ingredients required before attempting a native Stage 0.
# This script does not build or claim to provide a native OCTAGRAM rustc/Cargo.
set -eu

cd "$(dirname "$0")/.."
TARGET_JSON="targets/x86_64-unknown-octagram.json"
failures=0

section() {
    printf '\n== %s ==\n' "$1"
}

require_command() {
    if command -v "$1" >/dev/null 2>&1; then
        printf 'FOUND   %s: %s\n' "$1" "$(command -v "$1")"
    else
        printf 'MISSING %s\n' "$1"
        failures=$((failures + 1))
    fi
}

section "Host tools"
require_command rustup
require_command rustc
require_command cargo
require_command ld.lld
require_command readelf

section "Rust compiler"
if command -v rustc >/dev/null 2>&1; then
    rustc +nightly -vV
    printf '\nCargo: '
    cargo +nightly -V
    printf 'Sysroot: '
    SYSROOT="$(rustc +nightly --print sysroot)"
    printf '%s\n' "$SYSROOT"

    if [ -d "$SYSROOT/lib/rustlib/src/rust/library" ]; then
        printf 'FOUND   rust-src: %s/lib/rustlib/src/rust/library\n' "$SYSROOT"
    else
        printf 'MISSING rust-src source tree in nightly sysroot\n'
        failures=$((failures + 1))
    fi

    printf '\nInstalled nightly components:\n'
    rustup component list --toolchain nightly --installed || {
        printf 'WARN    unable to list rustup components\n'
    }
fi

section "LLVM linker"
if command -v ld.lld >/dev/null 2>&1; then
    ld.lld --version
fi

section "OCTAGRAM target specification"
if [ -f "$TARGET_JSON" ]; then
    printf 'FOUND   %s\n' "$TARGET_JSON"
    # json-target-spec is nightly-only; this checks that rustc can parse the
    # custom target before spending time compiling core/alloc.
    rustc +nightly -Z unstable-options \
        --print cfg --target "$TARGET_JSON" >/dev/null
    printf 'PASS    rustc accepted the OCTAGRAM target JSON\n'
else
    printf 'MISSING %s\n' "$TARGET_JSON"
    failures=$((failures + 1))
fi

section "Stage 0 boundary"
printf '%s\n' \
    "This is a host inventory, not a guest execution test." \
    "It does not prove that rustc, LLVM, or Cargo can run on OCTAGRAM." \
    "Native bootstrap remains blocked until a guest-executable compiler and" \
    "its runtime dependencies are built and tested in QEMU."

if [ "$failures" -ne 0 ]; then
    printf '\nFAIL    %s required inventory check(s) failed\n' "$failures"
    exit 1
fi

printf '\nPASS    Stage 0 host inventory completed\n'
