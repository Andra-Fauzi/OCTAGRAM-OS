#!/usr/bin/env python3
"""Validate fresh C emitted for the mrustc PoC before guest compilation."""

import pathlib
import re
import sys


MAIN_DEFINITION = re.compile(
    r"(?m)^\\s*(?:static\\s+)?(?:int|void)\\s+main\\s*\\("
)


def validate(source: str) -> None:
    if not source.strip():
        raise ValueError("generated poc.c is empty")
    if not MAIN_DEFINITION.search(source):
        raise ValueError("generated poc.c has no recognizable main definition")


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate_poc_output.py <poc.c>", file=sys.stderr)
        return 2
    try:
        validate(pathlib.Path(sys.argv[1]).read_text())
    except (OSError, UnicodeError, ValueError) as error:
        print("mrustc PoC output validation failed: %s" % error, file=sys.stderr)
        return 1
    print("validated fresh mrustc PoC C output: main definition found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
