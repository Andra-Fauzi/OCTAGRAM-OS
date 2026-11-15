# Arsitektur kernel

OCTAGRAM-OS adalah kernel x86_64 Rust yang boot melalui Limine. Kode platform
berada di `kernel/src/arch/x86_64/`; driver perangkat berada di
`kernel/src/drivers/`; layanan umum berada di `memory/`, `fs/`, `storage/`,
`terminal/`, dan `task.rs`.

## Urutan boot

`kmain` mengaktifkan logger serial COM1 sebelum inisialisasi lain agar pesan
boot awal tersedia di serial console. Setelah itu `kernel/src/main.rs`
menyiapkan GDT/TSS dan IDT, APIC/IOAPIC,
memory allocator, lalu VFS RAMFS. `drivers::initialize()` mengatur urutan
PCI, AHCI, dan USB; `drivers::poll_deferred()` menangani hotplug dari idle loop,
di luar interrupt handler. Shell dijalankan sebagai future pada executor
kooperatif. Perubahan urutan inisialisasi harus mempertimbangkan dependensi:
alokasi kernel memerlukan memory subsystem dan shell memerlukan VFS serta
keyboard.

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
yang menunggu keyboard dapat menahan proses lain sampai input tiba. `fork`
menyalin halaman user secara eager; copy-on-write dan SMP belum tersedia.

## Memory dan alamat ELF

Loader menerima little-endian ELF64 `ET_EXEC`, memeriksa program headers,
menolak dynamic/interpreter dan segmen writable+executable, lalu menyalin
segmen ke alamat virtual yang diminta. Program mendapat stack 8 halaman dan
heap image awal 1 MiB pada alamat `0x10000000`. Runtime memakai pemetaan
anonim tambahan melalui `mmap`/`munmap`, dibatasi oleh alamat
virtual dan frame fisik yang tersedia, bukan kuota halaman tetap. Pemetaan user
berada di page table privat tiap image, sedangkan pemetaan kernel dibagikan.

## Filesystem dan terminal

VFS global merutekan path ke mount point terpanjang. Root awal adalah RAMFS
volatil; `/dev` adalah devfs yang menyediakan empat virtual terminal,
`/dev/tty0` sampai `/dev/tty3`, `/dev/tty` untuk terminal aktif, dan
standard-stream endpoints. FAT32 dapat dipasang dari shell dan mendukung
operasi baca/tulis VFS, tetapi tidak memiliki journaling.

Program memiliki descriptor dan direktori kerja sendiri (diwarisi dari shell),
tetapi berbagi namespace VFS dan mount global. Descriptor 0/1/2 terhubung ke
TTY shell asal program. Input TTY memakai mode baris dengan echo, Backspace,
Enter, dan Ctrl-D; tiap virtual terminal memiliki buffer teks dan antrean input
sendiri. Pembacaan input menunggu interrupt keyboard. Userland tidak memiliki
syscall untuk menggambar langsung ke framebuffer. Ini belum menyediakan pseudo-terminal,
terminal control job, window system, atau filesystem terisolasi per proses.

## Modul penting

- `arch/x86_64/gdt.rs`: GDT, TSS, transisi ring-3 dan kembali.
- `arch/x86_64/idt.rs` serta `interrupt.rs`: IDT, interrupt stub, dan dispatch.
- `memory/paging.rs`: paging, mapping user, pemeriksaan rentang alamat.
- `elf.rs`: loader ELF, syscall ABI, dan pemakaian file VFS.
- `fs/`: trait filesystem, mount router, dan RAMFS.
- `storage/block.rs`: trait block device dan registry.
- `storage/fat32.rs` dan `storage/ext2.rs`: parsing serta operasi baca/tulis FAT32
  dan ext2.
- `drivers/`: driver PCI, AHCI, keyboard, USB, serial COM1 untuk log kernel,
  RTC CMOS untuk perintah `date`, dan ACPI untuk shutdown.
- `drivers/qca9377.rs`: deteksi PCI Qualcomm, wake/sleep MMIO terbatas untuk
  membaca register target. Probe laptop menunjukkan RTC ON (`0x73`), tetapi
  chip-ID sebelumnya dibaca dari offset yang keliru dan revisi belum
  terkonfirmasi. Driver memilih board data QCA9377. Alokator frame menyediakan
  halaman DMA32 dan modul CE menyediakan ring BMI serta pertukaran polling.
  Probe BMI yang mengaktifkan DMA tersedia sebagai feature eksperimen dan
  dinonaktifkan secara default sampai reset PCI, konfigurasi CE target, serta
  wake CPU target mengikuti urutan upstream. Upload board data, OTP, firmware,
  dan koneksi Wi-Fi belum berjalan.
- `task.rs`: executor kooperatif.

Sebelum mengubah subsystem, baca implementasi modul dan tesnya; jangan
menganggap API-nya setara Linux.
