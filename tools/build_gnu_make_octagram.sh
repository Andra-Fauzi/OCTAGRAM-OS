#!/usr/bin/env bash
# Cross-build upstream GNU Make for the OCTAGRAM x86_64 userspace ABI.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/user/third_party/gnu-make"
BUILD="$ROOT/user/build/gnu-make-octagram"
USER="$ROOT/user"
TARGET="x86_64-unknown-octagram"

fail() {
    echo "GNU-MAKE-OCTAGRAM: FAIL: $*" >&2
    exit 1
}

[[ -d "$SRC/.git" || -f "$SRC/.git" ]] || fail "upstream submodule missing; checkout recursively"
[[ -f "$USER/build/crt0.o" && -f "$USER/build/runtime.o" ]] || \
    make -C "$USER" build/crt0.o build/runtime.o

# GNU Make tracks generated Autotools files outside Git. Bootstrap the pinned
# upstream checkout and its pinned gnulib revision before configuring it.
if [[ ! -x "$SRC/configure" || ! -f "$SRC/src/config.h.in" ]]; then
    echo "GNU-MAKE-OCTAGRAM: bootstrapping upstream Autotools files"
    (cd "$SRC" && ./bootstrap --skip-po)
fi

CONFIG_SUB="$SRC/build-aux/config.sub"
[[ -x "$CONFIG_SUB" ]] || fail "bootstrap did not generate build-aux/config.sub"

# The upstream config.sub snapshot predates OCTAGRAM. Add the OS token to its
# canonical operating-system allowlist without changing any other target rule.
if ! "$CONFIG_SUB" "$TARGET" >/dev/null 2>&1; then
    python3 - "$CONFIG_SUB" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
anchor = "\t| openbsd* \\\n"
if anchor not in text:
    raise SystemExit("cannot safely extend config.sub: openbsd OS anchor not found")
text = text.replace(anchor, "\t| octagram* \\\n" + anchor, 1)
path.write_text(text)
PY
fi
"$CONFIG_SUB" "$TARGET" >/dev/null || fail "config.sub does not recognize $TARGET"

CC="${CC:-gcc}"
AR="${AR:-ar}"
RANLIB="${RANLIB:-ranlib}"
BUILD_TRIPLET="$("$CC" -dumpmachine)"
GCC_INTERNAL="$("$CC" -print-file-name=include)"
[[ -d "$GCC_INTERNAL" ]] || fail "cannot locate GCC internal headers"

mkdir -p "$BUILD"
cd "$BUILD"

export CC AR RANLIB
export CPPFLAGS="-I$USER/include -I$SRC/src -I$SRC/lib -I$SRC/gl -D_GNU_SOURCE=1"
export CFLAGS="-std=gnu11 -O2 -Wall -Wextra -ffreestanding -fno-builtin \
-fno-stack-protector -fno-pie -fno-pic -mno-red-zone -mcmodel=small \
-march=x86-64 -mtune=generic -mno-avx -mno-avx2 -mno-avx512f -mno-mmx \
-msse2 -mfpmath=387 -DOCTAGRAM_TLS_ERRNO -nostdinc -isystem $GCC_INTERNAL"
export LDFLAGS="-nostdlib -static -Wl,--gc-sections -Wl,-T,$USER/linker.ld"
export LIBS="$USER/build/crt0.o $USER/build/runtime.o"

echo "GNU-MAKE-OCTAGRAM: configure target=$TARGET build=$BUILD_TRIPLET"
"$SRC/configure" \
    --srcdir="$SRC" \
    --build="$BUILD_TRIPLET" \
    --host="$TARGET" \
    --prefix=/usr \
    --disable-nls \
    --without-guile \
    --disable-load \
    --disable-posix-spawn

echo "GNU-MAKE-OCTAGRAM: compile upstream GNU Make"
make -j2 V=1

[[ -s "$BUILD/make" ]] || fail "cross-build did not produce make"
readelf -h "$BUILD/make" | grep -q 'Advanced Micro Devices X86-64' || \
    fail "output is not an x86_64 ELF"
if readelf -l "$BUILD/make" | grep -q 'INTERP'; then
    fail "output unexpectedly requires a host dynamic loader"
fi
if readelf -d "$BUILD/make" >/dev/null 2>&1; then
    fail "output unexpectedly has a dynamic section"
fi

echo "GNU-MAKE-OCTAGRAM: PASS"
echo "GNU Make 4.4.1 cross-built for $TARGET: $BUILD/make"
