"""End-to-end A/B image test inside QEMU.

Phase 1 boots OCTAGRAM from the Live ISO, attaches a blank SATA disk and runs
`mkimage sata0 MKIMAGE-ERASE ab`, which builds the two-slot image on the disk
and verifies it by read-back.

Phase 2 boots that produced disk through UEFI firmware, then requires the
persistent EXT2 root to be selected from the A slot and the seeded update
journal to report slot A as known-good.

Environment:
    QEMU_BIN      qemu-system-x86_64 binary (default: from PATH)
    OCTAGRAM_ISO  Live ISO (default: OCTAGRAM-x86_64.iso)
    A/B disk      /dev/shm is used when writable so the build is not limited by
                  a network filesystem.
"""

import os
import socket
import subprocess
import sys
import tempfile
import time

ISO = os.environ.get("OCTAGRAM_ISO", "OCTAGRAM-x86_64.iso")
QEMU = os.environ.get("QEMU_BIN", "qemu-system-x86_64")
FIRMWARE_DIR = os.environ.get("OCTAGRAM_OVMF_DIR", "edk2-ovmf")
DISK = os.environ.get(
    "OCTAGRAM_AB_DISK",
    "/dev/shm/octagram-ab-test.img" if os.access("/dev/shm", os.W_OK) else "/tmp/octagram-ab-test.img",
)
DISK_MIB = 256
BUILD_TIMEOUT = 1200
BOOT_TIMEOUT = 180

KEY_NAMES = {
    " ": "spc",
    "/": "slash",
    "-": "minus",
    ".": "dot",
    "_": "shift-minus",
    ">": "shift-dot",
    ";": "semicolon",
}


class Session:
    def __init__(self, serial, monitor):
        self.serial = serial
        self.monitor = monitor
        self.process = None

    def text(self):
        try:
            with open(self.serial, "r", errors="replace") as handle:
                return handle.read()
        except FileNotFoundError:
            return ""

    def wait_for(self, needle, offset, timeout):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if needle in self.text()[offset:]:
                return True
            time.sleep(0.5)
        return False

    def send_line(self, command):
        client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        for _ in range(60):
            try:
                client.connect(self.monitor)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("cannot connect to the QEMU monitor")
        time.sleep(0.4)
        try:
            client.recv(4096)
        except OSError:
            pass
        for character in command:
            key = KEY_NAMES.get(character)
            if key is None:
                key = "shift-" + character.lower() if character.isupper() else character
            client.sendall(("sendkey %s\n" % key).encode())
            time.sleep(0.06)
        client.sendall(b"sendkey ret\n")
        # Keep the monitor connection open briefly so the final key is delivered
        # before the socket closes.
        time.sleep(0.8)
        client.close()


def start_qemu(serial, monitor, extra):
    if os.path.exists(serial):
        os.remove(serial)
    if os.path.exists(monitor):
        os.remove(monitor)
    command = [
        QEMU,
        "-M",
        "q35",
        "-m",
        "4G",
        "-display",
        "none",
        "-serial",
        "file:" + serial,
        "-monitor",
        "unix:%s,server,nowait" % monitor,
        "-no-reboot",
        "-no-shutdown",
    ]
    data_dir = os.environ.get("QEMU_DATA_DIR")
    if data_dir:
        command += ["-L", data_dir]
    return subprocess.Popen(command + extra)


def stop(process):
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()


def build_image(sessionExtra, serial, monitor):
    session = Session(serial, monitor)
    session.process = start_qemu(serial, monitor, sessionExtra)
    try:
        if not session.wait_for("octagram:/ $", 0, BOOT_TIMEOUT):
            print("BUILD-FAIL: the Live shell never appeared")
            print("\n".join(session.text().splitlines()[-40:]))
            return False
        offset = len(session.text())
        session.send_line("mkimage sata0 MKIMAGE-ERASE ab")
        if not session.wait_for("A/B image read-back verification passed", offset, BUILD_TIMEOUT):
            print("BUILD-FAIL: the A/B build did not verify by read-back")
            print("\n".join(session.text()[offset:].splitlines()[-40:]))
            return False
        print("MKIMAGE-AB-PASS", flush=True)
        return True
    finally:
        stop(session.process)


def boot_image(sessionExtra, serial, monitor):
    session = Session(serial, monitor)
    session.process = start_qemu(serial, monitor, sessionExtra)
    try:
        if not session.wait_for("octagram:/ $", 0, BOOT_TIMEOUT):
            print("BOOT-FAIL: the produced image did not reach a shell")
            print("\n".join(session.text().splitlines()[-40:]))
            return False
        output = session.text()
        if "persistent EXT2 root selected from sata0p2" not in output:
            print("BOOT-FAIL: the A slot root was not mounted from the disk")
            print("\n".join(output.splitlines()[-40:]))
            return False
        print("BOOT-FROM-IMAGE-PASS", flush=True)
        offset = len(output)
        session.send_line("selfupdate status")
        if not session.wait_for("active=A", offset, 60):
            print("JOURNAL-FAIL: the seeded journal did not report slot A")
            print("\n".join(session.text()[offset:].splitlines()[-20:]))
            return False
        print("JOURNAL-SLOT-A-PASS", flush=True)
        return True
    finally:
        stop(session.process)


def main():
    if not os.path.exists(ISO):
        print("SKIP: %s not found (run `make all RUST_PROFILE=release` first)" % ISO)
        return 1
    code = "%s/ovmf-code-x86_64.fd" % FIRMWARE_DIR
    variables = "%s/ovmf-vars-x86_64.fd" % FIRMWARE_DIR
    if not (os.path.exists(code) and os.path.exists(variables)):
        print("SKIP: OVMF firmware missing in %s" % FIRMWARE_DIR)
        return 1
    if os.path.exists(DISK):
        os.remove(DISK)
    with open(DISK, "wb") as handle:
        handle.truncate(DISK_MIB * 1024 * 1024)

    disk_args = [
        "-drive",
        "file=%s,format=raw,if=none,id=target0" % DISK,
        "-device",
        "ide-hd,drive=target0,bus=ide.1",
    ]

    serial = "/tmp/octagram-ab-serial.log"
    monitor = "/tmp/octagram-ab-monitor.sock"

    if not build_image(["-cdrom", ISO, "-boot", "d"] + disk_args, serial, monitor):
        return 1

    with tempfile.TemporaryDirectory() as directory:
        live_vars = os.path.join(directory, "vars.fd")
        with open(variables, "rb") as source, open(live_vars, "wb") as target:
            target.write(source.read())
        firmware = [
            "-drive",
            "if=pflash,format=raw,readonly=on,file=%s" % code,
            "-drive",
            "if=pflash,format=raw,file=%s" % live_vars,
        ]
        if not boot_image(firmware + disk_args, serial, monitor):
            return 1

    print("=== A/B IMAGE BUILD AND BOOT PASS ===", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
