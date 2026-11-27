#!/bin/sh
# Compile target-side Rust dependencies that already caused bootstrap failures
# before spending time on the LLVM and stage-2 compiler builds.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
toolchain=${OCTAGRAM_RUST_TOOLCHAIN:-nightly-2026-11-23}
target=$repo_root/user/rust/targets/x86_64-unknown-octagram.json
cargo_home=${CARGO_HOME:-$HOME/.cargo}
check_root=${RUNNER_TEMP:-/tmp}/octagram-stage2-dependency-check

[ -f "$target" ] || { echo "missing OCTAGRAM target spec: $target" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo "cargo is not installed" >&2; exit 1; }

rm -rf "$check_root"
mkdir -p "$check_root/src"
cat > "$check_root/Cargo.toml" <<EOF
[package]
name = "octagram-stage2-dependency-check"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
errno = { version = "=0.3.14", default-features = false }
getrandom = { version = "=0.3.3", default-features = false }
tempfile = { version = "=3.23.0" }
libc = { version = "=0.2.191", default-features = false }
EOF
cat > "$check_root/src/lib.rs" <<'EOF'
#![no_std]

pub fn check_errno_surface() -> i32 {
    errno::errno().0
}

pub fn check_random_surface(buffer: &mut [u8]) -> Result<(), getrandom::Error> {
    getrandom::fill(buffer)
}
EOF

echo "fetching errno 0.3.14, getrandom 0.3.3, tempfile 3.23.0, and libc 0.2.191"
CARGO_HOME=$cargo_home cargo fetch --manifest-path "$check_root/Cargo.toml"

extract_crate() {
    package=$1
    version=$2
    found=0
    for archive in "$cargo_home"/registry/cache/*/"$package-$version.crate"; do
        [ -f "$archive" ] || continue
        registry_id=$(basename "$(dirname "$archive")")
        source_root=$cargo_home/registry/src/$registry_id
        source=$source_root/$package-$version
        if [ ! -f "$source/Cargo.toml" ]; then
            mkdir -p "$source_root"
            tar -xzf "$archive" -C "$source_root"
        fi
        [ -f "$source/Cargo.toml" ] || {
            echo "failed to extract $package $version from $archive" >&2
            return 1
        }
        found=1
    done
    [ "$found" -eq 1 ] || {
        echo "Cargo cache is missing $package-$version.crate" >&2
        return 1
    }
}

extract_crate libc 0.2.191
extract_crate errno 0.3.14
extract_crate getrandom 0.3.3
extract_crate tempfile 3.23.0

for source in "$cargo_home"/registry/src/*/libc-0.2.191; do
    [ -f "$source/Cargo.toml" ] || continue
    python3 "$repo_root/tools/apply_rust_libc_octagram.py" "$source" --allow-unpinned
done
python3 "$repo_root/tools/patch_errno_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_getrandom_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_tempfile_octagram.py" --registry-root "$cargo_home/registry/src"

echo "checking patched target dependencies for x86_64-unknown-octagram"
CARGO_HOME=$cargo_home CARGO_TARGET_DIR=$check_root/target \
    cargo "+$toolchain" check \
    --manifest-path "$check_root/Cargo.toml" \
    --target "$target" \
    -Zjson-target-spec \
    -Zbuild-std=core

echo "OCTAGRAM-RUST-STAGE2-DEPENDENCY-CHECK-PASS"
