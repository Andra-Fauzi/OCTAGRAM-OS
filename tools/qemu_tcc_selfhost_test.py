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
    "B": "shift-b",
    "C": "shift-c",
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

    output_offset = len(serial_text())
    send_line("tcc /tmp/tcc.c -o /tmp/tcc-selfhost-minimal.elf")
    if not wait_for_after("program exited with status 0", output_offset, 120):
        print("TCC-COMPILE-FAIL: minimal native TinyCC compile did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /tmp/tcc-selfhost-minimal.elf")
    if not wait_for_after("TCC runs on OCTAGRAM: 24 24", output_offset, 120):
        print("TCC-RUN-FAIL: minimal compiled guest ELF output was not observed")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("tcc /tmp/tcc-full.c -o /tmp/tcc-selfhost-full.elf")
    if not wait_for_after("program exited with status 0", output_offset, 120):
        print("TCC-FULL-COMPILE-FAIL: extended libc smoke source did not compile")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram -n")
    if not wait_for_after("tcc hello.o libmessage.a -o hello.elf", output_offset, 30):
        print("MAKE-DRY-RUN-FAIL: native make did not expand the object-file link recipe")
        print(serial_text()[-5000:])
        return 1
    dry_run_output = serial_text()[output_offset:]
    required_dry_run = (
        "tcc -c hello.c -o hello.o",
        "tcc -c message.c -o message.o",
        "tcc -ar rcs libmessage.a message.o",
        "tcc hello.o libmessage.a -o hello.elf",
    )
    if not all(command in dry_run_output for command in required_dry_run):
        print("MAKE-DRY-RUN-FAIL: native make omitted an object, archive, or link recipe")
        print(dry_run_output[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram -j1 -n")
    if not wait_for_after("tcc hello.o libmessage.a -o hello.elf", output_offset, 30):
        print("MAKE-SERIAL-FAIL: native make rejected -j1")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram")
    if not wait_for_after("program exited with status 0", output_offset, 120):
        print("MAKE-BUILD-FAIL: native Make did not execute the TinyCC recipe")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("tcc hello.o libmessage.a -o hello.elf", output_offset, 5):
        print("MAKE-BUILD-FAIL: expected TinyCC recipe was not executed")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /share/octagram/hello.elf")
    if not wait_for_after("Built inside OCTAGRAM!", output_offset, 120):
        print("MAKE-ELF-RUN-FAIL: executable built by native Make did not run")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 5):
        print("MAKE-ELF-RUN-FAIL: built ELF did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    touch_offset = len(serial_text())
    send_line("tcc -c /share/octagram/message.c -o /share/octagram/phony-check")
    if not wait_for_after("program exited with status 0", touch_offset, 30):
        print("MAKE-PHONY-FAIL: could not pre-create a file matching the phony target")
        print(serial_text()[-5000:])
        return 1

    for attempt in range(2):
        output_offset = len(serial_text())
        send_line("make -C /share/octagram phony-check")
        if not wait_for_after("tcc -c message.c -o phony-check", output_offset, 30):
            print("MAKE-PHONY-FAIL: declared .PHONY recipe was skipped")
            print(serial_text()[-5000:])
            return 1
        if not wait_for_after("program exited with status 0", output_offset, 30):
            print("MAKE-PHONY-FAIL: phony recipe did not exit successfully")
            print(serial_text()[-5000:])
            return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram")
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("MAKE-INCREMENTAL-FAIL: second Make invocation did not exit successfully")
        print(serial_text()[-5000:])
        return 1
    incremental_output = serial_text()[output_offset:]
    if any(command in incremental_output for command in (
        "tcc -c hello.c -o hello.o",
        "tcc -c message.c -o message.o",
        "tcc -ar rcs libmessage.a message.o",
        "tcc hello.o libmessage.a -o hello.elf",
    )):
        print("MAKE-INCREMENTAL-FAIL: up-to-date object, archive, or executable recipe ran again")
        print(incremental_output[-5000:])
        return 1

    # EXT2 mtime has one-second resolution; cross a second before touching
    # the prerequisite so this test proves the newer-prerequisite path.
    time.sleep(1.2)
    touch_offset = len(serial_text())
    send_line("touch /share/octagram/hello.c")
    if not wait_for_after("octagram:/ $", touch_offset, 15):
        print("MAKE-PREREQUISITE-FAIL: touch did not return to the shell")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram")
    if not wait_for_after("tcc hello.o libmessage.a -o hello.elf", output_offset, 30):
        print("MAKE-PREREQUISITE-FAIL: newer source did not trigger a relink")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 120):
        print("MAKE-PREREQUISITE-FAIL: rebuild after touching source failed")
        print(serial_text()[-5000:])
        return 1
    hello_rebuild = serial_text()[output_offset:]
    if "tcc -c hello.c -o hello.o" not in hello_rebuild or "tcc -ar rcs libmessage.a message.o" in hello_rebuild:
        print("MAKE-PREREQUISITE-FAIL: touching hello.c rebuilt the wrong dependency set")
        print(hello_rebuild[-5000:])
        return 1

    time.sleep(1.2)
    touch_offset = len(serial_text())
    send_line("touch /share/octagram/message.c")
    if not wait_for_after("octagram:/ $", touch_offset, 15):
        print("MAKE-ARCHIVE-FAIL: touching message.c did not return to the shell")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram")
    archive_rebuild = (
        "tcc -c message.c -o message.o",
        "tcc -ar rcs libmessage.a message.o",
        "tcc hello.o libmessage.a -o hello.elf",
    )
    if not all(wait_for_after(command, output_offset, 30) for command in archive_rebuild):
        print("MAKE-ARCHIVE-FAIL: message.c did not rebuild its object, archive, and final ELF")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 120):
        print("MAKE-ARCHIVE-FAIL: archive dependency rebuild failed")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -B -C /share/octagram")
    forced_commands = (
        "tcc -c hello.c -o hello.o",
        "tcc -c message.c -o message.o",
        "tcc -ar rcs libmessage.a message.o",
        "tcc hello.o libmessage.a -o hello.elf",
    )
    if not all(wait_for_after(command, output_offset, 30) for command in forced_commands):
        print("MAKE-FORCE-FAIL: -B did not force every object/archive/link recipe")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 120):
        print("MAKE-FORCE-FAIL: forced rebuild did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    print("TCC-SELFHOST-PASS: TinyCC compile/run, Make build, up-to-date skip, newer-prerequisite rebuild, and -B force passed")
    print(serial_text()[-4000:])
    return 0


if __name__ == "__main__":
    sys.exit(main())
