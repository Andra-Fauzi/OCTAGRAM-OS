# Panduan Pengembangan OCTAGRAM-OS

Panduan ini adalah peta kerja untuk memahami, membangun, menguji, dan mengembangkan
OCTAGRAM-OS. Baca bersama [arsitektur](architecture.md), [boot](boot.md),
[memori](memory.md), [driver](drivers.md), dan [pengujian](testing.md).

> **Batas cakupan:** kernel utama yang paling matang dan jalur driver yang
> dijelaskan di sini adalah x86_64. Adanya linker script atau target Rust untuk
> arsitektur lain tidak otomatis berarti seluruh driver dan perangkat keras
> sudah setara di arsitektur tersebut. Jangan menganggap semua ABI Linux atau
> seluruh POSIX sudah tersedia hanya karena sebuah header atau nama fungsi ada.

## 1. Pahami lapisan OS terlebih dahulu

Urutan dependensi yang berguna untuk membaca source:

1. **Bootloader dan entry point** memuat ELF kernel serta menyediakan informasi
   boot melalui Limine.
2. **Arsitektur CPU** menyiapkan GDT/TSS, IDT, interrupt, APIC, I/O, dan state CPU.
3. **Manajemen memori** menyediakan frame allocator, heap kernel, paging, dan
   pemetaan MMIO.
4. **Kernel services** menyediakan task executor, VFS, block layer, terminal,
   network stack, logging, dan proses user.
5. **Bus dan driver** menemukan controller (mis. PCI), menginisialisasi
   controller, lalu menemukan perangkat di bawahnya (mis. USB).
6. **Device class/subsystem** mengubah operasi hardware menjadi layanan OS:
   keyboard menjadi input terminal, USB mass storage menjadi block device,
   jaringan menjadi interface/socket.
7. **Userspace** menjalankan ELF dengan ABI OCTAGRAM, libc/runtime sendiri,
   dynamic linker, toolchain, dan program.

Pembedaan penting saat menulis driver:

- **Bus driver** menemukan dan memberi representasi pada perangkat di bus,
  misalnya PCI.
- **Controller driver** mengendalikan controller bus, misalnya xHCI.
- **Class/function driver** mengerti protokol fungsi perangkat, misalnya HID
  keyboard atau USB Mass Storage.
- **Subsystem adapter** mendaftarkan kemampuan perangkat ke layanan kernel,
  misalnya BlockDevice ke storage::block.

Jangan mencampur semua lapisan di satu file. Driver class USB seharusnya tidak
mengulang scan PCI atau menulis register xHCI secara langsung jika fungsi
tersebut bisa dilakukan oleh transport/controller layer.

## 2. Peta repository

| Path | Tanggung jawab |
| --- | --- |
| README.md | Ringkasan proyek, target arsitektur, build dan cara menjalankan image |
| ROADMAP.md | Milestone, prioritas, dan acceptance rules |
| GNUmakefile | Build kernel, image ISO/HDD, dan target QEMU di root |
| limine.conf | Konfigurasi entry kernel untuk Limine |
| kernel/Cargo.toml, Cargo.lock | Package kernel dan dependency Rust yang terkunci |
| kernel/rust-toolchain.toml | Toolchain Rust yang dipakai kernel |
| kernel/build.rs | Metadata build/revision yang disisipkan saat compile |
| kernel/linker-*.ld | Tata letak/linker script per target arsitektur |
| user/Makefile | Build program C, libc, TinyCC, dynamic linker, dan demo |
| user/runtime.c | Implementasi libc/runtime C dan wrapper syscall |
| user/include/ | Header ABI/API yang dipakai aplikasi user |
| user/crt0.S, user/linker.ld | Entry point dan link script program C statis |
| user/ldso/ | Source dynamic loader OCTAGRAM dan linker script-nya |
| user/rust/ | Workspace program Rust no_std dan runtime OCTAGRAM |
| user/examples/ | Aplikasi demo dan regression test userspace |
| user/tests/ | Test host untuk bagian runtime C yang bisa diuji tanpa kernel OCTAGRAM |
| user/third_party/tinycc/ | Source TinyCC upstream yang dipakai sebagai toolchain; bukan kode kernel inti |
| tools/ | Script developer, termasuk smoke test QEMU dan workflow port CPython |
| docs/ | Dokumentasi arsitektur, subsistem, ABI, driver, build, testing, dan porting |

