#!/usr/bin/env python3
"""Check that the fcntl command numbers used by the ABI are all distinct.

The libc fcntl wrapper dispatches F_DUPFD/F_DUPFD_CLOEXEC/F_SETFD/F_SETFL as
integer varargs and F_GETLK/F_SETLK/F_SETLKW as struct flock pointers, so two
names sharing a value silently sends a call down the wrong path. That is exactly
how F_DUPFD_CLOEXEC (5) stopped duplicating descriptors once F_GETLK took 5; the
value is now the Linux-compatible 1030.

Usage: tools/check_octagram_fcntl_commands.py [header...]
"""

import re
import sys
from pathlib import Path

DEFAULT_HEADERS = ["user/include/fcntl.h"]

# Only command macros are dispatched on their numeric value. The lock types
# (F_RDLCK/F_WRLCK/F_UNLCK) and the O_* flags live in their own number spaces
# and legitimately reuse 0/1/2.
COMMAND_NAMES = (
    "F_DUPFD",
    "F_DUPFD_CLOEXEC",
    "F_GETFD",
    "F_SETFD",
    "F_GETFL",
    "F_SETFL",
    "F_GETLK",
    "F_SETLK",
    "F_SETLKW",
)


def main(argv):
    headers = [Path(path) for path in (argv or DEFAULT_HEADERS)]
    failures = 0
    for header in headers:
        if not header.exists():
            print("FAIL: %s does not exist" % header, file=sys.stderr)
            failures += 1
            continue
        text = header.read_text()
        defines = {
            name: int(value)
            for name, value in re.findall(r"#define (F_[A-Z0-9_]+) (\d+)\b", text)
        }
        missing = [name for name in COMMAND_NAMES if name not in defines]
        if missing:
            print(
                "FAIL: %s is missing %s" % (header, ", ".join(missing)),
                file=sys.stderr,
            )
            failures += 1
        commands = {name: defines[name] for name in COMMAND_NAMES if name in defines}
        by_value = {}
        for name, value in sorted(commands.items()):
            if name not in COMMAND_NAMES:
                continue
            if value in by_value:
                print(
                    "FAIL: %s assigns %s and %s the same value %d"
                    % (header, by_value[value], name, value),
                    file=sys.stderr,
                )
                failures += 1
                continue
            by_value[value] = name
        print("%s: %d distinct fcntl command numbers" % (header, len(by_value)))

    if failures:
        return 1
    print("OCTAGRAM-FCNTL-COMMANDS-PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
