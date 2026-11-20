"""Prove that work stored on an EXT2 disk survives a reboot of OCTAGRAM-OS.

Run QEMU with a second, unformatted disk (for example
`-drive file=/tmp/ext2-test.img,if=none,id=ext2,format=raw
-device ide-hd,drive=ext2,bus=ide.1` on a q35 machine) and with the monitor on
a unix socket. Set MON and SERIAL to those paths, or rely on the defaults:

    MON=/tmp/mon-ext2.sock SERIAL=/tmp/serial-ext2.log \
        python3 tools/qemu_ext2_persistence_test.py

The test formats the second disk with the native `mkfs.ext2`, mounts it,
writes a source file, unmounts it, resets the machine, and after the reboot
mounts the same disk and requires the exact content back. It also exercises the
shell pipeline and output redirection on that disk.
"""

import os
import socket
import sys
import time

MON = os.environ.get("MON", "/tmp/mon-ext2.sock")
SERIAL = os.environ.get("SERIAL", "/tmp/serial-ext2.log")

DISK = os.environ.get("DISK", "sata1")

MARKER = "OCTAGRAM-EXT2-PERSISTENCE-PASS"
FILE = "/mnt/src/persist.txt"
RENAMED = "/mnt/src/renamed.txt"
REDIRECTED = "/mnt/src/pipeline.txt"

KEY_NAMES = {
    "/": "slash",
    "-": "minus",
    ".": "dot",
    ">": "shift-dot",
    "_": "shift-minus",
    " ": "spc",
}


def serial_text():
    try:
        with open(SERIAL, "r", errors="replace") as handle:
            return handle.read()
    except FileNotFoundError:
        return ""


def wait_for_after(text, offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text in serial_text()[offset:]:
            return True
        time.sleep(0.5)
    return False


def wait_prompt_after(offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        tail = serial_text()[offset:]
        if "octagram:" in tail and tail.rstrip().endswith("$"):
            return True
        time.sleep(0.5)
    return False


def main():
    if not wait_for_after("octagram:", 0, 300):
        print("BOOT-FAIL: shell prompt never appeared")
        print(serial_text()[-3000:])
        return 1

    monitor = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    for _ in range(60):
        try:
            monitor.connect(MON)
            break
        except OSError:
            time.sleep(0.5)
    else:
        print("MONITOR-FAIL: cannot connect to %s" % MON)
        return 1
    monitor.settimeout(2)
    try:
        monitor.recv(4096)
    except OSError:
        pass

    def send_line(command, pause=0.06):
        for character in command:
            key = KEY_NAMES.get(character, character)
            monitor.sendall(("sendkey %s\n" % key).encode())
            time.sleep(pause)
        monitor.sendall(b"sendkey ret\n")
        time.sleep(0.5)

    def expect(command, marker, label, timeout=60):
        offset = len(serial_text())
        send_line(command)
        if not wait_for_after(marker, offset, timeout):
            print("%s-FAIL: '%s' did not produce '%s'" % (label, command, marker))
            print(serial_text()[-5000:])
            return None
        return offset

    if (
        expect(
            "mkfs.ext2 " + DISK + " --yes",
            "Formatted " + DISK + " as EXT2",
            "MKFS",
            420,
        )
        is None
    ):
        return 1
    if (
        expect(
            "mount " + DISK + " /mnt ext2",
            "mounted " + DISK + " at /mnt (ext2, read-write)",
            "MOUNT",
        )
        is None
    ):
        return 1
    if expect("mkdir /mnt/src", "octagram:/ $", "MKDIR") is None:
        return 1
    if expect("stat /mnt/src", "directory", "MKDIR-STAT") is None:
        return 1
    if expect("echo " + MARKER + " > " + FILE, "octagram:/ $", "WRITE") is None:
        return 1
    if expect("stat " + FILE, "bytes", "WRITE-STAT") is None:
        return 1
    if expect("cat " + FILE, MARKER, "READBACK") is None:
        return 1
    if expect("ls /mnt/src", "persist.txt", "LS") is None:
        return 1

    # Pipeline and output redirection on the persistent disk.
    if expect("cat " + FILE + " | wc", "1 1 ", "PIPE-WC") is None:
        return 1
    if (
        expect(
            "grep " + MARKER + " " + FILE + " > " + REDIRECTED,
            "octagram:/ $",
            "REDIRECT",
        )
        is None
    ):
        return 1
    if expect("cat " + REDIRECTED, MARKER, "REDIRECT-READBACK") is None:
        return 1

    if expect("mv " + FILE + " " + RENAMED, "octagram:/ $", "MV") is None:
        return 1
    if expect("stat " + FILE, "NotFound", "MV-OLD-GONE") is None:
        return 1
    if expect("cat " + RENAMED, MARKER, "MV-NEW-NAME") is None:
        return 1

    if expect("umount /mnt", "unmounted /mnt", "UMOUNT") is None:
        return 1

    reboot_offset = len(serial_text())
    monitor.sendall(b"system_reset\n")
    time.sleep(1)
    if not wait_for_after("OCTAGRAM-OS shell", reboot_offset, 300):
        print("REBOOT-FAIL: shell did not come back after reset")
        print(serial_text()[-5000:])
        return 1

    # After the reboot the disk contents must still be there.
    offset = reboot_offset
    if not wait_prompt_after(offset, 300):
        print("REBOOT-FAIL: prompt did not return after reset")
        print(serial_text()[-5000:])
        return 1
    if (
        expect(
            "mount " + DISK + " /mnt ext2",
            "mounted " + DISK + " at /mnt (ext2, read-write)",
            "REMOUNT",
        )
        is None
    ):
        return 1
    if expect("cat " + RENAMED, MARKER, "PERSISTENCE") is None:
        return 1
    if expect("stat " + FILE, "NotFound", "PERSISTENCE-OLD-GONE") is None:
        return 1
    if expect("cat " + REDIRECTED, MARKER, "PERSISTENCE-REDIRECT") is None:
        return 1
    if expect("wc " + FILE, "1 1 ", "PERSISTENCE-WC") is None:
        return 1

    print("EXT2-PERSISTENCE-PASS: native mkfs.ext2, mount, write, pipeline,")
    print("redirection, rename, unmount, reboot, remount, and read-back passed")
    print(serial_text()[-3000:])
    return 0


if __name__ == "__main__":
    sys.exit(main())