### 2.1 Peta setiap file utama di kernel/src

| File / direktori | Fungsi |
| --- | --- |
| main.rs | Limine request, kmain, urutan inisialisasi subsistem, spawn shell, idle loop, deferred driver work |
| arch/mod.rs | Pemilihan/ekspor implementasi arsitektur |
| arch/x86_64/mod.rs | Pengelompokan fasilitas CPU x86_64 |
| arch/x86_64/gdt.rs | GDT, TSS, segment selector, stack untuk transisi privilege |
| arch/x86_64/idt.rs | Pemasangan IDT dan handler interrupt/exception |
| arch/x86_64/interrupt.rs | Stub/dispatch interrupt, syscall dan fault/IRQ routing |
| arch/x86_64/apic.rs | Local APIC, IOAPIC, timer, routing interrupt |
| arch/x86_64/io.rs | Primitive port I/O in/out |
| arch/x86_64/fpu.rs | Inisialisasi state FPU/SSE |
| arch/x86_64/pat.rs | Pengaturan Page Attribute Table/cache type |
| arch/x86_64/thread_pointer.rs | Pengaturan thread pointer untuk TLS |
| auth.rs | Parsing dan pengelolaan credential/authentication database |
| sha256.rs | Primitive SHA-256 yang digunakan komponen autentikasi |
| error.rs | Tipe error kernel bersama |
| rsdp.rs | Penemuan root pointer ACPI (RSDP) |
| elf.rs | ELF loader, address space user, process/thread execution, syscall dispatch, dan lifecycle |
| task.rs | Executor future kernel kooperatif dan TaskHandle |
| sync.rs | Primitive sinkronisasi kernel bersama |
| memory/mod.rs | Inisialisasi layanan memori |
| memory/frame.rs | Frame allocator fisik, termasuk jalur frame DMA32 |
| memory/heap.rs | Heap kernel untuk alokasi dinamis |
| memory/paging.rs | Page table, mapping/unmapping, pemeriksaan user range, mapping MMIO |
| drivers/mod.rs | Registrasi driver saat boot dan polling pekerjaan deferred |
| drivers/device_model.rs | Registry perangkat/driver, matching, probe, state, remove |
| drivers/pci/mod.rs | API publik PCI dan struktur PciDevice |
| drivers/pci/config.rs | Baca/tulis PCI Configuration Space melalui port 0xCF8/0xCFC |
| drivers/pci/enumeration.rs | Scan BDF bus/device/function dan menghasilkan snapshot PCI |
| drivers/pci/bar.rs | Enable PCI function, baca BAR, kontrol memory decoding/bus mastering |
| drivers/pci/capability.rs | Traversal capability PCI, MSI/MSI-X metadata, power management, ASPM |
| drivers/ahci.rs | SATA AHCI controller dan adaptasi ke block device |
| drivers/keyboard.rs | Input keyboard PS/2/legacy ke antrean terminal |
| drivers/serial.rs | Serial COM1 untuk early boot/kernel log |
| drivers/rtc.rs | RTC CMOS untuk waktu real-time |
| drivers/acpi/mod.rs | Inisialisasi ACPI dan entry point power management |
| drivers/acpi/aml.rs | Parser/evaluator subset ACPI Machine Language |
| drivers/acpi/power.rs | Evaluasi power resource perangkat PCI seperti _PR0/_PS0 |
| drivers/qca9377.rs | Orkestrasi/bring-up driver Wi-Fi Qualcomm QCA9377 |
| drivers/qca9377/bmi.rs | BMI command protocol untuk komunikasi boot firmware |
| drivers/qca9377/ce.rs | Copy Engine descriptor/ring dan register |
| drivers/qca9377/htc.rs | HTC transport/service layer |
| drivers/qca9377/htt.rs | HTT data path framing |
| drivers/qca9377/init.rs | Urutan init device, firmware, endpoint, dan runtime state |
| drivers/qca9377/target.rs | Register/sequence reset dan target-specific helper |
| drivers/qca9377/wmi.rs | WMI control command/event |
| drivers/qca9377/wmi_tlv.rs | Encoding/decoding WMI TLV |
| drivers/wifi_firmware.rs | Representasi dan pencarian blob firmware/board data |
| drivers/usb/mod.rs | Facade USB, jenis controller, EndpointInfo, build switch xHCI |
| drivers/usb/controller.rs | PCI matching/probe host controller xHCI/EHCI/OHCI |
| drivers/usb/xhci.rs | xHCI register, command/transfer/event ring, slot, endpoint, transfer |
| drivers/usb/ehci.rs | EHCI host controller, schedule, port, control/bulk transfer |
| drivers/usb/ohci.rs | OHCI host controller, HCCA, ED/TD, port, transfer |
| drivers/usb/descriptors.rs | Parser configuration/interface/endpoint descriptor |
| drivers/usb/device.rs | Enumerasi USB device, grouping interface, bind class driver, hotplug cleanup |
| drivers/usb/hid_periodic.rs | Polling endpoint HID periodik pada jalur controller non-xHCI |
| drivers/usb/keyboard_usb.rs | HID boot keyboard → input keyboard kernel |
| drivers/usb/mouse_usb.rs | HID boot mouse |
| drivers/usb/mass_storage.rs | USB Mass Storage Bulk-Only Transport → block layer |
| drivers/usb/bulk_in.rs | Helper/jalur penerimaan bulk IN |
| drivers/usb/rndis.rs | CDC/RNDIS USB tethering |
| drivers/usb/ncm.rs | CDC-NCM USB tethering |
| fs/mod.rs | API VFS, mount routing, root filesystem initialization dan seed file |
| fs/ramfs.rs | Filesystem RAM yang volatile |
| fs/devfs.rs | Device filesystem, terminal dan special device node |
| storage/mod.rs | Modul filesystem/block storage |
| storage/block.rs | Trait BlockDevice dan registry block device |
| storage/fat32.rs | Implementasi FAT32 |
| storage/ext2.rs | Implementasi ext2 |
| network.rs | Inisialisasi network, link, DHCP/packet processing dan polling TCP |
| network/dns.rs | DNS packet/query/response helper |
| network/tcp.rs | State dan protokol TCP |
| network/http.rs | Parser/klien HTTP sederhana |
| graphics/mod.rs | Framebuffer dan operasi grafis |
| graphics/color.rs | Representasi dan helper warna |
| terminal/mod.rs | Virtual console, buffer tampilan, input/output terminal |
| terminal/font.rs | Bitmap font untuk rendering karakter |
| shell.rs | Shell interaktif, built-in commands, command dispatch, PATH, run/runp |
| klog.rs | Buffer kernel log untuk diagnostik dan penyimpanan log |
| toolchain.rs | Toolchain mini bawaan OS (assembler/compiler/linker subset) |

