#!/usr/bin/env python3
"""Apply the narrowly scoped compatibility rewrites needed by the TCC PoC."""

import pathlib
import sys

XGETBV = '__asm__ __volatile__ ("xgetbv" : "=a" (lo), "=d" (hi) : "c" (arg0) );'
XGETBV_REPLACEMENT = "lo = 0; hi = 0; /* tcc: no XGETBV opcode */"
ATOMIC128 = (
    "atomic_compare_exchange_strong_explicit((uint128_t _Atomic *)arg0, "
    "&var1._0, arg2, memory_order_seq_cst, memory_order_seq_cst)"
)
ATOMIC128_REPLACEMENT = (
    "poc_cxchg128((unsigned __int128 *)arg0, &var1._0, arg2)"
)


def rewrite(source: str) -> str:
    """Rewrite known mrustc output, accepting an already-rewritten file."""
    if XGETBV in source:
        source = source.replace(XGETBV, XGETBV_REPLACEMENT)
    elif XGETBV_REPLACEMENT not in source:
        raise ValueError("expected XGETBV fragment not found; mrustc output changed")

    if ATOMIC128 in source:
        source = source.replace(ATOMIC128, ATOMIC128_REPLACEMENT)
    elif ATOMIC128_REPLACEMENT not in source:
        raise ValueError("expected AtomicU128 fragment not found; mrustc output changed")

    if '__asm__ __volatile__ ("xgetbv"' in source:
        raise ValueError("unpatched XGETBV instruction remains")
    if "atomic_compare_exchange_strong_explicit((uint128_t _Atomic *)arg0" in source:
        raise ValueError("unpatched AtomicU128 intrinsic remains")
    return source


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: patch_core.py <libcore.c>", file=sys.stderr)
        return 2
    path = pathlib.Path(sys.argv[1])
    try:
        original = path.read_text()
        if not original:
            raise ValueError("generated libcore.c is empty")
        patched = rewrite(original)
        path.write_text(patched)
    except (OSError, ValueError) as error:
        print("mrustc compatibility patch failed: %s" % error, file=sys.stderr)
        return 1
    print("validated libcore.c compatibility rewrites: XGETBV + AtomicU128")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
