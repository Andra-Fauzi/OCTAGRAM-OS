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
getrandom_root = { package = "getrandom", version = "=0.3.3", default-features = false }
getrandom_cargo = { package = "getrandom", version = "=0.3.4", default-features = false }
getrandom_modern = { package = "getrandom", version = "=0.4.3", default-features = false }
libc_root = { package = "libc", version = "=0.2.191", default-features = false }
libc_cargo = { package = "libc", version = "=0.2.186", default-features = false }

[dev-dependencies]
tempfile_root = { package = "tempfile", version = "=3.23.0" }
tempfile_cargo = { package = "tempfile", version = "=3.27.0" }
EOF
cat > "$check_root/src/lib.rs" <<'EOF'
#![no_std]

pub fn check_errno_surface() -> i32 {
    errno::errno().0
}

pub fn check_random_root(buffer: &mut [u8]) -> Result<(), getrandom_root::Error> {
    getrandom_root::fill(buffer)
}

pub fn check_random_cargo(buffer: &mut [u8]) -> Result<(), getrandom_cargo::Error> {
    getrandom_cargo::fill(buffer)
}

pub fn check_random_modern(buffer: &mut [u8]) -> Result<(), getrandom_modern::Error> {
    getrandom_modern::fill(buffer)
}

// Compile-check the complete libc surface used by errno and the Rust Unix PAL
// against both Rust workspace and Cargo submodule lockfile versions.
pub unsafe fn check_rust_workspace_file_abi(
    path: *const libc_root::c_char,
    other_path: *const libc_root::c_char,
    fd: libc_root::c_int,
    directory_fd: libc_root::c_int,
) {
    let _ = unsafe { libc_root::flock(fd, libc_root::LOCK_EX | libc_root::LOCK_NB) };
    let _ = unsafe { libc_root::flock(fd, libc_root::LOCK_UN) };
    let _ = unsafe { libc_root::unlink(path) };
    let _ = unsafe { libc_root::unlinkat(directory_fd, path, 0) };
    let _ = unsafe { libc_root::link(path, other_path) };
    let _ = unsafe { libc_root::linkat(directory_fd, path, directory_fd, other_path, 0) };
    let _ = unsafe { libc_root::rename(path, other_path) };
    let _ = unsafe { libc_root::renameat(directory_fd, path, directory_fd, other_path) };
    let _ = unsafe { libc_root::openat(directory_fd, path, libc_root::O_RDONLY) };
    let directory = unsafe { libc_root::fdopendir(fd) };
    let _ = unsafe { libc_root::readdir(directory) };
    let _ = unsafe { libc_root::closedir(directory) };
    let _ = unsafe { libc_root::opendir(path) };
    let _ = unsafe { libc_root::strerror_r(0, core::ptr::null_mut(), 0) };
}

pub unsafe fn check_cargo_file_abi(
    path: *const libc_cargo::c_char,
    other_path: *const libc_cargo::c_char,
    fd: libc_cargo::c_int,
    directory_fd: libc_cargo::c_int,
) {
    let _ = unsafe { libc_cargo::flock(fd, libc_cargo::LOCK_EX | libc_cargo::LOCK_NB) };
    let _ = unsafe { libc_cargo::flock(fd, libc_cargo::LOCK_UN) };
    let _ = unsafe { libc_cargo::unlink(path) };
    let _ = unsafe { libc_cargo::unlinkat(directory_fd, path, 0) };
    let _ = unsafe { libc_cargo::link(path, other_path) };
    let _ = unsafe { libc_cargo::linkat(directory_fd, path, directory_fd, other_path, 0) };
    let _ = unsafe { libc_cargo::rename(path, other_path) };
    let _ = unsafe { libc_cargo::renameat(directory_fd, path, directory_fd, other_path) };
    let _ = unsafe { libc_cargo::openat(directory_fd, path, libc_cargo::O_RDONLY) };
    let directory = unsafe { libc_cargo::fdopendir(fd) };
    let _ = unsafe { libc_cargo::readdir(directory) };
    let _ = unsafe { libc_cargo::closedir(directory) };
    let _ = unsafe { libc_cargo::opendir(path) };
    let _ = unsafe { libc_cargo::strerror_r(0, core::ptr::null_mut(), 0) };
}
EOF

echo "fetching errno, all three getrandom versions, both tempfile versions, and both libc versions"
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
extract_crate libc 0.2.186
extract_crate errno 0.3.14
extract_crate getrandom 0.3.3
extract_crate getrandom 0.3.4
extract_crate getrandom 0.4.3
extract_crate tempfile 3.23.0
extract_crate tempfile 3.27.0

for libc_version in 0.2.191 0.2.186; do
    for source in "$cargo_home"/registry/src/*/libc-"$libc_version"; do
        [ -f "$source/Cargo.toml" ] || continue
        python3 "$repo_root/tools/apply_rust_libc_octagram.py" "$source" --allow-unpinned
    done
done
python3 "$repo_root/tools/patch_errno_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_getrandom_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_tempfile_octagram.py" --registry-root "$cargo_home/registry/src"

echo "checking tempfile's OCTAGRAM dependency graph"
dependency_tree=$(CARGO_HOME=$cargo_home cargo "+$toolchain" tree \
    --manifest-path "$check_root/Cargo.toml" \
    --target "$target" -Zjson-target-spec --edges normal,dev --prefix none)
if printf '%s\n' "$dependency_tree" | grep -Eq '^rustix v'; then
    echo "unexpected rustix dependency remains in the OCTAGRAM target graph" >&2
    printf '%s\n' "$dependency_tree" >&2
    exit 1
fi
for tempfile_version in 3.23.0 3.27.0; do
    if ! printf '%s\n' "$dependency_tree" | grep -Eq "^tempfile v$tempfile_version"; then
        echo "tempfile $tempfile_version is missing from the OCTAGRAM target dependency graph" >&2
        exit 1
    fi
done

echo "checking patched target dependencies for x86_64-unknown-octagram"
CARGO_HOME=$cargo_home CARGO_TARGET_DIR=$check_root/target \
    cargo "+$toolchain" check \
    --manifest-path "$check_root/Cargo.toml" \
    --target "$target" \
    -Zjson-target-spec \
    -Zbuild-std=core

echo "OCTAGRAM-RUST-STAGE2-DEPENDENCY-CHECK-PASS"
