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
    "'": "apostrophe",
    ">": "shift-dot",
    ";": "semicolon",
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


def wait_for_after(text, offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        if text in serial_text()[offset:]:
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
            key = KEY_NAMES.get(character)
            if key is None:
                key = "shift-" + character.lower() if character.isupper() else character
            monitor.sendall(("sendkey %s\n" % key).encode())
            time.sleep(0.06)
        monitor.sendall(b"sendkey ret\n")
        time.sleep(0.5)

    syscall_output_offset = len(serial_text())
    send_line("run /bin/syscall-compat-demo")

    if not wait_for("type a line", 120):
        print("TEST-FAIL: demo never asked for tty input")
        print(serial_text()[-4000:])
        return 1

    send_line("hello")

    if not wait_for_after("SYSCALL-TEST", syscall_output_offset, 180):
        print("TEST-FAIL: no result marker")
        print(serial_text()[-4000:])
        return 1
    if not wait_for_after("program exited with status 0", syscall_output_offset, 30):
        print("TEST-FAIL: syscall compatibility program did not exit successfully")
        output = serial_text()[syscall_output_offset:]
        failures = [line for line in output.splitlines() if "FAIL " in line]
        if failures:
            print("SYSCALL-FAILURES (%d):" % len(failures))
            for failure in failures:
                print(failure)
        else:
            print("SYSCALL-FAILURES: no individual FAIL line captured")
        print(output[-4000:])
        return 1
    if not wait_prompt(30):
        print("TEST-FAIL: shell did not return after syscall compatibility program")
        print(serial_text()[-4000:])
        return 1

    # Exercise the first native userspace shell through the same VFS path
    # that GNU Make will use as $(SHELL). This validates -c, fork/exec, pipes,
    # output redirection, append redirection, and the shell's cat builtin.
    shell_offset = len(serial_text())
    send_line("run /bin/sh -c 'echo SH-NATIVE-PASS'")
    if not wait_for_after("SH-NATIVE-PASS", shell_offset, 30):
        print("SH-NATIVE-FAIL: /bin/sh -c did not execute its builtin")
        print(serial_text()[-4000:])
        return 1
    if not wait_for_after("program exited with status 0", shell_offset, 30):
        print("SH-NATIVE-FAIL: simple command returned nonzero")
        print(serial_text()[-4000:])
        return 1
    if not wait_prompt(30):
        print("SH-NATIVE-FAIL: kernel shell did not return")
        return 1
    print("SH-NATIVE-COMMAND-PASS")

    shell_offset = len(serial_text())
    send_line("run /bin/sh -c 'echo SH-PIPE-PASS | cat'")
    if not wait_for_after("SH-PIPE-PASS", shell_offset, 30):
        print("SH-PIPE-FAIL: pipeline did not pass stdin to cat")
        print(serial_text()[-4000:])
        return 1
    if not wait_for_after("program exited with status 0", shell_offset, 30):
        print("SH-PIPE-FAIL: pipeline returned nonzero")
        print(serial_text()[-4000:])
        return 1
    if not wait_prompt(30):
        print("SH-PIPE-FAIL: kernel shell did not return")
        return 1
    print("SH-PIPE-PASS")

    shell_offset = len(serial_text())
    send_line("run /bin/sh -c 'echo SH-REDIR-PASS > /tmp/sh-native-test; echo SH-APPEND-PASS >> /tmp/sh-native-test'")
    if not wait_for_after("program exited with status 0", shell_offset, 30):
        print("SH-REDIR-FAIL: redirection command returned nonzero")
        print(serial_text()[-4000:])
        return 1
    if not wait_prompt(30):
        print("SH-REDIR-FAIL: kernel shell did not return")
        return 1
    send_line("cat /tmp/sh-native-test")
    if not wait_for_after("SH-REDIR-PASS", shell_offset, 30) or not wait_for_after("SH-APPEND-PASS", shell_offset, 30):
        print("SH-REDIR-FAIL: output or append redirection was not persisted")
        print(serial_text()[-4000:])
        return 1
    print("SH-REDIR-PASS")

    # Rust host bootstrap depends on these OS services. These binaries were
    # already part of the image, but compiling them on the host is not enough:
    # execute them in the guest and require their success markers and exit code.
    guest_tests = (
        ("/bin/mmap-demo", "mmap POSIX demo: PASS", "M4-ABI-MMAP"),
        ("/bin/tls-demo", "static TLS initialized", "M4-ABI-TLS"),
        ("/bin/thread-demo", "preemptive threads, TLS, and SSE context passed", "M4-ABI-THREADS"),
        (
            "/bin/rust-hello-octagram",
            "Rust custom target x86_64-unknown-octagram runs in OCTAGRAM!",
            "M4-RUST-TARGET-EXEC",
        ),
        ("/bin/execve-demo", "EXECVP-PATH-SEARCH-PASS", "LIBC-EXECVP-PATH-SEARCH"),
        ("/bin/posix_spawn_demo", "posix_spawn environment/status/WNOHANG/file actions/chdir/setsid/signal attributes/scheduling/resetids/spawnp passed", "POSIX-SPAWN-PIPE-FD-PATH"),
    )
    for command, marker, label in guest_tests:
        output_offset = len(serial_text())
        send_line("run " + command)
        if not wait_for_after(marker, output_offset, 120):
            print(label + "-FAIL: guest program did not emit its success marker")
            print(serial_text()[-5000:])
            return 1
        if not wait_for_after("program exited with status 0", output_offset, 120):
            print(label + "-FAIL: guest program did not exit with status 0")
            print(serial_text()[-5000:])
            return 1
        if not wait_prompt(30):
            print(label + "-FAIL: shell did not return after guest program")
            print(serial_text()[-5000:])
            return 1
        print(label + "-PASS")

    # This binary is Rust no_std + alloc, cross-compiled on the host but
    # executed as a native OCTAGRAM ELF. It is not rustc-host self-hosting.
    output_offset = len(serial_text())
    send_line("run /bin/rust-hello")
    if not wait_for_after("Rust custom target x86_64-unknown-octagram runs in OCTAGRAM!", output_offset, 120):
        print("RUST-NOSTD-FAIL: Rust no_std + alloc program did not emit its marker")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust allocator mmap/munmap cycle: PASS", output_offset, 30):
        print("RUST-ALLOCATOR-FAIL: allocation/drop/alignment/reallocation cycle failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime file/stat: PASS", output_offset, 30):
        print("RUST-FILE-ABI-FAIL: Rust runtime open/read/stat/close test failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime directory iteration: PASS", output_offset, 30):
        print("RUST-DIRENT-ABI-FAIL: Rust opendir/readdir/closedir test failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime clock/random/pid: PASS", output_offset, 30):
        print("RUST-OS-ABI-FAIL: clock_gettime/getrandom/getpid runtime test failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime child process/waitpid: PASS", output_offset, 30):
        print("RUST-PROCESS-FAIL: Rust fork/waitpid child ownership test failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime Command::output: PASS", output_offset, 30):
        print("RUST-COMMAND-OUTPUT-FAIL: Command.output stdout capture/reap failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime Command stdout capture: PASS", output_offset, 30):
        print("RUST-STDIO-FAIL: Rust Command stdout capture via owned pipe failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime Command exec handshake: PASS", output_offset, 30):
        print("RUST-COMMAND-FAIL: Rust Command exec success/failure handshake failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("Rust runtime VM/pipe/futex/thread: PASS", output_offset, 30):
        print("RUST-HOST-ABI-FAIL: mmap/mprotect/munmap/pipe2/dup3/futex/thread runtime test failed")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", output_offset, 30):
        print("RUST-NOSTD-FAIL: Rust no_std + alloc program did not exit with status 0")
        print(serial_text()[-5000:])
        return 1
    if not wait_prompt(30):
        print("RUST-NOSTD-FAIL: shell did not return after Rust program")
        print(serial_text()[-5000:])
        return 1
    print("RUST-NOSTD-PASS")

    # This executable is built from upstream rust-src std, linked for the
    # OCTAGRAM ABI, embedded into the kernel image, and exercised in the guest.
    std_offset = len(serial_text())
    send_line("run /bin/rust-std-probe")
    for marker, label in (
        ("RUST-STD-OS-STRING-FFI-PASS", "Unix OS string byte preservation"),
        ("RUST-STD-PATH-OS-STRING-FFI-PASS", "PathBuf OS string byte preservation"),
        ("RUST-STD-PATH-LEXICAL-PASS", "lexical path components and joins"),
        ("RUST-STD-FS-BYTE-PATH-PASS", "non-UTF-8 filesystem paths"),
        ("RUST-STD-READ-DIR-BYTE-NAME-PASS", "non-UTF-8 directory entry names"),
        ("RUST-STD-FS-DIRECTORY-PASS", "directory rename and file descriptor I/O"),
        ("RUST-STD-READ-DIR-NESTED-PASS", "nested directory iteration"),
        ("RUST-STD-FS-TRUNCATE-PASS", "file descriptor truncation and data sync"),
        ("RUST-STD-FS-ATOMIC-RENAME-PASS", "atomic artifact replacement"),
        ("RUST-STD-REMOVE-DIR-ALL-RECURSIVE-PASS", "recursive directory removal through fdopendir/readdir"),
        ("RUST-STD-FS-PATH-MANAGEMENT-PASS", "nested directories and current directory"),
        ("RUST-STD-AT-DIRFD-PASS", "dirfd-relative openat/mkdirat/renameat/unlinkat"),
        ("RUST-STD-FS-SYMLINK-PASS", "upstream std symlink, read_link and symlink_metadata"),
        ("RUST-STD-FS-PERMISSIONS-PASS", "upstream std Unix file permission metadata and updates"),
        ("RUST-STD-ENV-PASS", "environment"),
        ("RUST-STD-ENV-MUTATION-PASS", "process environment set_var and remove_var"),
        ("RUST-STD-FS-METADATA-PASS", "filesystem metadata"),
        ("RUST-STD-READ-DIR-PASS", "directory iteration"),
        ("RUST-STD-THREAD-CLOCK-PASS", "thread and monotonic clock"),
        ("RUST-STD-PROCESS-PASS", "process spawn and output capture"),
        ("RUST-STD-PROCESS-ARGV-IO-PASS", "child argv forwarding, stderr capture and exit status"),
        ("RUST-STD-PROCESS-SIGNAL-STATUS-PASS", "POSIX signal termination wait status"),
        ("RUST-STD-PROCESS-CHILD-ID-PASS", "Rust Child::id reports the spawned process PID"),
        ("RUST-STD-PROCESS-TRY-WAIT-KILL-PASS", "Rust child try_wait, kill, and wait lifecycle"),
        ("RUST-STD-PROCESS-WAIT-CACHE-PASS", "Rust child repeated wait and cached try_wait status"),
        ("RUST-STD-PROCESS-ENV-CWD-PASS", "child process environment and working directory"),
        ("RUST-STD-PROCESS-PATH-LOOKUP-PASS", "Rust Command resolves a bare child executable using PATH"),
        ("RUST-STD-IO-WRITE-PASS", "standard output"),
        ("RUST-STD-RUNTIME-PASS", "upstream std runtime"),
    ):
        if not wait_for_after(marker, std_offset, 45):
            print("RUST-STD-" + label.upper().replace(" ", "-") + "-FAIL: expected marker missing")
            print(serial_text()[-7000:])
            return 1
    if not wait_for_after("program exited with status 0", std_offset, 45):
        print("RUST-STD-RUNTIME-FAIL: upstream std probe did not exit successfully")
        print(serial_text()[-7000:])
        return 1
    if not wait_prompt(30):
        print("RUST-STD-RUNTIME-FAIL: shell did not return after upstream std probe")
        print(serial_text()[-7000:])
        return 1
    print("RUST-STD-QEMU-PASS")

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
