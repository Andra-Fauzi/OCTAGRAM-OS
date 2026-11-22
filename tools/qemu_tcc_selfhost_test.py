import hashlib
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
    ">": "shift-dot",
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
    send_line("run /tmp/tcc-selfhost-full.elf")
    if not wait_for_after("TCC runs on OCTAGRAM: 24 24", output_offset, 120):
        print("TCC-FULL-RUN-FAIL: extended libc smoke ELF did not reach its success output")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("TCC-FULL-RUN-FAIL: extended libc smoke ELF did not exit with status 0")
        print(serial_text()[-5000:])
        return 1

    # Exercise GCC-compatible integer overflow builtins through the guest
    # TinyCC parser and the support helper linked from libtcc1.a.
    output_offset = len(serial_text())
    send_line("tcc /tmp/tcc-overflow.c -o /tmp/tcc-overflow.elf")
    if not wait_for_after("program exited with status 0", output_offset, 90):
        print("TCC-OVERFLOW-COMPILE-FAIL: integer overflow builtins did not compile")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /tmp/tcc-overflow.elf")
    if not wait_for_after("TCC-OVERFLOW-PASS", output_offset, 60):
        print("TCC-OVERFLOW-RUN-FAIL: signed/unsigned overflow cases failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 15):
        print("TCC-OVERFLOW-RUN-FAIL: overflow test did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    # Parse the actual AtomicU128 PoC helper and the exact pointer-based
    # rewrite shape with TinyCC; this is a bytewise unreachable-path shim,
    # not an atomic primitive.
    output_offset = len(serial_text())
    send_line(
        "tcc -include /tmp/tcc-atomic128-fix.h "
        "/tmp/tcc-atomic128-shim.c -o /tmp/tcc-atomic128-shim.elf"
    )
    if not wait_for_after("program exited with status 0", output_offset, 90):
        print("TCC-ATOMIC128-SHIM-COMPILE-FAIL: TinyCC could not compile the shim")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /tmp/tcc-atomic128-shim.elf")
    if not wait_for_after("TCC-ATOMIC128-SHIM-PASS", output_offset, 60):
        print("TCC-ATOMIC128-SHIM-RUN-FAIL: bytewise shim checks failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 15):
        print("TCC-ATOMIC128-SHIM-RUN-FAIL: helper test did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    # Compile only: XGETBV can raise #UD when the guest CPU/OS has not
    # enabled OSXSAVE, so never execute this function as part of the smoke test.
    output_offset = len(serial_text())
    send_line("tcc /tmp/tcc-xgetbv.c -o /tmp/tcc-xgetbv.elf")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("TCC-XGETBV-ASSEMBLE-FAIL: TinyCC could not assemble XGETBV")
        print(serial_text()[-5000:])
        return 1
    output_offset = len(serial_text())
    send_line("run /tmp/tcc-xgetbv.elf")
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("TCC-XGETBV-RUN-FAIL: compile-only XGETBV container ELF did not run")
        print(serial_text()[-5000:])
        return 1

    # Verify that the real freestanding math object is available through libc.a.
    output_offset = len(serial_text())
    send_line("tcc /tmp/tcc-math.c -o /tmp/tcc-math.elf")
    if not wait_for_after("program exited with status 0", output_offset, 90):
        print("TCC-MATH-COMPILE-FAIL: guest math program did not link against libc.a")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /tmp/tcc-math.elf")
    if not wait_for_after("TCC-MATH-PASS", output_offset, 60):
        print("TCC-MATH-RUN-FAIL: guest math regression cases failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 15):
        print("TCC-MATH-RUN-FAIL: guest math program did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    # Exercise the shell's native ELF linker with two independent TinyCC
    # objects and a cross-object PLT32/PC32 call relocation.
    for command, label in (
        ("tcc -c /share/octagram/ld-main.c -o /tmp/ld-main.o", "LD-MAIN-COMPILE"),
        ("tcc -c /share/octagram/ld-helper.c -o /tmp/ld-helper.o", "LD-HELPER-COMPILE"),
    ):
        output_offset = len(serial_text())
        send_line(command)
        if not wait_for_after("program exited with status 0", output_offset, 60):
            print("%s-FAIL: TinyCC did not produce its relocatable object" % label)
            print(serial_text()[-5000:])
            return 1

    output_offset = len(serial_text())
    send_line("ld /tmp/ld-main.o /tmp/ld-helper.o -o /tmp/ld-multi.elf")
    if not wait_for_after("ld: wrote", output_offset, 60):
        print("LD-MULTI-LINK-FAIL: native ld rejected multiple ELF objects")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("ld: entry point", output_offset, 5):
        print("LD-MULTI-LINK-FAIL: native ld did not report an ELF entry point")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /tmp/ld-multi.elf")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("LD-MULTI-RUN-FAIL: multi-object ELF did not exit successfully")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("tcc -ar rcs /tmp/libld.a /tmp/ld-helper.o")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("LD-ARCHIVE-CREATE-FAIL: TinyCC did not create the static archive")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("ld /tmp/ld-main.o /tmp/libld.a -o /tmp/ld-archive.elf")
    if not wait_for_after("ld: entry point", output_offset, 60):
        print("LD-ARCHIVE-LINK-FAIL: native ld did not extract the required archive member")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /tmp/ld-archive.elf")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("LD-ARCHIVE-RUN-FAIL: executable linked from a static archive did not exit successfully")
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
    send_line("make -C /share/octagram -n automatic-vars-check")
    if not wait_for_after("echo automatic:automatic-vars-check first:hello.c newer:hello.c order:order-only-dep", output_offset, 30):
        print("MAKE-AUTOMATIC-VARS-FAIL: $@, $<, $?, or $| expanded incorrectly")
        print(serial_text()[-5000:])
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
    send_line("make -C /share/octagram order-only-check-run")
    for command in (
        "tcc -c message.c -o order-only-dep",
        "tcc -c message.c -o order-only-check",
    ):
        if not wait_for_after(command, output_offset, 30):
            print("MAKE-ORDER-ONLY-FAIL: initial order-only dependency build failed")
            print(serial_text()[-5000:])
            return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("MAKE-ORDER-ONLY-FAIL: initial order-only target build failed")
        print(serial_text()[-5000:])
        return 1

    time.sleep(1.2)
    touch_offset = len(serial_text())
    send_line("touch /share/octagram/order-only-dep")
    if not wait_for_after("octagram:/ $", touch_offset, 15):
        print("MAKE-ORDER-ONLY-FAIL: could not make order-only prerequisite newer")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram order-only-check-run")
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("MAKE-ORDER-ONLY-FAIL: second order-only target invocation failed")
        print(serial_text()[-5000:])
        return 1
    order_only_output = serial_text()[output_offset:]
    if "tcc -c message.c -o order-only-check" in order_only_output:
        print("MAKE-ORDER-ONLY-FAIL: newer order-only prerequisite incorrectly rebuilt target")
        print(order_only_output[-3000:])
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

    # The native bootstrap Make can emit a machine-readable build graph
    # report. Verify both file creation and the persisted target/dependency
    # status by reading it back through the guest shell.
    output_offset = len(serial_text())
    send_line("make -C /share/octagram --manifest /share/octagram/native-build.manifest")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("MAKE-MANIFEST-FAIL: native Make could not write its build manifest")
        print(serial_text()[-6000:])
        return 1

    output_offset = len(serial_text())
    send_line("cat /share/octagram/native-build.manifest")
    if not wait_for_after("hello.elf\tup-to-date", output_offset, 30):
        print("MAKE-MANIFEST-FAIL: manifest omitted the up-to-date executable record")
        print(serial_text()[-6000:])
        return 1
    manifest_output = serial_text()[output_offset:]
    if "OCTAGRAM-BUILD-MANIFEST\t2" not in manifest_output or "hello.o libmessage.a" not in manifest_output:
        print("MAKE-MANIFEST-FAIL: manifest header or dependency graph is incomplete")
        print(manifest_output[-6000:])
        return 1
    manifest_lines = manifest_output.splitlines()
    if "# target\tstatus\tmtime_unix\tsize_bytes\tsha256\tdependencies" not in manifest_lines:
        print("MAKE-MANIFEST-FAIL: manifest is missing its size column")
        print(manifest_output[-6000:])
        return 1
    hello_record = next((line.split("\t") for line in manifest_lines if line.startswith("hello.elf\t")), None)
    if hello_record is None or len(hello_record) != 6 or not hello_record[3].isdigit() or len(hello_record[4]) != 64 or hello_record[5] != "hello.o libmessage.a":
        print("MAKE-MANIFEST-FAIL: executable size/dependency record is malformed")
        print(manifest_output[-6000:])
        return 1
    print("MAKE-MANIFEST-OCTAGRAM-PASS: target status, mtime, size, and dependency graph verified")

    # Verify the digest implementation against the published SHA-256 of an
    # empty file. This catches padding/length and hex-encoding mistakes.
    output_offset = len(serial_text())
    send_line("touch /tmp/octagram-sha256-probe")
    if not wait_for_after("octagram:/ $", output_offset, 15):
        print("MAKE-SHA256-FAIL: could not create the empty digest probe")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram --manifest /tmp/octagram-sha256.manifest /tmp/octagram-sha256-probe")
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("MAKE-SHA256-FAIL: manifest generation for digest probe failed")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("cat /tmp/octagram-sha256.manifest")
    expected_empty_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    if not wait_for_after(expected_empty_sha256, output_offset, 30):
        print("MAKE-SHA256-FAIL: empty-file SHA-256 does not match the known digest")
        print(serial_text()[-5000:])
        return 1
    print("MAKE-SHA256-OCTAGRAM-PASS: streaming SHA-256 matches the known empty-file digest")

    # Exercise a non-empty multi-block SHA-256 input. The known digest is
    # calculated by the host test runner, while OCTAGRAM hashes the file
    # through repeated 1 KiB read syscalls.
    sha_payload = "a" * 100
    sha_expected = hashlib.sha256((sha_payload + "\n").encode()).hexdigest()
    output_offset = len(serial_text())
    send_line("echo %s > /tmp/octagram-sha256-multiblock" % sha_payload)
    if not wait_for_after("octagram:/ $", output_offset, 30):
        print("MAKE-SHA256-MULTIBLOCK-FAIL: could not write the non-empty fixture")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("make -C /share/octagram --manifest /tmp/octagram-sha256-multiblock.manifest /tmp/octagram-sha256-multiblock")
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("MAKE-SHA256-MULTIBLOCK-FAIL: manifest generation failed")
        print(serial_text()[-5000:])
        return 1

    output_offset = len(serial_text())
    send_line("cat /tmp/octagram-sha256-multiblock.manifest")
    if not wait_for_after(sha_expected, output_offset, 30):
        print("MAKE-SHA256-MULTIBLOCK-FAIL: non-empty multi-block digest differs from host SHA-256")
        print("expected: %s" % sha_expected)
        print(serial_text()[-5000:])
        return 1
    print("MAKE-SHA256-MULTIBLOCK-PASS: non-empty multi-block digest matches host hashlib")



    # Run the upstream GNU Make 4.4.1 binary, separately from the small
    # bootstrap make currently installed at /bin/make.
    output_offset = len(serial_text())
    send_line("/usr/bin/gmake --version")
    if not wait_for_after("GNU Make 4.4.1", output_offset, 30):
        print("GNU-MAKE-VERSION-FAIL: upstream GNU Make did not start")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("GNU-MAKE-VERSION-FAIL: --version did not exit successfully")
        print(serial_text()[-6000:])
        return 1

    # The bootstrap make tests above already produced all outputs. Advance
    # the source timestamps so upstream GNU Make has real work to plan instead
    # of correctly reporting that the default target is already up to date.
    # `touch` is a shell builtin, so success is signalled by the next prompt,
    # not by the external-program exit-status message.
    for source in ("hello.c", "message.c"):
        output_offset = len(serial_text())
        send_line("touch /share/octagram/%s" % source)
        if not wait_for_after("octagram:/ $", output_offset, 30):
            print("GNU-MAKE-SETUP-FAIL: could not refresh %s before dry-run" % source)
            print(serial_text()[-6000:])
            return 1

    output_offset = len(serial_text())
    send_line("/usr/bin/gmake -C /share/octagram -n")
    if not wait_for_after("tcc hello.o libmessage.a -o hello.elf", output_offset, 60):
        print("GNU-MAKE-DRY-RUN-FAIL: upstream GNU Make did not parse the native Makefile")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("GNU-MAKE-DRY-RUN-FAIL: upstream GNU Make dry-run failed")
        print(serial_text()[-6000:])
        return 1

    output_offset = len(serial_text())
    send_line("/usr/bin/gmake -B -C /share/octagram")
    gnu_make_recipes = (
        "tcc -c hello.c -o hello.o",
        "tcc -c message.c -o message.o",
        "tcc -ar rcs libmessage.a message.o",
        "tcc hello.o libmessage.a -o hello.elf",
    )
    if not all(wait_for_after(command, output_offset, 60) for command in gnu_make_recipes):
        print("GNU-MAKE-BUILD-FAIL: GNU Make did not execute all TinyCC recipes")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 180):
        print("GNU-MAKE-BUILD-FAIL: upstream GNU Make build did not exit successfully")
        print(serial_text()[-6000:])
        return 1

    output_offset = len(serial_text())
    send_line("run /share/octagram/hello.elf")
    if not wait_for_after("Built inside OCTAGRAM!", output_offset, 60):
        print("GNU-MAKE-RUN-FAIL: executable produced by GNU Make did not run")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("GNU-MAKE-RUN-FAIL: executable produced by GNU Make returned failure")
        print(serial_text()[-6000:])
        return 1

    # Exercise GNU Make's own automatic-variable expansion rather than only
    # the bootstrap make implementation.
    output_offset = len(serial_text())
    send_line("/usr/bin/gmake -C /share/octagram -n automatic-vars-check")
    if not wait_for_after("echo automatic:automatic-vars-check first:hello.c newer:hello.c order:order-only-dep", output_offset, 60):
        print("GNU-MAKE-AUTOMATIC-VARS-FAIL: GNU Make expanded automatic variables incorrectly")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("GNU-MAKE-AUTOMATIC-VARS-FAIL: automatic variable dry-run failed")
        print(serial_text()[-6000:])
        return 1

    # A phony target must run on every invocation, even if a file with the
    # same name exists in the working directory.
    for attempt in range(2):
        output_offset = len(serial_text())
        send_line("/usr/bin/gmake -C /share/octagram phony-check")
        if not wait_for_after("tcc -c message.c -o phony-check", output_offset, 60):
            print("GNU-MAKE-PHONY-FAIL: GNU Make skipped a declared phony target")
            print(serial_text()[-6000:])
            return 1
        if not wait_for_after("program exited with status 0", output_offset, 60):
            print("GNU-MAKE-PHONY-FAIL: phony recipe did not exit successfully")
            print(serial_text()[-6000:])
            return 1

    output_offset = len(serial_text())
    send_line("/usr/bin/gmake -C /share/octagram order-only-check-run")
    if not wait_for_after("tcc -c message.c -o order-only-check", output_offset, 60):
        print("GNU-MAKE-ORDER-ONLY-FAIL: initial order-only target recipe did not run")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("GNU-MAKE-ORDER-ONLY-FAIL: initial order-only build failed")
        print(serial_text()[-6000:])
        return 1

    time.sleep(1.2)
    touch_offset = len(serial_text())
    send_line("touch /share/octagram/order-only-dep")
    if not wait_for_after("octagram:/ $", touch_offset, 30):
        print("GNU-MAKE-ORDER-ONLY-FAIL: could not refresh order-only prerequisite")
        print(serial_text()[-6000:])
        return 1

    output_offset = len(serial_text())
    send_line("/usr/bin/gmake -C /share/octagram order-only-check-run")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("GNU-MAKE-ORDER-ONLY-FAIL: second order-only invocation failed")
        print(serial_text()[-6000:])
        return 1
    order_output = serial_text()[output_offset:]
    if "tcc -c message.c -o order-only-check" in order_output:
        print("GNU-MAKE-ORDER-ONLY-FAIL: newer order-only prerequisite rebuilt its target")
        print(order_output[-4000:])
        return 1

    # Repeating the default target must be a no-op when every output is fresh.
    output_offset = len(serial_text())
    send_line("/usr/bin/gmake -C /share/octagram")
    if not wait_for_after("program exited with status 0", output_offset, 60):
        print("GNU-MAKE-INCREMENTAL-FAIL: second GNU Make invocation failed")
        print(serial_text()[-6000:])
        return 1
    incremental_output = serial_text()[output_offset:]
    if any(command in incremental_output for command in (
        "tcc -c hello.c -o hello.o",
        "tcc -c message.c -o message.o",
        "tcc -ar rcs libmessage.a message.o",
        "tcc hello.o libmessage.a -o hello.elf",
    )):
        print("GNU-MAKE-INCREMENTAL-FAIL: up-to-date target unexpectedly rebuilt")
        print(incremental_output[-4000:])
        return 1

    print("GNU-MAKE-OCTAGRAM-PASS: upstream GNU Make 4.4.1 version, dry-run, automatic variables, phony/order-only, incremental build, forced TinyCC build, and guest ELF execution passed")
    print("TCC-SELFHOST-PASS: TinyCC compile/run, Make build, up-to-date skip, newer-prerequisite rebuild, and -B force passed")
    print(serial_text()[-4000:])
    return 0


if __name__ == "__main__":
    sys.exit(main())
