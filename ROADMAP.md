# OCTAGRAM-OS Development Roadmap

## Objective

Develop OCTAGRAM-OS into a self-hosting POSIX-style operating system with native
user applications, isolated processes, persistent storage, networking, desktop
services, and a package/application ecosystem.

## Priority 1: Core process isolation

- [x] Give each process a private user page table where technically feasible
- [x] Prevent ring 3 code from changing kernel mappings
- [x] Add process groups and session identifiers
- [ ] Add ASLR for PIE, mmap, stack, and heap layouts
  - [x] Randomized PIE load bias
  - [x] Randomized mmap starting cursor
- [ ] Restrict fork/exec address-space inheritance
- [ ] Add process capability and policy metadata
- [ ] Test kernel faults, invalid mappings, and cross-process memory access

## Priority 2: POSIX runtime

- [x] Basic file and directory operations
- [x] Process fork, execve, waitpid, pipes, and threads
- [x] Signals, poll, select, epoll, sockets, eventfd, and timerfd
- [x] pread/pwrite/readv/writev and resource APIs
- [x] Implement `realpath`
- [x] Implement `execvp`
- [ ] Implement complete `posix_spawn` file actions and attributes
- [x] Implement basic process groups and `setsid`
- [ ] Implement alternate signal stacks
- [ ] Implement dynamic shared module loading
- [ ] Implement sendmsg/recvmsg
- [ ] Implement a complete `pwd`/`grp` database

## Priority 3: Dynamic linker and native runtime

- [x] PIC/PIE validation for dynamic executables
- [ ] TLS support in shared objects
- [x] Constructors and init arrays
- [ ] Destructors and fini arrays
- [ ] Symbol versioning
- [ ] Lazy binding
- [x] `LD_LIBRARY_PATH` library search path
- [ ] Copy relocations and weak symbols
- [ ] ASLR and randomized fixed load bias

## Priority 4: Scheduler and threads

- [x] Replace the fixed 16-task limit with a 64-task scheduler limit
- [x] Add basic thread attributes and detached-state handling
- [ ] Add scheduler priorities and aging
- [ ] Make blocking syscalls scheduler-aware
- [ ] Implement wakeup cancellation and robust timed waits
- [ ] Improve mutex/condvar/futex implementation
- [ ] Add kernel task fairness and watchdog monitoring

## Priority 5: Networking

- [ ] TCP IPv4 reliability and socket lifecycle hardening
- [x] UDP POSIX API for socket/bind/sendto/recvfrom
- [ ] IPv6
- [x] Nonblocking socket support via SOCK_NONBLOCK and MSG_DONTWAIT
- [ ] DNS resolver cache and complete getaddrinfo-like API
  - [x] Numeric IPv4 `getaddrinfo`, `inet_*`
- [ ] Network namespace or policy layer
- [ ] DHCP/client and routing persistence

## Priority 6: Storage

- [ ] Persistent root and per-user home
  - [x] `/home/octagram`, `/etc`, `/var`, `/opt`, `/apps`
  - [x] `df` mount reporting
  - [x] `persist <mount>` home setup
- [ ] Journaling/recovery for mutation failures
- [ ] Ext3/ext4 read support or focused write support
- [ ] EFI/GPT partition mounting improvements
- [ ] ext2 race and corruption hardening
- [ ] Swap or swap-backed anonymous memory
- [ ] RAID/LVM volume abstraction
- [ ] Disk image tools and recovery utility

## Priority 7: Drivers

- [ ] Audio/HDA
- [ ] Generic GPU framebuffer and accelerated graphics
- [ ] Printer and webcam USB classes
- [ ] HID improvements and multi-pointer support
- [ ] Network-interface abstraction
- [ ] NVMe
- [ ] UASP
- [ ] Stable device hotplug lifecycle

## Priority 8: Desktop and ecosystem

- [ ] Compositor/window manager
- [ ] GUI toolkit and drawing API
- [ ] Font rasterization and locale support
- [ ] Terminal emulator improvements
- [ ] Session manager and login
  - [x] `login`, `whoami`, `session`, and prompt user
  - [ ] Password/authentication database
- [x] Package metadata and repository format
  - [x] `/var/lib/octagram/packages/*.pkg`
  - [x] `registry.txt` package registry
- [ ] Package manager with install/remove/update/verify
  - [x] `pkg add/update/remove/list/info/verify`
- [ ] App launcher and file manager
  - [x] `/apps` registry directory
  - [x] `apps` shell command
- [ ] Power/session settings

## CPython 3.11/3.12

CPython is intentionally separate from the roadmap. Use `make python311` or
`make python312` only after the POSIX and linker milestones above are stable.
Do not install a Linux CPython ELF in OCTAGRAM; it will not use the OCTAGRAM
syscall ABI.

## Acceptance rules

No milestone is complete until:

1. It builds from a clean repository.
2. It has an OCTAGRAM ELF or kernel smoke test.
3. It is seeded into the runtime/VFS when required.
4. It has a focused positive and negative test.
5. Documentation records unsupported behavior explicitly.
