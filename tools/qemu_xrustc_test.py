"""Compile and run Rust inside OCTAGRAM with the native `xrustc` frontend.

The test boots a host-built A/B disk image through UEFI firmware (no ISO
writer needed), then drives the shell over the QEMU monitor:

  * `xrustc /share/octagram/xrustc-sum.rs -o /tmp/xrustc-sum.elf` must produce
    an ELF whose entry point the native linker chose,
  * `run /tmp/xrustc-sum.elf` must print the value the subset program computes
    with recursion and a loop, then exit with status 0,
  * `xrustc /share/octagram/xrustc-bad.rs -o /tmp/xrustc-bad.elf` must be
    rejected with a source line and must not write an ELF.

Environment:
    QEMU_BIN              qemu-system-x86_64 (default: $HOME/qemu-root/usr/bin)
    OCTAGRAM_OVMF_DIR     OVMF firmware directory (default: edk2-ovmf)
    OCTAGRAM_XRUSTC_DISK  bootable A/B image (default: /dev/shm/octagram-xrustc.img)
"""

import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time

QEMU = os.environ.get(
    "QEMU_BIN",
    os.path.expanduser("~/qemu-root/usr/bin/qemu-system-x86_64"),
)
FIRMWARE_DIR = os.environ.get("OCTAGRAM_OVMF_DIR", "edk2-ovmf")
QEMU_DATA_DIR = os.environ.get(
    "OCTAGRAM_QEMU_DATA", os.path.expanduser("~/qemu-root/usr/share/qemu")
)
DISK = os.environ.get(
    "OCTAGRAM_XRUSTC_DISK",
    "/dev/shm/octagram-xrustc.img"
    if os.access("/dev/shm", os.W_OK)
    else "/tmp/octagram-xrustc.img",
)
SERIAL = os.environ.get("OCTAGRAM_XRUSTC_SERIAL", "/tmp/octagram-xrustc-serial.log")
MONITOR = os.environ.get("OCTAGRAM_XRUSTC_MONITOR", "/tmp/octagram-xrustc-monitor.sock")
MEMORY_MIB = os.environ.get("OCTAGRAM_XRUSTC_MEMORY", "2048")
BOOT_TIMEOUT = 300
COMMAND_TIMEOUT = 120

KEY_NAMES = {
    " ": "spc",
    "/": "slash",
    "-": "minus",
    ".": "dot",
    "_": "shift-minus",
    ">": "shift-dot",
    "<": "shift-comma",
    "=": "equal",
    "+": "shift-equal",
    ":": "shift-semicolon",
    ";": "semicolon",
    ",": "comma",
    "(": "shift-9",
    ")": "shift-0",
    '"': "shift-apostrophe",
}


def serial_text():
    try:
        with open(SERIAL, "r", errors="replace") as handle:
            return handle.read()
    except FileNotFoundError:
        return ""


