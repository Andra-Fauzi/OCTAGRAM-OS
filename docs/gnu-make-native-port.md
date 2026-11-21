# GNU Make native port for OCTAGRAM-OS

## Scope

Port the upstream GNU Make implementation to the x86_64 OCTAGRAM userspace.
Do not extend the Rust bootstrap `make` to imitate GNU Make. The bootstrap
utility remains only until the upstream program can be built and executed
natively.

## Upstream source

- Project: GNU Make
- Release: 4.4.1
- Upstream mirror: https://github.com/mirror/make
- Release tag: `4.4.1`
- Pinned source commit: `d66a65ad5a0e31b287f53930b0f09e31801f1613`
- License: GPL-3.0-or-later (see upstream `COPYING`)

The source is tracked as a Git submodule at
`user/third_party/gnu-make`. The exact commit is pinned by the parent
repository's gitlink; update it only as a deliberate upstream-version change.
CI checks out submodules recursively and validates the pin.

## Porting constraints

OCTAGRAM already has an x86_64 userspace ABI, a freestanding C runtime,
TinyCC, `fork/execve/waitpid`, pipes, file descriptors, stat-family calls,
time APIs, and environment support. This is a useful starting point, but it
does not prove GNU Make can be linked or run yet.

The port must use OCTAGRAM's own headers and libc, not accidentally pick up
the host Linux headers or host libc. Initial target configuration:

- `--host=x86_64-unknown-octagram`
- disable NLS and Guile for the first native build
- static ELF executable using the OCTAGRAM CRT and libc
- no Linux dynamic loader or host shared-library dependency

GNU Make's configure probes must be treated as cross-compilation probes:
never execute an OCTAGRAM ELF on the Linux build host. Generate the target
feature configuration explicitly, then compile and link against OCTAGRAM
headers/libraries.

## Milestones

1. **Source import and provenance — done.** Pin GNU Make 4.4.1 as a submodule
   and make CI verify the exact upstream revision.
2. **Configure/ABI audit — next.** Enumerate configure feature tests and the
   source-level APIs used by the x86_64/POSIX implementation. Produce a
   checked-in OCTAGRAM config header rather than reusing host-generated
   `config.h`.
3. **First cross-build.** Compile upstream `src/` with the OCTAGRAM C headers,
   CRT, and libc. Keep unsupported features disabled only where GNU Make's
   configure system supports that choice; do not replace GNU Make algorithms
   with a custom implementation.
4. **Runtime compatibility.** Implement missing libc/POSIX functions in
   OCTAGRAM's libc/kernel ABI. Prioritize directory traversal, glob/fnmatch,
   signals/job control, wait status, terminal process groups, timestamps,
   temporary files, and subprocess semantics as demonstrated by build errors
   and tests.
5. **Native execution.** Run the upstream test suite's portable subset in
   QEMU, then run real recursive and parallel Makefiles on persistent ext2.
6. **System integration.** Install the GNU Make executable as `/bin/make`.
   Keep the small Rust utility under a distinct bootstrap name until native
   GNU Make has passed the acceptance tests.
7. **Self-hosting gate.** Build OCTAGRAM userspace and kernel dependencies from
   inside OCTAGRAM using GNU Make, with no Linux Make process involved.

## Acceptance tests

- `make --version` identifies GNU Make 4.4.1.
- `make -f Makefile -n` expands recipes correctly.
- variables, includes, functions, pattern and suffix rules work.
- automatic variables, order-only prerequisites, phony targets, and recursive
  `$(MAKE)` work.
- incremental rebuild and failure propagation work.
- `-j2` and higher parallel jobs do not lose child exit status or deadlock.
- QEMU tests prove process, signal, pipe, and filesystem behavior.
- a real OCTAGRAM component can be rebuilt from source using the native GNU
  Make binary.

## Current status

Only source import/provenance and CI checkout validation are complete.
GNU Make is **not yet ported, built, embedded, or QEMU-tested**. The existing
Rust `/bin/make` remains the bootstrap implementation until the acceptance
gates above pass.


## ABI audit (21 November 2026)

The upstream POSIX implementation uses the process/job-control path in
`src/job.c`, `src/posixos.c`, and `src/main.c`. The OCTAGRAM libc headers
and runtime already expose several of the required interfaces:

