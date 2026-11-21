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
