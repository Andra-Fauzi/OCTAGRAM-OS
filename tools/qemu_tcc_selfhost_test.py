import os
import socket
import sys
import time

MON = os.environ.get("MON", "/tmp/mon-tcc.sock")
SERIAL = os.environ.get("SERIAL", "/tmp/serial-tcc.log")

KEY_NAMES = {
    "/": "slash",
    "-": "minus",
    ".": "dot",
    "_": "shift-minus",
    " ": "spc",
}


def serial_text():
    try:
        with open(SERIAL, "r", errors="replace") as handle:
            return handle.read()
    except FileNotFoundError:
        return ""


def wait_for(text, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text in serial_text():
            return True
        time.sleep(0.25)
    return False


def wait_for_after(text, offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text in serial_text()[offset:]:
            return True
        time.sleep(0.25)
    return False


def main():
    if not wait_for("octagram:", 240):
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
        print("MONITOR-FAIL: cannot connect")
        return 1

    monitor.settimeout(2)
    try:
        monitor.recv(4096)
    except OSError:
        pass

    def send_line(command):
        for character in command:
            key = KEY_NAMES.get(character, character)
            monitor.sendall(("sendkey %s\n" % key).encode())
            time.sleep(0.06)
        monitor.sendall(b"sendkey ret\n")
        time.sleep(0.5)

    send_line("tcc /tmp/tcc.c -o /tmp/tcc-selfhost-smoke.elf")
    if not wait_for("program exited with status 0", 120):
        print("TCC-COMPILE-FAIL: native TinyCC did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    send_line("run /tmp/tcc-selfhost-smoke.elf")
    if not wait_for("TCC runs on OCTAGRAM: 24 24", 120):
        print("TCC-RUN-FAIL: compiled guest ELF output was not observed")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram -n")
    if not wait_for_after("tcc hello.c -o hello.elf", output_offset, 30):
        print("MAKE-DRY-RUN-FAIL: native make did not expand the selfhost recipe")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram -j1 -n")
    if not wait_for_after("tcc hello.c -o hello.elf", output_offset, 30):
        print("MAKE-SERIAL-FAIL: native make rejected -j1")
        print(serial_text()[-5000:])
        return 1

    print("TCC-SELFHOST-PASS: native TinyCC compiled and ran a guest ELF")
    print(serial_text()[-3000:])
    return 0


if __name__ == "__main__":
    sys.exit(main())
