#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOSTCC="${HOSTCC:-cc}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 -m unittest discover -s "$ROOT/tools/mrustc_poc" -p 'test_*.py'
python3 "$ROOT/tools/check_tinycc_xgetbv_encoding.py"
PYTHONPYCACHEPREFIX="$TMP/pycache" python3 -m py_compile \
    "$ROOT/tools/mrustc_poc/qemu_test.py" \
    "$ROOT/tools/qemu_tcc_selfhost_test.py"
bash -n "$ROOT/tools/mrustc_poc/build_host.sh"


"$HOSTCC" -std=c11 -O2 -Wall -Wextra -Werror \
    -I"$ROOT/tools/mrustc_poc" \
    "$ROOT/tools/mrustc_poc/test_atomic128_shim.c" \
    -o "$TMP/test_atomic128_shim"
"$TMP/test_atomic128_shim"

echo "MRUSTC-COMPAT-HOST-PASS"
