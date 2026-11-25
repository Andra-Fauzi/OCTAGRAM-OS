#!/bin/sh
# stage_octagram_toolchain.sh: collect a bootstrapped rustc/cargo and verify it.
#
# Bootstrap output is easy to install by mistake: a stage2 tree can exist
# without a host `std`, or the binaries can still be host ELFs. This script
# copies only what the guest needs and refuses the common failure cases before
# the tree reaches an image:
#
#   bin/rustc, bin/cargo        must be ELF executables
#   lib/rustlib/                must contain a host std rlib
#   toolchain.json              manifest with what was validated
#
# Usage:
#   tools/stage_octagram_toolchain.sh --source DIR --output DIR \
#       [--host TRIPLE] [--target TRIPLE] [--rust-commit SHA] [--llvm-version VER]

set -eu

source_dir=
output_dir=
host=x86_64-unknown-octagram
target=x86_64-unknown-none
rust_commit=unknown
llvm_version=unknown

while [ $# -gt 0 ]; do
    case $1 in
        --source) source_dir=$2; shift 2 ;;
        --output) output_dir=$2; shift 2 ;;
        --host) host=$2; shift 2 ;;
        --target) target=$2; shift 2 ;;
        --rust-commit) rust_commit=$2; shift 2 ;;
        --llvm-version) llvm_version=$2; shift 2 ;;
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "stage_octagram_toolchain: unknown argument '$1'" >&2; exit 2 ;;
    esac
done

if [ -z "$source_dir" ] || [ -z "$output_dir" ]; then
    echo "stage_octagram_toolchain: --source and --output are required" >&2
    exit 2
fi
if [ ! -f "$source_dir/bin/rustc" ]; then
    echo "stage_octagram_toolchain: error: no rustc in $source_dir/bin" >&2
    echo "stage_octagram_toolchain: did the bootstrap build fail?" >&2
    exit 1
fi

require_elf() {
    magic=$(head -c 4 "$1" | od -An -tx1 | tr -d ' \n')
    if [ "$magic" != "7f454c46" ]; then
        echo "stage_octagram_toolchain: error: $1 is not an ELF executable" >&2
        echo "stage_octagram_toolchain: a host rustc was staged instead of the target build" >&2
        exit 1
    fi
}

rm -rf "$output_dir"
mkdir -p "$output_dir/bin" "$output_dir/lib"

cp "$source_dir/bin/rustc" "$output_dir/bin/rustc"
chmod 0755 "$output_dir/bin/rustc"
require_elf "$output_dir/bin/rustc"

cargo_present=0
if [ -f "$source_dir/bin/cargo" ]; then
    cp "$source_dir/bin/cargo" "$output_dir/bin/cargo"
    chmod 0755 "$output_dir/bin/cargo"
    require_elf "$output_dir/bin/cargo"
    cargo_present=1
else
    echo "stage_octagram_toolchain: warning: no cargo was produced" >&2
fi

if [ ! -d "$source_dir/lib/rustlib" ]; then
    echo "stage_octagram_toolchain: error: no lib/rustlib in $source_dir" >&2
    echo "stage_octagram_toolchain: rustc needs core/alloc/std beside it" >&2
    exit 1
fi
cp -R "$source_dir/lib/rustlib" "$output_dir/lib/rustlib"

# A compiler without the host standard library cannot build anything.
host_std_dir=$output_dir/lib/rustlib/$host/lib
set +e
set -- "$host_std_dir"/libstd-*.rlib
set -e
if [ ! -e "$1" ]; then
    echo "stage_octagram_toolchain: error: std for $host is missing in $host_std_dir" >&2
    echo "stage_octagram_toolchain: bootstrap did not build the host std; rerun with --host $host so 'library/std' is included" >&2
    exit 1
fi
host_std=$1

rustc_size=$(wc -c < "$output_dir/bin/rustc" | tr -d ' ')
cargo_size=0
if [ "$cargo_present" -eq 1 ]; then
    cargo_size=$(wc -c < "$output_dir/bin/cargo" | tr -d ' ')
fi

cat > "$output_dir/toolchain.json" <<EOF
{
  "host": "$host",
  "target": "$target",
  "rust_commit": "$rust_commit",
  "llvm_version": "$llvm_version",
  "rustc_bytes": $rustc_size,
  "cargo_bytes": $cargo_size,
  "host_std": "$(basename "$host_std")",
  "install_path": "/toolchain"
}
EOF

echo "staged OCTAGRAM toolchain at $output_dir"
echo "  rustc:    $rustc_size bytes ($host)"
if [ "$cargo_present" -eq 1 ]; then
    echo "  cargo:    $cargo_size bytes"
else
    echo "  cargo:    absent"
fi
echo "  host std: $(basename "$host_std")"
