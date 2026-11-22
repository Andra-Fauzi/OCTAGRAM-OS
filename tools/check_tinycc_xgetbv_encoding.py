#!/usr/bin/env python3
"""Guard the TinyCC fixed-byte XGETBV assembler table entry."""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
ENTRY = re.compile(
    r"DEF_ASM_OP0L\(xgetbv,\s*0xd0,\s*0,\s*OPC_0F01\)"
)

for relative in (
    "user/third_party/tinycc/x86_64-asm.h",
    "user/third_party/tinycc/i386-asm.h",
):
    source = (ROOT / relative).read_text()
    matches = ENTRY.findall(source)
    if len(matches) != 1:
        raise SystemExit(
            "%s: expected exactly one fixed XGETBV 0F 01 D0 entry, found %d"
            % (relative, len(matches))
        )

assembler = (ROOT / "user/third_party/tinycc/i386-asm.c").read_text()
if "pa->sym == TOK_ASM_xgetbv" in assembler:
    raise SystemExit(
        "i386-asm.c: XGETBV is fixed OPC_0F01 bytes and must not use a "
        "synthetic generic ModRM operand"
    )

print("TCC-XGETBV-TABLE-PASS")
