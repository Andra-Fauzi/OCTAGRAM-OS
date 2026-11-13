# OCTAGRAM-OS

This repository will demonstrate how to set up a basic kernel in Rust using Limine.

## Kernel layout

The kernel is organized by responsibility so new hardware and platform support
have a clear place:

- `kernel/src/main.rs` coordinates boot and subsystem initialization.
- `kernel/src/arch/<architecture>/` contains CPU/platform-specific code such as
  interrupt handling, APIC, and port I/O.
- `kernel/src/drivers/` contains device drivers, currently PCI and USB.
- `kernel/src/memory/`, `graphics/`, and `terminal/` contain shared kernel
  services.
- `kernel/src/fs/` contains the VFS, and `kernel/src/storage/` contains block
  devices and on-disk filesystems. Boot mounts a volatile RAM root filesystem;
  persistent volumes can be mounted from the shell.

Keep hardware-specific instructions in `arch/`, and build reusable device
logic in `drivers/`. When adding a subsystem, expose its module through the
appropriate `mod.rs` and keep initialization/call sites in `main.rs` limited to
boot orchestration.

## Task model

The kernel includes a cooperative async executor in `kernel/src/task.rs`.
`task::spawn` registers a `Send + 'static` future, `task::yield_now().await`
voluntarily gives other runnable tasks a turn, and the idle loop polls ready
tasks before halting the CPU. A retained `TaskHandle` can wake a pending task;
its `wake` operation only sets an atomic flag and is suitable for interrupt
handlers.

These are **kernel tasks**, not isolated processes: tasks share the kernel
address space and privileges. They are not preempted, so every task must return
or yield regularly; blocking I/O or an infinite loop without a yield stalls the
whole kernel. Do not call `task::spawn` from an interrupt handler. Preemptive threads for kernel tasks are not implemented. Kernel tasks share one
global VFS and framebuffer terminal. User ELF programs get private address
spaces, per-process file descriptors, working directories, PIDs, and kernel
stacks. `runp <elf> <elf>...` uses round-robin LAPIC timer preemption while
programs execute in ring 3. Syscalls that block in the kernel (such as TTY
input) are not yet scheduler-aware and can delay other processes. User
processes still share the global filesystem namespace and mounts.

## Interactive shell

The framebuffer shell starts after boot and accepts input from USB HID
keyboards and the legacy PS/2 keyboard IRQ. It includes `help`, `clear`, `pwd`,
`cd`, `ls`, `cat`, `touch`, `mkdir`, `rm`, `stat`, `cp`, `edit`, `devices`,
`mount`, `umount`, `echo`, and `run`. Paths support relative names, `.`, and
`..`; the keyboard maps common US-QWERTY punctuation including `/`.
`edit <path>` opens or creates a UTF-8
file (up to 8 KiB); use arrow keys to move, Backspace to delete, Ctrl+S to
save, and Ctrl+Q to quit. Unsaved changes require a second Ctrl+Q to discard.
File paths are rooted at `/` in the volatile RAM VFS. Keyboard input is queued
by the interrupt handler and consumed by the shell task.

## Persistent storage

The x86_64 kernel detects SATA disks through AHCI (`sata0`, `sata1`, ...) and
xHCI USB Mass Storage devices using Bulk-Only Transport (`usb0`, `usb1`, ...).
Run `devices` to list registered block devices. FAT32 volumes with an MBR
partition table (or a FAT32 superfloppy without a partition table) can be
mounted read-write:

```text
mkdir /mnt
devices
mount usb0 /mnt
ls /mnt
cat /mnt/README.TXT
touch /mnt/NEW.TXT
edit /mnt/NEW.TXT
umount /mnt
```

The FAT32 driver supports file reads and writes, truncation, file and directory
creation, and removal through the VFS. GPT, exFAT, NTFS, and USB Attached SCSI
(UASP) are not supported. FAT32 has no journaling, so an I/O failure or power
loss during a mutation can leave filesystem metadata inconsistent. Unmount a
volume before disconnecting its device.

## Running ELF programs

The x86_64 kernel has an initial user-mode ELF64 loader in `kernel/src/elf.rs`.
It accepts little-endian, static `ET_EXEC` binaries only; interpreters,
dynamic linking, writable-executable segments, and images whose mapped
segments, fixed 1 MiB heap, and stack exceed 4,096 pages (~16 MiB) are
rejected. Loaded program pages are user-accessible, the stack is
non-executable, and user faults terminate the program. ELF files can be loaded
from the VFS with
`elf::run_file("/bin/program")` after the file has been written there through
the VFS API.

The OCTAGRAM ABI is intentionally small and is not Linux-compatible:

- `int 0x80`, `RAX = 0`: exit, with status in `RDI`.
- `RAX = 1..12`: read/write, open/close, seek/stat, mkdir/unlink, truncate,
  heap break, getpid, and isatty. Arguments use `RDI`, `RSI`, and `RDX`;
  failures return negative errno values. See [docs/syscall-abi.md](docs/syscall-abi.md)
  for the complete table and data layouts.

Programs run in ring 3 using private user page tables with shared kernel
mappings. `runp` starts multiple ELFs under round-robin preemption; the shell
waits until every process exits or faults. A process that never exits can
therefore hold up the caller indefinitely.
`user/include/octagram.h` and `user/runtime.c` provide a small freestanding
runtime, not a full libc. Build static user ELFs on the development host using
`make -C user`. The OS shell also has a native minimal `as`, `ld`, and `cc`
subset for simple programs; these are not GNU toolchain ports. See
[`docs/building.md`](docs/building.md) for supported syntax and usage.

## Developer documentation

The [docs/](docs/README.md) directory contains guides to kernel architecture,
building user programs, the syscall ABI, storage, porting applications, and
adding drivers.

## Development checks

From the repository root, format and check the x86_64 kernel with:

```sh
cargo fmt --manifest-path kernel/Cargo.toml -- --check
cargo test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
make -C kernel KARCH=x86_64
make -C user
```

The build requires the Rust target configured by `kernel/rust-toolchain.toml`.
See the sections below for image creation and running the OS in QEMU.

## How to use this?

### Dependencies

Any `make` command depends on GNU make (`gmake`) and is expected to be run using it. This usually means using `make` on most GNU/Linux distros, or `gmake` on other non-GNU systems.

All `make all*` targets depend on Rust.

Additionally, building an ISO with `make all` requires `xorriso`, and building a HDD/USB image with `make all-hdd` requires `sgdisk` (usually from `gdisk` or `gptfdisk` packages) and `mtools`.

### Architectural targets

The `KARCH` make variable determines the target architecture to build the kernel and image for.

The default `KARCH` is `x86_64`. Other options include: `aarch64`, `riscv64`, and `loongarch64`.

Other architectures will need to be enabled in kernel/rust-toolchain.toml

### Makefile targets

Running `make all` will compile the kernel (from the `kernel/` directory) and then generate a bootable ISO image.

Running `make all-hdd` will compile the kernel and then generate a raw image suitable to be flashed onto a USB stick or hard drive/SSD.

Running `make run` will build the kernel and a bootable ISO (equivalent to make all) and then run it using `qemu` (if installed).

Running `make run-hdd` will build the kernel and a raw HDD image (equivalent to make all-hdd) and then run it using `qemu` (if installed).

The `run-uefi` and `run-hdd-uefi` targets are equivalent to their non `-uefi` counterparts except that they boot `qemu` using a UEFI-compatible firmware.
