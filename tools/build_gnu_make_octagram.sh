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
    # Savannah intermittently returns HTTP 500 for the pinned Gnulib fetch.
    # Use the GitHub mirror that exposes the same stable-202301 revision.
    export GNULIB_URL="${GNULIB_URL:-https://github.com/coreutils/gnulib.git}"
    # A failed git fetch can leave an initialized but empty gnulib directory;
    # bootstrap treats that as an existing checkout and will not clone again.
    if [[ ! -f "$SRC/gnulib/gnulib-tool" ]]; then
        rm -rf "$SRC/gnulib"
    fi
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
if "| openbsd*" not in text:
    raise SystemExit("cannot safely extend config.sub: openbsd OS token not found")
text = text.replace("| openbsd*", "| octagram* | openbsd*", 1)
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
CFLAGS_PARTS=(
    -std=gnu11 -O2 -Wall -Wextra
    -ffreestanding -fno-builtin -fno-stack-protector
    -fno-pie -fno-pic -mno-red-zone -mcmodel=small
    -march=x86-64 -mtune=generic -mno-avx -mno-avx2
    -mno-avx512f -mno-mmx -msse2 -mfpmath=387
    -DOCTAGRAM_TLS_ERRNO -DOCTAGRAM_GNU_MAKE_ERRNO_ACCESSOR
    -Dalloca=__builtin_alloca
    -include time.h
    -nostdinc -isystem "$GCC_INTERNAL"
)
printf -v CFLAGS '%s ' "${CFLAGS_PARTS[@]}"
export CFLAGS
export LDFLAGS="-nostdlib -static -Wl,--gc-sections -Wl,-T,$USER/linker.ld"
export LIBS="$USER/build/crt0.o $USER/build/runtime.o"

echo "GNU-MAKE-OCTAGRAM: configure target=$TARGET build=$BUILD_TRIPLET"
# Keep the cross result deterministic. unistd.h advertises POSIX so GNU Make
# does not declare its legacy zero-argument getcwd prototype.
export ac_cv_func_getcwd=yes

if ! "$SRC/configure" \
    --srcdir="$SRC" \
    --build="$BUILD_TRIPLET" \
    --host="$TARGET" \
    --prefix=/usr \
    --disable-nls \
    --without-guile \
    --disable-load \
    --disable-posix-spawn; then
    if [[ -f config.log ]]; then
        echo "GNU-MAKE-OCTAGRAM: configure diagnostics (last 100 lines of config.log):" >&2
        tail -n 100 config.log >&2
    fi
    fail "upstream configure failed"
fi

echo "GNU-MAKE-OCTAGRAM: resolved compiler environment"
printf '  CC=%q\\n  CPPFLAGS=%q\\n  CFLAGS=%q\\n  LDFLAGS=%q\\n' "$CC" "$CPPFLAGS" "$CFLAGS" "$LDFLAGS"
echo "GNU-MAKE-OCTAGRAM: generated Make variables"
make -pn 2>/dev/null | grep -E '^(CC|CPP|CPPFLAGS|CFLAGS|DEFS|DEFAULT_INCLUDES|AM_CPPFLAGS|AM_CFLAGS) ?[:=]' | head -n 30 || true

echo "GNU-MAKE-OCTAGRAM: compile upstream GNU Make"
make -j2 V=1

[[ -s "$BUILD/make" ]] || fail "cross-build did not produce make"
readelf -h "$BUILD/make" | grep -q 'Advanced Micro Devices X86-64' || \
    fail "output is not an x86_64 ELF"
if readelf -l "$BUILD/make" | grep -q 'INTERP'; then
    fail "output unexpectedly requires a host dynamic loader"
fi
if readelf -d "$BUILD/make" 2>&1 | grep -q "Dynamic section"; then
    fail "output unexpectedly has a dynamic section"
fi

echo "GNU-MAKE-OCTAGRAM: PASS"
echo "GNU Make 4.4.1 cross-built for $TARGET: $BUILD/make"
