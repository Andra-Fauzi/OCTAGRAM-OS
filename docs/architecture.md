# Arsitektur kernel

OCTAGRAM-OS adalah kernel x86_64 Rust yang boot melalui Limine. Kode platform
berada di `kernel/src/arch/x86_64/`; driver perangkat berada di
`kernel/src/drivers/`; layanan umum berada di `memory/`, `fs/`, `storage/`,
`terminal/`, dan `task.rs`.

## Urutan boot

`kmain` di `kernel/src/main.rs` menyiapkan GDT/TSS dan IDT, APIC/IOAPIC,
memory allocator, VFS RAMFS, PCI, AHCI, lalu USB. Shell dijalankan sebagai
future pada executor kooperatif. Perubahan urutan inisialisasi harus
mempertimbangkan dependensi: alokasi kernel memerlukan memory subsystem dan
shell memerlukan VFS serta keyboard.

## Task, process, dan user mode

Executor `task.rs` menjalankan task kernel kooperatif; task berbagi hak akses,
address space kernel, VFS global, dan terminal. ELF statis x86_64 dimuat ke
address space user privat. Perintah `run` menjalankan satu program; `runp`
menjalankan beberapa program dengan round-robin preemption timer LAPIC.
Descriptor, cwd, heap break, PID, kernel stack, dan page table dikelola per
proses. Saat program keluar atau fault, scheduler memilih proses berikutnya;
halaman proses dilepas setelah seluruh grup selesai.

Loader ELF membuat page table baru untuk setiap image: separuh alamat user
dimulai kosong dan hanya berisi segmen, stack, serta heap program itu; pemetaan
kernel di separuh atas tetap tersedia untuk syscall dan interrupt. Page milik
program dan page table privat dibebaskan setelah program selesai atau image
di-unload. Timer hanya mengganti proses saat interrupt berasal dari ring 3;
interrupt ketika kernel sedang menjalankan syscall tidak melakukan preemption
agar tidak memutus operasi kernel yang memegang lock. Karena itu syscall TTY
yang menunggu keyboard dapat menahan proses lain sampai input tiba. Ini belum
mencakup blocking/wakeup scheduler, `fork`, `exec`, `wait`, atau SMP.

## Memory dan alamat ELF

Loader menerima little-endian ELF64 `ET_EXEC`, memeriksa program headers,
menolak dynamic/interpreter dan segmen writable+executable, lalu menyalin
segmen ke alamat virtual yang diminta. Program mendapat stack 8 halaman dan
heap tetap 1 MiB pada alamat `0x10000000`; syscall `brk` hanya mengatur batas
logis heap tersebut. Total pemetaan program dibatasi 4096 halaman. Pemetaan
user berada di page table privat tiap image, sedangkan pemetaan kernel
dibagikan.

## Filesystem dan terminal

VFS global merutekan path ke mount point terpanjang. Root awal adalah RAMFS
volatil; `/dev` adalah devfs yang menyediakan `/dev/tty`, `/dev/console`, dan
standard-stream endpoints. FAT32 dapat dipasang dari shell dan mendukung
operasi baca/tulis VFS, tetapi tidak memiliki journaling.

Program memiliki descriptor dan direktori kerja sendiri (diwarisi dari shell),
tetapi berbagi
namespace VFS dan mount global. Descriptor 0/1/2 terhubung ke TTY. Input TTY
memakai mode baris dengan echo, Backspace, Enter, dan Ctrl-D; output terminal
menggunakan framebuffer. Pembacaan input menunggu interrupt keyboard. Ini
belum menyediakan pseudo-terminal, terminal control job, atau filesystem
terisolasi per proses.

## Modul penting

- `arch/x86_64/gdt.rs`: GDT, TSS, transisi ring-3 dan kembali.
- `arch/x86_64/idt.rs` serta `interrupt.rs`: IDT, interrupt stub, dan dispatch.
- `memory/paging.rs`: paging, mapping user, pemeriksaan rentang alamat.
- `elf.rs`: loader ELF, syscall ABI, dan pemakaian file VFS.
- `fs/`: trait filesystem, mount router, dan RAMFS.
- `storage/block.rs`: trait block device dan registry.
- `storage/fat32.rs`: parser dan mutasi FAT32.
- `drivers/`: driver PCI, AHCI, keyboard, dan USB.
- `task.rs`: executor kooperatif.

Sebelum mengubah subsystem, baca implementasi modul dan tesnya; jangan
menganggap API-nya setara Linux.