File yang berada dalam satu modul bukan sekadar file utilitas bebas: modul induk
biasanya menentukan API yang diekspor dan titik integrasi. Sebelum menambah file,
periksa mod.rs terkait, tipe state yang sudah ada, serta test di bagian bawah
file atau modul tersebut.

### 2.2 Peta userspace

| Path | Fungsi |
| --- | --- |
| user/runtime.c | Implementasi libc OCTAGRAM: wrapper syscall, POSIX subset, stdio, allocator, pthread, socket, loader hooks |
| user/crt0.S | _start, membaca argc/argv/envp, menyiapkan runtime/TLS, memanggil main, lalu exit |
| user/setjmp.S | Implementasi assembly setjmp/longjmp |
| user/linker.ld | Linker script executable statis OCTAGRAM |
| user/include/octagram.h | Definisi syscall wrapper dan ABI khusus OCTAGRAM |
| user/include/*.h, user/include/sys/, user/include/netinet/ | Header C yang menjadi kontrak aplikasi dan libc |
| user/ldso/loader.c | Dynamic linker/loader dan relocation ELF |
| user/ldso/start.S | Entry/transfer assembly loader |
| user/ldso/transfer.S | Transfer control ke executable setelah load |
| user/ldso/linker.ld | Linker script dynamic loader |
| user/ldso/dynlib.versions | Version script untuk library demo |
| user/Makefile | Target program C, runtime, TinyCC, shared library dan test |
| user/examples/*.c | Program contoh dan regression test; lihat PROGRAMS di Makefile untuk daftar yang dibangun |
| user/tests/*.c | Test host untuk bagian runtime C yang bisa dieksekusi tanpa syscall kernel OCTAGRAM |
| user/rust/Makefile | Build aplikasi Rust no_std dan menghasilkan ELF |
| user/rust/runtime/src/lib.rs | Runtime Rust OCTAGRAM, syscall dan allocator |
| user/rust/hello/src/main.rs | Contoh aplikasi Rust minimal |
| user/rust/make/src/main.rs | Implementasi utility make yang disertakan ke filesystem awal |
| user/third_party/tinycc/ | TinyCC upstream + konfigurasi build OCTAGRAM; perubahan vendor harus dipisahkan dari runtime sendiri |

## 3. Alur boot dan tempat menambahkan kode

Urutan nyata pada x86_64 saat ini:

1. Limine memanggil kmain di kernel/src/main.rs.
2. drivers::early_init() mengaktifkan serial COM1 agar log awal tetap terlihat.
3. Kernel menyiapkan FPU, GDT/TSS, IDT, RSDP, PIC/APIC dan interrupt.
4. memory::init() menyiapkan alokasi memori; fs::init() menyiapkan VFS
   root RAM dan file/program bawaan.
5. drivers::initialize() menginisialisasi ACPI, mendaftarkan driver, scan PCI,
   dan mencoba bind controller/perangkat.
6. Network lease dipulihkan, timer LAPIC dimulai, shell task dibuat untuk setiap
   virtual terminal.
7. Idle loop menjalankan task siap, menunggu interrupt, memproses unknown IRQ
   diagnostics, lalu memanggil drivers::poll_deferred() dan TCP polling.

Jangan memindahkan scan hardware ke sebelum allocator/paging siap. Jangan
melakukan enumerasi USB, alokasi, atau operasi lama dari ISR. Gunakan interrupt
untuk mencatat event lalu kerjakan di deferred context.

## 4. Build lingkungan pengembangan

Pada Debian/Ubuntu/WSL untuk x86_64:

    sudo apt update
    sudo apt install -y build-essential binutils nasm lld xorriso mtools gdisk qemu-system-x86 python3 curl git

Pasang Rust nightly dan target kernel:

    rustup toolchain install nightly --component rustfmt --target x86_64-unknown-none

Target Rust userspace juga menggunakan linker ld.lld; paket lld menyediakan
linker tersebut di runner Linux umum. Jika build melaporkan linker 'ld.lld'
not found, cek command -v ld.lld dan instal paket lld.

### Urutan build dari clean checkout

Kernel menyertakan beberapa binary userspace melalui include_bytes! dari
kernel/src/fs/mod.rs. Karena itu, buat artefak userspace **sebelum** menjalankan
cargo test kernel:

    make -C user/rust make
    make -C user
    make -C user dynamic
    cargo +nightly fmt --manifest-path kernel/Cargo.toml --check
    cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
    make -C kernel KARCH=x86_64
    make all

Build target terpisah:
- make -C user: ELF program C, TinyCC, runtime statis, demo.
- make -C user dynamic: shared libc, dynamic loader, PIE/dynamic examples.
- make -C user/rust make: membangun user/rust/build/make.elf yang di-embed kernel.
- cargo +nightly test ...: test helper/modul kernel di host Linux.
- make -C kernel KARCH=x86_64: kernel bare-metal.
- make all: kernel + ISO Limine OCTAGRAM-x86_64.iso.

Bila mengubah header userspace, bersihkan artefak lama agar semua program
dikompilasi ulang: make -C user clean. Bila mengubah ABI/linker/layout, lakukan
clean build yang sesuai dan jangan hanya mengandalkan incremental build.

## 5. Menjalankan dan debug

    make run                 # image ISO + QEMU/UEFI pada target x86_64
    make run-bios            # ISO melalui QEMU BIOS
    make run-hdd             # image HDD

Untuk memberi QEMU xHCI dan keyboard USB secara eksplisit, gunakan flag QEMU
yang sesuai dengan target:
make run QEMUFLAGS="-m 2G -device qemu-xhci,id=xhci -device usb-kbd,bus=xhci.0".
Periksa target Makefile jika ingin menambah perangkat QEMU lain.

Pemeriksaan saat runtime:
- help: perintah shell.
- devices: block device yang dapat di-mount.
- drivers: driver yang terdaftar.
- lsdev: perangkat dan binding driver.
- sched: status task/proses dan scheduler.
- savelog: simpan kernel log untuk dianalisis.

Log boot kernel dikirim ke COM1. Untuk debugging QEMU dari host, arahkan serial
ke stdio/log file, misalnya QEMUFLAGS="-m 2G -serial stdio" jika target
Makefile mendukung flag tersebut.

## 6. Alur kerja perubahan kernel

Untuk fitur baru:

1. Temukan owner modul melalui peta file dan mod.rs.
2. Baca tipe data, invariant, call sites, dan test yang sudah ada.
3. Definisikan error path sebelum menulis jalur sukses.
4. Pisahkan parser/state machine murni dari akses register.
5. Tulis unit test host untuk parser, batas input, state transitions, dan failure.
6. Tambahkan integrasi ke modul induk/registry hanya setelah primitive diuji.
7. Build dari clean dependency order.
8. Boot QEMU atau hardware yang sesuai dan uji perilaku sebenarnya.
9. Update docs dan roadmap; jangan tandai selesai hanya karena kode berhasil compile.

### Aturan keselamatan driver

- MMIO harus dipetakan melalui paging helper dan diakses sesuai aturan volatile.
- Jangan pernah menganggap virtual address sama dengan physical/DMA address.
- DMA buffer harus aligned, berada pada rentang yang bisa diakses controller,
  dan tetap hidup sampai transfer benar-benar berhenti.
- Jangan aktifkan PCI Bus Master sebelum buffer/ring DMA siap.
- Setiap polling register/controller harus memiliki timeout.
- Jangan memegang spinlock saat menunggu tanpa batas.
- ISR singkat: acknowledge/record event; alokasi dan pekerjaan kompleks di
  deferred context.
- unsafe harus menjelaskan invariant ownership, alignment, lifetime, dan
  register state yang dijaga.

## 7. Panduan lanjutan

- [Tutorial driver PCI](pci-driver-tutorial.md)
- [Tutorial driver USB](usb-driver-tutorial.md)
- [Kontrak umum driver](writing-drivers.md)
- [Peta subsistem driver](drivers.md)
- [Boot dan init](boot.md)
- [Memory](memory.md)
- [Testing](testing.md)
- [Syscall ABI](syscall-abi.md)
- [Limitations](limitations.md)
