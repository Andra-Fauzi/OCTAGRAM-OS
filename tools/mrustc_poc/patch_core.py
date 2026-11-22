#!/usr/bin/env python3
"""Apply the remaining narrowly scoped mrustc-to-TinyCC compatibility rewrite."""

import pathlib
import sys

XGETBV = '__asm__ __volatile__ ("xgetbv" : "=a" (lo), "=d" (hi) : "c" (arg0) );'
OBSOLETE_XGETBV_STUB = "lo = 0; hi = 0; /* tcc: no XGETBV opcode */"
ATOMIC128 = (
    "atomic_compare_exchange_strong_explicit((uint128_t _Atomic *)arg0, "
    "&var1._0, arg2, memory_order_seq_cst, memory_order_seq_cst)"
)
ATOMIC128_REPLACEMENT = (
    "poc_cxchg128((void *)arg0, (void *)&var1._0, (const void *)&arg2)"
)


def rewrite(source: str) -> str:
    """Preserve real XGETBV assembly and rewrite the still-unsupported AtomicU128."""
    if XGETBV not in source:
        raise ValueError("expected XGETBV fragment not found; mrustc output changed")
    if OBSOLETE_XGETBV_STUB in source:
        raise ValueError("obsolete XGETBV no-op stub found; keep the real instruction")

    if ATOMIC128 in source:
        source = source.replace(ATOMIC128, ATOMIC128_REPLACEMENT)
    elif ATOMIC128_REPLACEMENT not in source:
        raise ValueError("expected AtomicU128 fragment not found; mrustc output changed")

    if XGETBV not in source:
        raise ValueError("XGETBV instruction was unexpectedly modified")
    if ATOMIC128 in source:
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
    print("validated libcore.c compatibility rewrite: AtomicU128 (XGETBV preserved)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
