"""Compile mrustc-generated C with the guest `tcc` and run the result.

Boots a host-built A/B disk image (tools/build_xrustc_boot_image.sh) plus a
second disk that holds the C sources produced by tools/mrustc_poc/build_host.sh,
mounts it at /mnt, compiles with the guest tcc, and runs the ELF.

Environment:
    OCTAGRAM_POC_WORK     artifact directory (default: /dev/shm/mrustc-poc)
    OCTAGRAM_POC_BOOT     bootable A/B image (default: /dev/shm/octagram-xrustc.img)
    OCTAGRAM_POC_SOURCES  EXT2 source disk (default: $WORK/sources.img)
    QEMU_BIN              qemu-system-x86_64 (default: $HOME/qemu-root/usr/bin)
    OCTAGRAM_OVMF_DIR     OVMF firmware directory (default: edk2-ovmf)
    OCTAGRAM_POC_QEMU_LOG host QEMU output log (default: unique temp file)
"""

import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time

QEMU = os.environ.get(
    "QEMU_BIN", os.path.expanduser("~/qemu-root/usr/bin/qemu-system-x86_64")
)
FIRMWARE_DIR = os.environ.get("OCTAGRAM_OVMF_DIR", "edk2-ovmf")
QEMU_DATA_DIR = os.environ.get(
    "OCTAGRAM_QEMU_DATA", os.path.expanduser("~/qemu-root/usr/share/qemu")
)
WORK = os.environ.get("OCTAGRAM_POC_WORK", "/dev/shm/mrustc-poc")
BOOT = os.environ.get("OCTAGRAM_POC_BOOT", "/dev/shm/octagram-xrustc.img")
SOURCES = os.environ.get("OCTAGRAM_POC_SOURCES", os.path.join(WORK, "sources.img"))
SERIAL = os.environ.get("OCTAGRAM_POC_SERIAL", "/tmp/octagram-mrustc-serial.log")
MONITOR = os.environ.get("OCTAGRAM_POC_MONITOR", "/tmp/octagram-mrustc-monitor.sock")
QEMU_LOG = os.environ.get("OCTAGRAM_POC_QEMU_LOG")
MEMORY_MIB = os.environ.get("OCTAGRAM_POC_MEMORY", "2048")
COMPILE_TIMEOUT = int(os.environ.get("OCTAGRAM_POC_COMPILE_TIMEOUT", "540"))

