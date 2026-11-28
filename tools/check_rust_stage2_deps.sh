#!/bin/sh
# Compile target-side Rust dependency slices before the expensive LLVM build.
# Rust and Cargo have separate lockfiles, so use separate Cargo workspaces: Cargo
# otherwise unifies semver-compatible exact versions such as getrandom 0.3.3/0.3.4.
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
mkdir -p "$check_root"

write_dependency_project() {
    project=$1
    project_name=$2
    libc_version=$3
    getrandom_version=$4
    tempfile_version=$5
    filetime_version=$6
    socket2_version=$7
    mkdir -p "$project/src"

    cat > "$project/Cargo.toml" <<EOF
[package]
name = "$project_name"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
errno = { version = "=0.3.14", default-features = false }
getrandom = { version = "=$getrandom_version", default-features = false }
libc = { version = "=$libc_version", default-features = false }
filetime = { version = "=$filetime_version" }
socket2 = { version = "=$socket2_version" }
tempfile = { version = "=$tempfile_version" }

# This negative cfg dependency is a direct compile-time probe: if Cargo fails
# to recognize target_os="octagram" from the custom JSON spec, rustix is built
# and its unsupported libc surface fails before any LLVM work begins.
[target.'cfg(not(target_os = "octagram"))'.dependencies]
rustix-cfg-probe = { package = "rustix", version = "=1.1.6", default-features = false, features = ["fs"] }
EOF

    cat > "$project/src/lib.rs" <<'EOF'
#![no_std]

pub fn check_errno_surface() -> i32 {
    errno::errno().0
}

pub fn check_random_surface(buffer: &mut [u8]) -> Result<(), getrandom::Error> {
    getrandom::fill(buffer)
}

// Compile-check the POSIX surface consumed by errno and Rust std's Unix PAL.
pub unsafe fn check_octagram_file_abi(
    path: *const libc::c_char,
    other_path: *const libc::c_char,
    fd: libc::c_int,
    directory_fd: libc::c_int,
) {
    let _ = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    let _ = unsafe { libc::flock(fd, libc::LOCK_UN) };
    let _ = unsafe { libc::unlink(path) };
    let _ = unsafe { libc::unlinkat(directory_fd, path, 0) };
    let _ = unsafe { libc::link(path, other_path) };
    let _ = unsafe { libc::linkat(directory_fd, path, directory_fd, other_path, 0) };
    let _ = unsafe { libc::rename(path, other_path) };
    let _ = unsafe { libc::renameat(directory_fd, path, directory_fd, other_path) };
    let _ = unsafe { libc::openat(directory_fd, path, libc::O_RDONLY) };
    let directory = unsafe { libc::fdopendir(fd) };
    let _ = unsafe { libc::readdir(directory) };
    let _ = unsafe { libc::closedir(directory) };
    let _ = unsafe { libc::opendir(path) };
    let _ = unsafe { libc::strerror_r(0, core::ptr::null_mut(), 0) };
}
EOF
}

# Rust root lockfile: libc 0.2.191, getrandom 0.3.3, tempfile 3.23.0.
write_dependency_project \
    "$check_root/rust-workspace" \
    octagram-rust-workspace-dependency-check \
    0.2.191 0.3.3 3.23.0 0.2.27 0.6.5

# Cargo submodule lockfile: libc 0.2.186, getrandom 0.3.4, tempfile 3.27.0.
write_dependency_project \
    "$check_root/cargo-submodule" \
    octagram-cargo-submodule-dependency-check \
    0.2.186 0.3.4 3.27.0 0.2.29 0.6.4

# getrandom 0.4.3 is also present in the two lockfiles. Test it separately to
# avoid Cargo unifying semver-compatible exact versions inside one workspace.
mkdir -p "$check_root/getrandom-modern/src"
cat > "$check_root/getrandom-modern/Cargo.toml" <<'EOF'
[package]
name = "octagram-getrandom-modern-dependency-check"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
getrandom = { version = "=0.4.3", default-features = false }
EOF
cat > "$check_root/getrandom-modern/src/lib.rs" <<'EOF'
#![no_std]

pub fn check_random_surface(buffer: &mut [u8]) -> Result<(), getrandom::Error> {
    getrandom::fill(buffer)
}
EOF

echo "fetching separate Rust-workspace, Cargo-submodule, and getrandom 0.4.3 dependency graphs"
for manifest in \
    "$check_root/rust-workspace/Cargo.toml" \
    "$check_root/cargo-submodule/Cargo.toml" \
    "$check_root/getrandom-modern/Cargo.toml"; do
    CARGO_HOME=$cargo_home cargo fetch --manifest-path "$manifest"
