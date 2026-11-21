#!/usr/bin/env bash
# Build the release kernel and turn it into a bootable A/B disk image, so the
# QEMU tests can boot this checkout without an ISO writer.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${OCTAGRAM_XRUSTC_DISK:-/dev/shm/octagram-xrustc.img}"
if [[ ! -w "$(dirname "$OUT")" ]]; then
    OUT="/tmp/$(basename "$OUT")"
fi

make -C "$ROOT/kernel" KARCH=x86_64 RUST_PROFILE=release

OCTAGRAM_BOOT_IMAGE="$OUT" \
OCTAGRAM_KERNEL_ELF="$ROOT/kernel/target/x86_64-unknown-none/release/octagram-kernel" \
OCTAGRAM_BOOTLOADER="$ROOT/limine/BOOTX64.EFI" \
    cargo +nightly test \
        --manifest-path "$ROOT/kernel/Cargo.toml" \
        --target x86_64-unknown-linux-gnu \
        -- --ignored writes_a_boot_image_from_real_artifacts

echo "BOOT-IMAGE: $OUT"