| Area | Existing OCTAGRAM support |
| --- | --- |
| Process creation | `fork`, `execve`, `waitpid`, `posix_spawn`/`posix_spawnp` |
| Child file descriptors | `pipe`, `dup2`, `fcntl`, `open`, `close` |
| Signals | `sigaction`, `sigprocmask`, signal masks in spawn attributes |
| Job control | `setpgid`, process/session IDs, terminal APIs |
| Filesystem | `stat`, `lstat`, `fstat`, `fstatat`, `opendir`, `readdir`, `closedir` |
| Paths | `access`, `chdir`, `getcwd`, `realpath`, `readlink` |
| Time/resource | `utimensat`, `futimens`, `getrlimit`, `setrlimit`, `getloadavg` |
| Diagnostics | `strerror`, `strsignal`, `isatty`, `ttyname` |

The following are gaps or configure decisions to verify before compiling:

- `pselect` is not currently declared in OCTAGRAM's `sys/select.h`; keep
  `HAVE_PSELECT` disabled initially and use the `select` fallback.
- `mkstemp`, `mktemp`, and `setlinebuf` are not currently exposed by the
  OCTAGRAM libc headers/runtime. Check whether the bundled gnulib objects
  provide the needed replacements; otherwise add libc implementations.
- `strcoll`/locale behavior is not yet equivalent to a full POSIX locale
  implementation. The first build should use the C locale and disable NLS.
- The native userspace does not yet provide a standalone `/bin/sh` executable.
  GNU Make recipes are executed through `$(SHELL) -c`, so a usable native
  command interpreter is a hard runtime dependency, not an optional feature.
  The in-kernel interactive shell is not automatically an executable that
  GNU Make can spawn.
- Configure must not accidentally detect host glibc headers/functions or link
  against host libc. A successful Linux build is not evidence of an OCTAGRAM
  target build.

The immediate implementation order is therefore: provide a deterministic
OCTAGRAM config header and cross-build; add/confirm the missing libc APIs;
provide a spawnable `/bin/sh`; then run the upstream Make test subset under
QEMU. Do not install upstream GNU Make as `/bin/make` until recipe execution
and child status handling pass those tests.


## Cross-build implementation (21 November 2026)

The first reproducible cross-build driver is
`tools/build_gnu_make_octagram.sh`. It:

1. bootstraps the upstream Autotools files and GNU Make's pinned gnulib
   revision from `bootstrap.conf`;
2. teaches the generated `config.sub` the OCTAGRAM OS token;
3. configures with `--host=x86_64-unknown-octagram`, NLS/Guile/load and
   posix_spawn disabled for the first pass;
4. uses only the OCTAGRAM userspace headers plus GCC's compiler-internal
   headers (`-nostdinc` prevents accidental use of Linux system headers);
5. links against OCTAGRAM's existing CRT/runtime with its static linker script;
6. verifies the result is an x86_64 static ELF without a host interpreter or
   dynamic section.

The generated executable is kept at
`user/build/gnu-make-octagram/make`; this is a build artifact and does not
replace the current bootstrap `/bin/make`.

The target libc header `user/include/limits.h` now defines the x86_64/POSIX
limits required by the strict no-host-headers build. More missing declarations
or implementations will be added only when the upstream configure/build
probes demonstrate they are needed.

The bootstrap now sets `GNULIB_URL` to the GitHub mirror `coreutils/gnulib` while retaining the upstream `GNULIB_REVISION=stable-202301` pin. This avoids the intermittent Savannah HTTP 500 seen in CI. It also removes an incomplete `user/third_party/gnu-make/gnulib` checkout (one without `gnulib-tool`) before retrying, because GNU Make's bootstrap otherwise mistakes the empty directory left by a failed fetch for a usable checkout. The mirror exposes the pinned `stable-202301` branch; the source revision remains selected by GNU Make's own `bootstrap.conf`.

The first compile attempt exposed three target-header mismatches. The OCTAGRAM
`unistd.h` now advertises POSIX via `_POSIX_VERSION`, preventing GNU Make's
legacy zero-argument `getcwd` declaration from conflicting with the POSIX
prototype. `user/include/ar.h` provides the traditional System V archive header
used by upstream `arscan.c`, and the cross-build maps `alloca` to GCC's compiler
builtin because this freestanding target does not ship a host `alloca.h`. The
CI build has not yet passed; these changes are being validated by the next
workflow run.

CI runs this cross-build after the existing userland CRT/runtime build. A
successful compile is only the first gate: QEMU execution, recipe shell
integration, and upstream regression tests remain separate gates.