def wait_for(needle, offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if needle in serial_text()[offset:]:
            return True
        time.sleep(0.25)
    return False


class Session:
    def __init__(self, process, monitor):
        self.process = process
        self.monitor = monitor

    def send_line(self, command):
        for character in command:
            key = KEY_NAMES.get(character, character)
            self.monitor.sendall(("sendkey %s\n" % key).encode())
            time.sleep(0.05)
        self.monitor.sendall(b"sendkey ret\n")
        time.sleep(0.4)

    def run(self, command, needle, timeout=COMMAND_TIMEOUT):
        offset = len(serial_text())
        self.send_line(command)
        if wait_for(needle, offset, timeout):
            return offset, True
        return offset, False

    def stop(self):
        try:
            self.monitor.sendall(b"quit\n")
        except OSError:
            pass
        self.process.terminate()
        try:
            self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.process.kill()


def main():
    if not os.path.exists(DISK):
        print("SKIP: bootable image missing: %s" % DISK)
        print("Build it with tools/build_xrustc_boot_image.sh")
        return 2
    code = os.path.join(FIRMWARE_DIR, "ovmf-code-x86_64.fd")
    vars_template = os.path.join(FIRMWARE_DIR, "ovmf-vars-x86_64.fd")
    if not os.path.exists(code) or not os.path.exists(vars_template):
        print("SKIP: OVMF firmware missing in %s" % FIRMWARE_DIR)
        return 2

    for path in (SERIAL, MONITOR):
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass
    live_vars = os.path.join(tempfile.gettempdir(), "octagram-xrustc-vars.fd")
    shutil.copyfile(vars_template, live_vars)

    command = [
        QEMU,
        "-L", QEMU_DATA_DIR,
        "-M", "q35",
        "-m", MEMORY_MIB,
        "-display", "none",
        "-no-reboot",
        "-serial", "file:%s" % SERIAL,
        "-monitor", "unix:%s,server,nowait" % MONITOR,
        "-drive", "if=pflash,format=raw,readonly=on,file=%s" % code,
        "-drive", "if=pflash,format=raw,file=%s" % live_vars,
        "-drive", "file=%s,format=raw,if=none,id=disk0" % DISK,
        "-device", "ide-hd,drive=disk0,bus=ide.0",
    ]
    process = subprocess.Popen(
        command,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.STDOUT,
    )

    monitor = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    for _ in range(120):
        try:
            monitor.connect(MONITOR)
            break
        except OSError:
            if process.poll() is not None:
                print("QEMU-EXIT: %s" % process.returncode)
                print(serial_text()[-3000:])
                return 1
            time.sleep(0.5)
    else:
        process.kill()
        print("MONITOR-FAIL: cannot connect")
        return 1
    monitor.settimeout(2)
    try:
        monitor.recv(4096)
    except OSError:
        pass

    session = Session(process, monitor)
    try:
        if not wait_for("octagram:", 0, BOOT_TIMEOUT):
            print("BOOT-FAIL: shell prompt never appeared")
            print(serial_text()[-4000:])
            return 1
        if not wait_for("[rootfs] persistent EXT2 root selected", 0, 10):
            print("NOTE: kernel did not report a persistent root; /share may be read-only")

        offset, ok = session.run(
            "xrustc /share/octagram/xrustc-sum.rs -o /tmp/xrustc-sum.elf",
            "xrustc: entry point",
        )
        if not ok:
            print("XRUSTC-COMPILE-FAIL: native Rust subset compile did not finish")
            print(serial_text()[offset:][-4000:])
            return 1
        if "xrustc: wrote" not in serial_text()[offset:]:
            print("XRUSTC-COMPILE-FAIL: no ELF was written")
            print(serial_text()[offset:][-4000:])
            return 1

        offset, ok = session.run("run /tmp/xrustc-sum.elf", "xrustc fib sum: 88")
        if not ok:
            print("XRUSTC-RUN-FAIL: subset program output was not observed")
            print(serial_text()[offset:][-4000:])
            return 1
        if not wait_for("program exited with status 0", offset, 60):
            print("XRUSTC-RUN-FAIL: subset program did not exit successfully")
            print(serial_text()[offset:][-4000:])
            return 1

        offset, ok = session.run(
            "xrustc /share/octagram/xrustc-bad.rs -o /tmp/xrustc-bad.elf",
            "is not supported by xrustc stage 0",
        )
        if not ok:
            print("XRUSTC-REJECT-FAIL: unsupported source was not rejected with a line")
            print(serial_text()[offset:][-4000:])
            return 1
        if "xrustc: wrote" in serial_text()[offset:]:
            print("XRUSTC-REJECT-FAIL: rejected source still produced an ELF")
            print(serial_text()[offset:][-4000:])
            return 1

        print("XRUSTC-SUBSET-PASS")
        print("=== XRUSTC NATIVE RUST SUBSET PASS ===")
        return 0
    finally:
        session.stop()


if __name__ == "__main__":
    sys.exit(main())