done

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

for version in 0.2.191 0.2.186; do extract_crate libc "$version"; done
extract_crate errno 0.3.14
for version in 0.3.3 0.3.4 0.4.3; do extract_crate getrandom "$version"; done
for version in 3.23.0 3.27.0; do extract_crate tempfile "$version"; done
for version in 0.2.27 0.2.29; do extract_crate filetime "$version"; done
for version in 0.6.5 0.6.4; do extract_crate socket2 "$version"; done

for libc_version in 0.2.191 0.2.186; do
    for source in "$cargo_home"/registry/src/*/libc-"$libc_version"; do
        [ -f "$source/Cargo.toml" ] || continue
        python3 "$repo_root/tools/apply_rust_libc_octagram.py" "$source" --allow-unpinned
    done
done
python3 "$repo_root/tools/patch_errno_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_getrandom_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_tempfile_octagram.py" --registry-root "$cargo_home/registry/src"
python3 "$repo_root/tools/patch_filetime_octagram.py" --registry-root "$cargo_home/registry/src"

# The preflight includes std-based crates (tempfile, filetime, socket2), not
# just no_std crates. Build the patched target std from the installed rust-src
# component so missing std APIs and libc bindings fail here, before LLVM.
rust_sysroot=$(rustc "+$toolchain" --print sysroot)
rust_library=$rust_sysroot/lib/rustlib/src/rust/library
if [ ! -f "$rust_library/std/src/os/unix/mod.rs" ]; then
    echo "Rust nightly rust-src is missing the library/std sources: $rust_library" >&2
    exit 1
fi
OCTAGRAM_RUST_STD_ROOT="$rust_library/std/src" \
OCTAGRAM_RUST_TOOLCHAIN="$toolchain" \
    python3 "$repo_root/tools/patch_rust_std_octagram.py"

echo "OCTAGRAM custom-target cfg:"
rustc "+$toolchain" -Z unstable-options --print cfg --target "$target" | sort

echo "patched tempfile rustix manifest selectors:"
for manifest in "$cargo_home"/registry/src/*/tempfile-3.23.0/Cargo.toml \
                "$cargo_home"/registry/src/*/tempfile-3.27.0/Cargo.toml; do
    [ -f "$manifest" ] || continue
    echo "--- $manifest"
    grep -A4 -B1 'dependencies.rustix' "$manifest" || true
done

check_target_graph() {
    project=$1
    expected_tempfile=$2
    label=$3
    echo "checking $label target dependency graph"
    dependency_tree=$(CARGO_HOME=$cargo_home cargo "+$toolchain" tree \
        --manifest-path "$project/Cargo.toml" \
        --target "$target" -Zjson-target-spec --edges normal --prefix none)
    if printf '%s\n' "$dependency_tree" | grep -Eq '^rustix v'; then
        # cargo tree may display target-specific nodes even when the selected
        # custom JSON target disables them. The compile step below is the
        # authority: rustix-cfg-probe is excluded only if target_os is honored.
        echo "NOTE: cargo tree reports rustix for $label; cargo check will verify target cfg before compilation"
    fi
    if [ -n "$expected_tempfile" ] &&
       ! printf '%s\n' "$dependency_tree" | grep -Eq "^tempfile v$expected_tempfile"; then
        echo "tempfile $expected_tempfile is missing from the OCTAGRAM target graph ($label)" >&2
        printf '%s\n' "$dependency_tree" >&2
        exit 1
    fi
}

check_and_compile() {
    project=$1
    expected_tempfile=$2
    label=$3
    check_target_graph "$project" "$expected_tempfile" "$label"
    echo "compiling $label dependencies for x86_64-unknown-octagram"
    CARGO_HOME=$cargo_home CARGO_TARGET_DIR=$check_root/target-octagram \
        cargo "+$toolchain" check \
        --manifest-path "$project/Cargo.toml" \
        --target "$target" \
        -Zjson-target-spec \
        -Zbuild-std=core,alloc,std
}

check_and_compile "$check_root/rust-workspace" 3.23.0 rust-workspace
check_and_compile "$check_root/cargo-submodule" 3.27.0 cargo-submodule
check_and_compile "$check_root/getrandom-modern" "" getrandom-modern

echo "OCTAGRAM-RUST-STAGE2-DEPENDENCY-CHECK-PASS"
