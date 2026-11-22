#!/usr/bin/env bash
# Build mrustc on the host, transpile Rust `core` and the PoC program to C, and
# write an EXT2 source disk that the guest can compile with its own `tcc`.
#
# Everything lands in $WORK (default /dev/shm/mrustc-poc): it needs roughly
# 4 GiB of scratch space because the Rust source tarball is ~575 MiB.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="${WORK:-/dev/shm/mrustc-poc}"
RUST_VERSION="${RUST_VERSION:-1.90.0}"
MRUSTC_TARGET_VER="${MRUSTC_TARGET_VER:-1.90}"
PARLEVEL="${PARLEVEL:-$(nproc)}"
SRC="$WORK/rustc-$RUST_VERSION-src"
MRUSTC_DIR="$WORK/mrustc"
IMAGE="$WORK/sources.img"

mkdir -p "$WORK"
bash "$ROOT/tools/test_mrustc_compat_host.sh"

if [[ ! -d "$MRUSTC_DIR" ]]; then
    git clone --depth 1 https://github.com/thepowersgang/mrustc "$MRUSTC_DIR"
fi

# mrustc injects the implicit `alloc_system` crate into every binary crate; a
# no_std crate does not ship one, so disable the injection for this PoC.
python3 - "$MRUSTC_DIR" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1]) / "src" / "main.cpp"
text = path.read_text()
if "false && !allocator_crate_loaded" not in text:
    old = "                if( !allocator_crate_loaded )"
    new = "                if( false && !allocator_crate_loaded )"
    if old not in text:
        raise SystemExit("mrustc alloc_system patch point not found")
    path.write_text(text.replace(old, new, 1))
    print("patched mrustc: alloc_system injection disabled")
PY

make -C "$MRUSTC_DIR" -j"$PARLEVEL" bin/mrustc

if [[ ! -d "$SRC/library" ]]; then
    curl -fL -o "$WORK/rustc-$RUST_VERSION-src.tar.gz" \
        "https://static.rust-lang.org/dist/rustc-$RUST_VERSION-src.tar.gz"
    mkdir -p "$SRC"
    tar xzf "$WORK/rustc-$RUST_VERSION-src.tar.gz" -C "$SRC" --strip-components=1 \
        --exclude='*/src/llvm-project/*' --exclude='*/src/tools/*'
fi

# Apply the mrustc compatibility patch exactly once. Do not mask arbitrary
# patch failures: distinguish a clean tree, an already-patched tree, and a
# source archive that no longer matches the patch.
RUSTC_PATCH="$MRUSTC_DIR/rustc-$RUST_VERSION-src.patch"
if (cd "$SRC" && patch --dry-run -p0 --forward <"$RUSTC_PATCH" >/dev/null 2>&1); then
    (cd "$SRC" && patch -p0 --forward <"$RUSTC_PATCH")
elif (cd "$SRC" && patch --dry-run -R -p0 <"$RUSTC_PATCH" >/dev/null 2>&1); then
    echo "Rust source compatibility patch already applied"
else
    echo "ERROR: Rust $RUST_VERSION source does not match the mrustc patch" >&2
    exit 1
fi

MRUSTC="$MRUSTC_DIR/bin/mrustc"

# `core` must exist first: mrustc cannot compile a `#![no_core]` crate on its
# own (it fails with "Couldn't find component 0 of ::ops::RangeFull").
MRUSTC_TARGET_VER="$MRUSTC_TARGET_VER" "$MRUSTC" \
    --crate-name core --crate-type rlib --edition 2024 \
    --target x86_64-unknown-linux-gnu \
    -o "$WORK/libcore" "$SRC/library/core/src/lib.rs"

# mrustc tries to link the program itself and fails (it does not add libcore.o
# to the link line); the generated poc.c is what this PoC needs.
MRUSTC_TARGET_VER="$MRUSTC_TARGET_VER" "$MRUSTC" \
    --crate-name poc --crate-type bin --edition 2021 \
    --target x86_64-unknown-linux-gnu \
    --extern core="$WORK/libcore.hir" \
    -o "$WORK/poc" "$ROOT/tools/mrustc_poc/poc.rs" || true

test -s "$WORK/poc.c"

# TinyCC now assembles XGETBV directly. AtomicU128 remains outside this PoC's
# supported subset and is narrowly redirected to its unreachable-path helper.
python3 "$ROOT/tools/mrustc_poc/patch_core.py" "$WORK/libcore.c"
test -s "$WORK/libcore.c"
test -s "$WORK/poc.c"

rm -f "$IMAGE"
mke2fs -q -t ext2 -F -b 4096 -I 128 \
    -O sparse_super,filetype,^extent,^64bit,^metadata_csum,^flex_bg,^dir_index,^resize_inode \
    "$IMAGE" 256M
debugfs -w -R "mkdir /src" "$IMAGE" >/dev/null 2>&1
for file in libcore.c poc.c; do
    debugfs -w -R "write $WORK/$file /src/$file" "$IMAGE" >/dev/null 2>&1
done
for file in fix.h shim.c poc.rs; do
    debugfs -w -R "write $ROOT/tools/mrustc_poc/$file /src/$file" "$IMAGE" >/dev/null 2>&1
done
debugfs -w -R "write $ROOT/user/examples/tcc_overflow.c /src/tcc_overflow.c" "$IMAGE" >/dev/null 2>&1
debugfs -w -R "write $ROOT/user/examples/tcc_xgetbv.c /src/tcc_xgetbv.c" "$IMAGE" >/dev/null 2>&1
debugfs -w -R "write $ROOT/user/examples/tcc_math.c /src/tcc_math.c" "$IMAGE" >/dev/null 2>&1
debugfs -w -R "write $ROOT/user/examples/tcc_atomic128_shim.c /src/tcc_atomic128_shim.c" "$IMAGE" >/dev/null 2>&1

echo "MRUSTC-C-POC-ARTIFACTS: $WORK"
echo "SOURCE-IMAGE: $IMAGE"