KEY_NAMES = {
    " ": "spc",
    "/": "slash",
    "-": "minus",
    ".": "dot",
    "_": "shift-minus",
    ":": "shift-semicolon",
    ">": "shift-dot",
    "(": "shift-9",
    ")": "shift-0",
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
        time.sleep(0.5)
    return False


def wait_prompt(offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        tail = serial_text()[offset:]
        if "octagram:" in tail and tail.rstrip().endswith("$"):
            return True
        time.sleep(0.5)
    return False


def key_name(character):
    if character in KEY_NAMES:
        return KEY_NAMES[character]
    if "A" <= character <= "Z":
        return "shift-" + character.lower()
    return character


def main():
    global SERIAL, MONITOR, QEMU_LOG
    for path in (BOOT, SOURCES):
        if not os.path.exists(path):
            print("SKIP: missing %s" % path)
            return 2
    code = os.path.join(FIRMWARE_DIR, "ovmf-code-x86_64.fd")
    vars_template = os.path.join(FIRMWARE_DIR, "ovmf-vars-x86_64.fd")
    if not os.path.exists(code) or not os.path.exists(vars_template):
        print("SKIP: OVMF firmware missing in %s" % FIRMWARE_DIR)
        return 2

    if "OCTAGRAM_POC_SERIAL" not in os.environ:
        serial_fd, SERIAL = tempfile.mkstemp(
            prefix="octagram-mrustc-", suffix=".serial.log"
        )
        os.close(serial_fd)
    if "OCTAGRAM_POC_MONITOR" not in os.environ:
        monitor_fd, MONITOR = tempfile.mkstemp(
            prefix="octagram-mrustc-", suffix=".monitor.sock"
        )
        os.close(monitor_fd)
        os.unlink(MONITOR)
    if QEMU_LOG is None:
        log_fd, QEMU_LOG = tempfile.mkstemp(
            prefix="octagram-mrustc-", suffix=".qemu.log"
        )
        os.close(log_fd)

    for path in (SERIAL, MONITOR):
        try:
            os.unlink(path)
        except FileNotFoundError:
            pass
    vars_fd, live_vars = tempfile.mkstemp(prefix="octagram-mrustc-vars-", suffix=".fd")
    os.close(vars_fd)
    try:
        shutil.copyfile(vars_template, live_vars)
    except BaseException:
        os.unlink(live_vars)
        raise

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
        "-drive", "file=%s,format=raw,if=none,id=disk0" % BOOT,
        "-device", "ide-hd,drive=disk0,bus=ide.0",
        "-drive", "file=%s,format=raw,if=none,id=disk1" % SOURCES,
        "-device", "ide-hd,drive=disk1,bus=ide.1",
    ]
    process = None
    monitor = None
    qemu_log_handle = None
    connected = False

    def send_line(text, pause=0.04):
        for character in text:
            monitor.sendall(("sendkey %s\n" % key_name(character)).encode())
            time.sleep(pause)
        monitor.sendall(b"sendkey ret\n")
        time.sleep(0.4)

    try:
        qemu_log_handle = open(QEMU_LOG, "w")
        process = subprocess.Popen(
            command, stdout=qemu_log_handle, stderr=subprocess.STDOUT
        )
        monitor = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        for _ in range(120):
            try:
                monitor.connect(MONITOR)
                connected = True
                break
            except OSError:
                if process is not None and process.poll() is not None:
                    print("QEMU-EXIT: %s" % process.returncode)
                    print(serial_text()[-3000:])
                    qemu_log_handle.flush()
                    with open(QEMU_LOG, "r", errors="replace") as handle:
                        print("=== QEMU LOG ===")
                        print(handle.read()[-3000:])
                    return 1
                time.sleep(0.5)
        if not connected:
            print("MONITOR-FAIL: cannot connect")
            return 1
        monitor.settimeout(2)
        try:
            monitor.recv(4096)
        except OSError:
            pass

        if not wait_for("octagram:", 0, 240):
            print("BOOT-FAIL: shell prompt never appeared")
            print(serial_text()[-4000:])
            return 1

        mounted = False
        for disk in ("sata1", "ata1"):
            offset = len(serial_text())
            send_line("mount %s /mnt ext2" % disk)
            if wait_for("mounted %s at /mnt" % disk, offset, 90):
                mounted = True
                break
        if not mounted:
            print("MOUNT-FAIL: /mnt was not mounted")
            print(serial_text()[-4000:])
            return 1

        # Verify the compiler builtin path on a small source file before
        # spending minutes compiling the 587k-line mrustc core translation.
        offset = len(serial_text())
        send_line(
            "tcc /mnt/src/tcc_overflow.c -o /mnt/tcc_overflow.elf"
        )
        if not wait_prompt(offset, 90) or not wait_for(
            "program exited with status 0", offset, 90
        ):
            print("TCC-OVERFLOW-COMPILE-FAIL")
            print(serial_text()[offset:][-4000:])
            return 1
        print("=== TCC OVERFLOW COMPILE ===")
        print(serial_text()[offset:][-2500:])

        offset = len(serial_text())
        send_line("run /mnt/tcc_overflow.elf")
        overflow_ok = wait_for("TCC-OVERFLOW-PASS", offset, 45)
        overflow_exit_ok = wait_for("program exited with status 0", offset, 45)
        overflow_prompt_ok = wait_prompt(offset, 45)
        print("=== TCC OVERFLOW RUN ===")
        print(serial_text()[offset:][-2500:])
        if not (overflow_ok and overflow_exit_ok and overflow_prompt_ok):
            print("TCC-OVERFLOW-FAIL")
            return 1
        print("TCC-OVERFLOW-PASS")

        # Compile and link the function, then run only main(). The function
        # containing XGETBV remains uncalled because OSXSAVE/XCR0 may be off.
        offset = len(serial_text())
        send_line("tcc /mnt/src/tcc_xgetbv.c -o /mnt/tcc_xgetbv.elf")
        if not wait_prompt(offset, 60) or not wait_for(
            "program exited with status 0", offset, 60
        ):
            print("TCC-XGETBV-ASSEMBLE-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        print("TCC-XGETBV-ASSEMBLE-PASS")

        offset = len(serial_text())
        send_line("run /mnt/tcc_xgetbv.elf")
        if not wait_prompt(offset, 30) or not wait_for(
            "program exited with status 0", offset, 30
        ):
            print("TCC-XGETBV-RUN-FAIL")
            print(serial_text()[offset:][-2000:])
            return 1
        print("TCC-XGETBV-RUN-PASS")

        # Ensure the freestanding math implementation is present in libc.a.
        offset = len(serial_text())
        send_line("tcc /mnt/src/tcc_math.c -o /mnt/tcc_math.elf")
        if not wait_prompt(offset, 90) or not wait_for(
            "program exited with status 0", offset, 90
        ):
            print("TCC-MATH-COMPILE-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        offset = len(serial_text())
        send_line("run /mnt/tcc_math.elf")
        if not wait_for("TCC-MATH-PASS", offset, 60):
            print("TCC-MATH-RUN-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        if not wait_for("program exited with status 0", offset, 30):
            print("TCC-MATH-EXIT-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        if not wait_prompt(offset, 30):
            print("TCC-MATH-PROMPT-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        print("TCC-MATH-PASS")

        # Compile and run the bytewise AtomicU128 PoC shim with TinyCC.
        offset = len(serial_text())
        send_line(
            "tcc -include /mnt/src/fix.h /mnt/src/tcc_atomic128_shim.c "
            "-o /mnt/tcc_atomic128_shim.elf"
        )
        if not wait_prompt(offset, 90) or not wait_for(
            "program exited with status 0", offset, 90
        ):
            print("TCC-ATOMIC128-SHIM-COMPILE-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        offset = len(serial_text())
        send_line("run /mnt/tcc_atomic128_shim.elf")
        if not wait_for("TCC-ATOMIC128-SHIM-PASS", offset, 60):
            print("TCC-ATOMIC128-SHIM-RUN-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        if not wait_for("program exited with status 0", offset, 30):
            print("TCC-ATOMIC128-SHIM-EXIT-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        if not wait_prompt(offset, 30):
            print("TCC-ATOMIC128-SHIM-PROMPT-FAIL")
            print(serial_text()[offset:][-3000:])
            return 1
        print("TCC-ATOMIC128-SHIM-PASS")

        offset = len(serial_text())
        send_line(
            "tcc -I /mnt/src -include /mnt/src/fix.h /mnt/src/shim.c "
            "/mnt/src/poc.c /mnt/src/libcore.c -o /mnt/poc.elf"
        )
        if not wait_prompt(offset, COMPILE_TIMEOUT) or not wait_for(
            "program exited with status 0", offset, COMPILE_TIMEOUT
        ):
            print("TCC-COMPILE-FAIL")
            print(serial_text()[offset:][-4000:])
            return 1
        print("=== TCC OUTPUT ===")
        print(serial_text()[offset:][-4000:])

        offset = len(serial_text())
        send_line("run /mnt/poc.elf")
        ok = wait_for("mrustc-c-poc: 108", offset, 120)
        exit_ok = wait_for("program exited with status 0", offset, 120)
        prompt_ok = wait_prompt(offset, 120)
        print("=== RUN OUTPUT ===")
        print(serial_text()[offset:][-2000:])
        if not (ok and exit_ok and prompt_ok):
            print("MRUSTC-C-POC-FAIL")
            return 1

        print("MRUSTC-C-POC-PASS")
        return 0
    finally:
        if connected and monitor is not None:
            try:
                monitor.sendall(b"quit\n")
            except OSError:
                pass
        if monitor is not None:
            monitor.close()
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        if qemu_log_handle is not None:
            qemu_log_handle.close()
        for path in (MONITOR, live_vars):
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass


if __name__ == "__main__":
    sys.exit(main())
