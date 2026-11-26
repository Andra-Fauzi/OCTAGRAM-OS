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
    "|": "shift-backslash",
    "\\": "backslash",
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


def wait_for_exact_line_after(expected, offset, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        output = serial_text()[offset:].replace("\\r\\n", "\\n")
        if expected in output.splitlines():
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
    if not wait_for_exact_line_after("SH-PIPE-PASS", shell_offset, 30):
        print("SH-PIPE-FAIL: expected an exact marker line from echo | cat")
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
        ("/bin/thread-demo", "preemptive threads, TLS, SSE context, and detached pthreads passed", "M4-ABI-THREADS"),
        (
            "/bin/rust-hello-octagram",
            "Rust custom target x86_64-unknown-octagram runs in OCTAGRAM!",
            "M4-RUST-TARGET-EXEC",
        ),
        ("/bin/execve-demo", "EXECVP-PATH-SEARCH-PASS", "LIBC-EXECVP-PATH-SEARCH"),
        ("/bin/file-api-demo", "fstat/access/rename/umask/dup/fcntl/pread/pwrite passed", "POSIX-FCNTL-SHARED-APPEND"),
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
        ("RUST-STD-GETRANDOM-FLAGS-PASS", "getrandom flags and error validation"),
        ("RUST-STD-FIONBIO-PASS", "ioctl FIONBIO enables nonblocking pipe reads"),
        ("RUST-STD-SOCKET-EMPTY-IOV-PASS", "sendmsg/recvmsg zero-iovec validation"),
        ("RUST-STD-UNIX-SOCKETPAIR-MSG-PASS", "AF_UNIX sendmsg/recvmsg and MSG_PEEK"),
        ("RUST-STD-UNIX-SOCKETPAIR-RECV-NAME-PASS", "unnamed socketpair source address"),
        ("RUST-STD-UNIX-SOCKETPAIR-MSG-NAME-PASS", "connected socketpair destination rejection"),
        ("RUST-STD-UNIX-SOCKETPAIR-GETSOCKOPT-PASS", "socketpair SO_TYPE and SO_ERROR"),
        ("RUST-STD-UNIX-SOCKETPAIR-DOMAIN-PROTOCOL-PASS", "socketpair SO_DOMAIN and SO_PROTOCOL"),
        ("RUST-STD-UNIX-SOCKETPAIR-PEERCRED-PASS", "socketpair peer credentials"),
        ("RUST-STD-UNIX-SOCKETPAIR-ACCEPTCONN-PASS", "socketpair SO_ACCEPTCONN"),
        ("RUST-STD-UNIX-SOCKETPAIR-NAME-PASS", "socketpair local and peer names"),
        ("RUST-STD-UNIX-SOCKETPAIR-CLOEXEC-PASS", "socketpair close-on-exec flags"),
        ("RUST-STD-UNIX-SOCKETPAIR-NONBLOCK-PASS", "socketpair nonblocking flags"),
        ("RUST-STD-UNIX-SOCKETPAIR-SHUTDOWN-PASS", "socketpair half-close and EOF"),
        ("RUST-STD-UNIX-SOCKETPAIR-SEND-RECV-PASS", "socketpair send/recv and MSG_PEEK"),
        ("RUST-STD-UNIX-SEQPACKET-VECTORED-PASS", "SOCK_SEQPACKET writev/readv preserve one record"),
        ("RUST-STD-TLS-PASS", "Rust local-exec TLS initialization"),
        ("RUST-STD-THREAD-MAIN-CURRENT-PASS", "main-thread identity"),
        ("RUST-STD-THREAD-CLOSURE-ENTER-PASS", "Rust thread closure entry"),
        ("RUST-STD-THREAD-SPAWN-RETURN-PASS", "Rust thread spawn returns to caller"),
        ("RUST-STD-PROCESS-INHERITED-STDIO-PASS", "child inherited standard descriptors"),
        ("RUST-STD-OS-STRING-FFI-PASS", "Unix OS string byte preservation"),
        ("RUST-STD-PATH-OS-STRING-FFI-PASS", "PathBuf OS string byte preservation"),
        ("RUST-STD-PATH-LEXICAL-PASS", "lexical path components and joins"),
        ("RUST-STD-FS-BYTE-PATH-PASS", "non-UTF-8 filesystem paths"),
        ("RUST-STD-READ-DIR-BYTE-NAME-PASS", "non-UTF-8 directory entry names"),
        ("RUST-STD-FS-DIRECTORY-PASS", "directory rename and file descriptor I/O"),
        ("RUST-STD-READ-DIR-NESTED-PASS", "nested directory iteration"),
        ("RUST-STD-THREAD-DETACH-PASS", "Rust JoinHandle drop and pthread detach"),
        ("RUST-STD-FS-TRUNCATE-PASS", "file descriptor truncation and data sync"),
        ("RUST-STD-FS-ATOMIC-RENAME-PASS", "atomic artifact replacement"),
        ("RUST-STD-REMOVE-DIR-ALL-RECURSIVE-PASS", "recursive directory removal through fdopendir/readdir"),
        ("RUST-STD-FS-PATH-MANAGEMENT-PASS", "nested directories and current directory"),
        ("RUST-STD-AT-DIRFD-PASS", "dirfd-relative openat/mkdirat/renameat/unlinkat"),
        ("RUST-STD-FS-SYMLINK-PASS", "upstream std symlink, read_link and symlink_metadata"),
        ("RUST-STD-FS-COPY-PASS", "upstream std filesystem artifact copy and byte verification"),
        ("RUST-STD-FS-HARD-LINK-PASS", "upstream std hard-link creation and inode lifetime"),
        ("RUST-STD-FS-PERMISSIONS-PASS", "upstream std Unix file permission metadata and updates"),
        ("RUST-STD-FS-FLOCK-FLAGS-PASS", "flock syscall argument validation and descriptor checks"),
        ("RUST-STD-FS-FLOCK-PASS", "upstream std File locking, blocking wait and close release"),
        ("RUST-STD-ENV-PASS", "environment"),
        ("RUST-STD-ENV-MUTATION-PASS", "process environment set_var and remove_var"),
        ("RUST-STD-FS-METADATA-PASS", "filesystem metadata"),
        ("RUST-STD-READ-DIR-PASS", "directory iteration"),
        ("RUST-STD-THREAD-TLS-ISOLATION-PASS", "Rust thread-local storage isolation across threads"),
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

    # M4.3.1: the first freestanding C++ executable. It is built by the host
    # g++ without libstdc++/libc++ (no exceptions, no RTTI) and linked against
    # OCTAGRAM's own crt0 and C runtime.
    cpp_offset = len(serial_text())
    send_line("run /bin/cpp-smoke")
    if not wait_for_after("CPP-FREESTANDING-PASS", cpp_offset, 30):
        print("CPP-FREESTANDING-FAIL: C++ guest program did not emit its marker")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", cpp_offset, 30):
        print("CPP-FREESTANDING-FAIL: C++ guest program did not exit with status 0")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("CPP-FREESTANDING-FAIL: shell did not return after the C++ program")
        print(serial_text()[-6000:])
        return 1
    print("CPP-FREESTANDING-PASS")

    # M4.3.2: first program linked against the compiled libc++ library
    # (libc++.a) instead of headers only. It exercises std::string (small and
    # heap-backed), std::vector, std::sort, and std::mutex.
    libcxx_offset = len(serial_text())
    send_line("run /bin/libcxx-string-smoke")
    if not wait_for_after("LIBCXX-LIBRARY-PASS", libcxx_offset, 30):
        print("LIBCXX-LIBRARY-FAIL: compiled libc++ probe did not emit its marker")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", libcxx_offset, 30):
        print("LIBCXX-LIBRARY-FAIL: compiled libc++ probe did not exit with status 0")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("LIBCXX-LIBRARY-FAIL: shell did not return after the libc++ probe")
        print(serial_text()[-6000:])
        return 1
    print("LIBCXX-LIBRARY-PASS")

    # M4.3.2: iostream/locale chain (std::cout, stringstreams, locale facets).
    # It needs nl_langinfo, localeconv, and the POSIX _l wide-character classes.
    iostream_offset = len(serial_text())
    send_line("run /bin/libcxx-iostream-smoke")
    if not wait_for_after("LIBCXX-IOSTREAM-PASS", iostream_offset, 40):
        print("LIBCXX-IOSTREAM-FAIL: iostream probe did not emit its marker")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", iostream_offset, 30):
        print("LIBCXX-IOSTREAM-FAIL: iostream probe did not exit with status 0")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("LIBCXX-IOSTREAM-FAIL: shell did not return after the iostream probe")
        print(serial_text()[-6000:])
        return 1
    print("LIBCXX-IOSTREAM-PASS")

    # M4.3.2: threading, futures, and file streams. This is the part of
    # libc++ that Cargo/rustc lean on most: std::thread, mutex/condition
    # variable, promise/future, and std::fstream over the guest VFS.
    thread_offset = len(serial_text())
    send_line("run /bin/libcxx-thread-smoke")
    if not wait_for_after("LIBCXX-THREAD-SUITE-PASS", thread_offset, 60):
        print("LIBCXX-THREAD-FAIL: thread probe did not emit its marker")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", thread_offset, 30):
        print("LIBCXX-THREAD-FAIL: thread probe did not exit with status 0")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("LIBCXX-THREAD-FAIL: shell did not return after the thread probe")
        print(serial_text()[-6000:])
        return 1
    print("LIBCXX-THREAD-PASS")

    # M4.3.2: filesystem, random, regex, and chrono.
    fs_offset = len(serial_text())
    send_line("run /bin/libcxx-fs-smoke")
    if not wait_for_after("LIBCXX-FS-SUITE-PASS", fs_offset, 60):
        print("LIBCXX-FS-FAIL: filesystem probe did not emit its marker")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", fs_offset, 30):
        print("LIBCXX-FS-FAIL: filesystem probe did not exit with status 0")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("LIBCXX-FS-FAIL: shell did not return after the filesystem probe")
        print(serial_text()[-6000:])
        return 1
    print("LIBCXX-FS-PASS")

    # Containers and shared locking: ordered/unordered associative containers,
    # valarray, bitset, the C++17 vocabulary types, std::function, std::atomic,
    # and std::shared_mutex on top of the pthread_rwlock implementation.
    container_offset = len(serial_text())
    send_line("/bin/libcxx-container-smoke")
    if not wait_for_after("LIBCXX-CONTAINER-PASS", container_offset, 90):
        print("LIBCXX-CONTAINER-FAIL: guest probe did not pass")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("LIBCXX-CONTAINER-FAIL: shell did not return")
        print(serial_text()[-6000:])
        return 1
    print("LIBCXX-CONTAINER-PASS")

    # std::format/std::print: compile-time-checked format strings into
    # std::string, format_to, vformat, numeric/float/string specifiers,
    # std::print/std::println on fd 1, std::expected, std::osyncstream and
    # mdspan.
    format_offset = len(serial_text())
    send_line("/bin/libcxx-format-smoke")
    if not wait_for_after("LIBCXX-FORMAT-PASS", format_offset, 90):
        print("LIBCXX-FORMAT-FAIL: guest probe did not pass")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("LIBCXX-FORMAT-FAIL: shell did not return")
        print(serial_text()[-6000:])
        return 1
    print("LIBCXX-FORMAT-PASS")

    # Memory-backed stdio: open_memstream/fmemopen/dprintf, the API a compiler
    # front end uses to assemble a diagnostic string without a filesystem
    # round trip, plus the append/short-write/zero-filled-hole semantics.
    memstream_offset = len(serial_text())
    send_line("/bin/memstream-demo")
    if not wait_for_after("MEMSTREAM-PASS", memstream_offset, 60):
        print("MEMSTREAM-FAIL: guest probe did not pass")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("MEMSTREAM-FAIL: shell did not return")
        print(serial_text()[-6000:])
        return 1
    print("MEMSTREAM-PASS")

    # Search, traversal and word expansion: tsearch/twalk, scandir/alphasort,
    # wordexp/wordfree (quoting, $VAR, glob, error codes), nftw/ftw and the two
    # basename forms. These are what a ported build tool uses instead of a
    # shell to expand a command line and walk a source tree.
    search_offset = len(serial_text())
    send_line("/bin/posix-search-demo")
    if not wait_for_after("POSIX-SEARCH-PASS", search_offset, 60):
        print("POSIX-SEARCH-FAIL: guest probe did not pass")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("POSIX-SEARCH-FAIL: shell did not return")
        print(serial_text()[-6000:])
        return 1
    print("POSIX-SEARCH-PASS")

    # Process, table and temporary-file layer: popen/pclose, hsearch, the
    # mkostemp/mkstemps family, arc4random and mmap64.
    toolchain_offset = len(serial_text())
    send_line("/bin/toolchain-demo")
    if not wait_for_after("TOOLCHAIN-PASS", toolchain_offset, 60):
        print("TOOLCHAIN-FAIL: guest probe did not pass")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("TOOLCHAIN-FAIL: shell did not return")
        print(serial_text()[-6000:])
        return 1
    print("TOOLCHAIN-PASS")

    # M4.4 groundwork: the Cargo-side driver parses Cargo.toml/Cargo.lock
    # offline, resolves path dependencies into a build order, and reports
    # metadata. It does not claim to be upstream Cargo, and it is not asked to
    # build here because no rustc exists in the guest yet.
    cargo_offset = len(serial_text())
    # `cargo` is the shell-facing name of the octagram-cargo driver: the
    # probe exercises the name a user types, not only the explicit binary.
    send_line("cargo --locked plan --manifest-path /cargo-demo/Cargo.toml")
    if not wait_for_after("demo-lib 0.1.0", cargo_offset, 30):
        print("OCTAGRAM-CARGO-FAIL: plan did not list the path dependency")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("demo-app 0.1.0", cargo_offset, 30):
        print("OCTAGRAM-CARGO-FAIL: plan did not list the root package")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("OCTAGRAM-CARGO-FAIL: shell did not return after plan")
        print(serial_text()[-6000:])
        return 1

    metadata_offset = len(serial_text())
    send_line("cargo metadata --manifest-path /cargo-demo/Cargo.toml")
    if not wait_for_after("\"name\": \"demo_lib\"", metadata_offset, 30):
        print("OCTAGRAM-CARGO-FAIL: metadata did not describe the lib target")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("OCTAGRAM-CARGO-FAIL: shell did not return after metadata")
        print(serial_text()[-6000:])
        return 1
    print("OCTAGRAM-CARGO-PASS")

    # M4.3.4 gate, executed only when a native toolchain was installed into
    # /toolchain (tools/install-disk.sh --toolchain ...): the compiler must run
    # as an OCTAGRAM process and Cargo must drive it offline. An image without
    # a toolchain reports SKIP instead of failing the run.
    rustc_offset = len(serial_text())
    send_line("rustc --version")
    if not wait_prompt(60):
        print("NATIVE-RUSTC-FAIL: shell did not return after rustc --version")
        print(serial_text()[-6000:])
        return 1
    rustc_output = serial_text()[rustc_offset:]
    if "command not found" in rustc_output:
        print("OCTAGRAM-RUSTC-SKIP: no /toolchain/bin/rustc in this image")
    else:
        if "rustc 1." not in rustc_output:
            print("NATIVE-RUSTC-FAIL: rustc --version printed no version string")
            print(rustc_output[-3000:])
            return 1
        print("NATIVE-RUSTC-VERSION-PASS")

        build_offset = len(serial_text())
        send_line("cargo build --offline --manifest-path /cargo-demo/Cargo.toml")
        if not wait_prompt(600):
            print("NATIVE-CARGO-FAIL: cargo build --offline did not return")
            print(serial_text()[-6000:])
            return 1
        build_output = serial_text()[build_offset:]
        if "error" in build_output.lower():
            print("NATIVE-CARGO-FAIL: cargo build --offline reported an error")
            print(build_output[-6000:])
            return 1
        print("OCTAGRAM-RUSTC-PASS")

    # M4.3.0: the object emitted by rustc for x86_64-unknown-octagram is linked
    # by the guest's own `ld` and the result runs, so the native linker is
    # proven against real rustc relocation and section-symbol output.
    native_offset = len(serial_text())
    send_line("ld /share/octagram/rust_native_probe.o -o /tmp/rust_native_probe.elf")
    if not wait_for_after("ld: wrote", native_offset, 30):
        print("RUST-NATIVE-LD-FAIL: native ld did not link the rustc object")
        print(serial_text()[-6000:])
        return 1
    send_line("run /tmp/rust_native_probe.elf")
    if not wait_for_after("RUST-NATIVE-LD-PASS", native_offset, 30):
        print("RUST-NATIVE-LD-FAIL: linked executable did not emit its marker")
        print(serial_text()[-6000:])
        return 1
    if not wait_for_after("program exited with status 0", native_offset, 30):
        print("RUST-NATIVE-LD-FAIL: linked executable did not exit with status 0")
        print(serial_text()[-6000:])
        return 1
    if not wait_prompt(30):
        print("RUST-NATIVE-LD-FAIL: shell did not return after linked executable")
        print(serial_text()[-6000:])
        return 1
    print("RUST-NATIVE-LD-PASS")

    # Keep the independent POSIX spawn regression after the Rust std probe so
    # a failure in that demo cannot mask the upstream std/TLS guest result.
    spawn_offset = len(serial_text())
    send_line("run /bin/posix_spawn_demo")
    if not wait_for_after("posix_spawn environment/status/WNOHANG/file actions/chdir/setsid/signal attributes/scheduling/resetids/spawnp passed", spawn_offset, 120):
        print("POSIX-SPAWN-PIPE-FD-PATH-FAIL: guest program did not emit its success marker")
        print(serial_text()[-5000:])
        return 1
    if not wait_for_after("program exited with status 0", spawn_offset, 120):
        print("POSIX-SPAWN-PIPE-FD-PATH-FAIL: guest program did not exit with status 0")
        print(serial_text()[-5000:])
        return 1
    if not wait_prompt(30):
        print("POSIX-SPAWN-PIPE-FD-PATH-FAIL: shell did not return after guest program")
        print(serial_text()[-5000:])
        return 1
    print("POSIX-SPAWN-PIPE-FD-PATH-PASS")

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
