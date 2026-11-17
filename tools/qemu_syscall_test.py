import os
import socket
import sys
import time

MON = "/tmp/mon.sock"
SERIAL = "/tmp/serial.log"

KEY_NAMES = {
    "/": "slash",
    "-": "minus",
    ".": "dot",
    "_": "shift-minus",
    " ": "spc",
    "\n": "ret",
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
        time.sleep(0.5)
    return False


def wait_prompt(timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if "octagram:" in serial_text() and serial_text().rstrip().endswith("$"):
            return True
        time.sleep(0.5)
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
    time.sleep(0.5)
    try:
        monitor.recv(4096)
    except OSError:
        pass

    def send_line(text):
        for character in text:
            key = KEY_NAMES.get(character, character)
            monitor.sendall(("sendkey %s\n" % key).encode())
            time.sleep(0.06)
        monitor.sendall(b"sendkey ret\n")
        time.sleep(0.5)

    send_line("run /bin/syscall-compat-demo")

    if not wait_for("type a line", 120):
        print("TEST-FAIL: demo never asked for tty input")
        print(serial_text()[-4000:])
        return 1

    send_line("hello")

    if not wait_for("SYSCALL-TEST", 180):
        print("TEST-FAIL: no result marker")
        print(serial_text()[-4000:])
        return 1

    time.sleep(1)
    print("=== TEST OUTPUT ===")
    text = serial_text()
    start = text.find("ok   uname() == 0")
    if start < 0:
        start = max(0, len(text) - 4000)
    print(text[start:])
    return 0


if __name__ == "__main__":
    sys.exit(main())
