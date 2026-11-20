# Rust host ABI dependency matrix — OCTAGRAM-OS

Status audit: 20 November 2026.
Branch: `feature/native-self-hosting`.
Machine-readable source of truth: [`tools/rust_host_abi_matrix.json`](../tools/rust_host_abi_matrix.json).
CI validator: [`tools/check_rust_host_abi_matrix.py`](../tools/check_rust_host_abi_matrix.py).

## 1. Triple yang dipakai

| Role | Triple | Fungsi |
| --- | --- | --- |
| Build | `x86_64-unknown-linux-gnu` | Menjalankan bootstrap awal dari Linux/CI |
| Host | `x86_64-unknown-octagram` | OS yang kelak menjalankan executable `rustc`, Cargo, dan backend |
| Kernel target | `x86_64-unknown-none` | Kernel freestanding yang dibangun compiler host |

Jangan menyamakan `--target x86_64-unknown-octagram` pada Cargo Linux dengan build host compiler. Saat ini Cargo/rustc Linux menghasilkan satu guest ELF bertarget OCTAGRAM; yang berjalan pada OCTAGRAM adalah ELF guest tersebut, bukan compiler yang menghasilkannya.

## 2. Status evidence

| Status | Arti |
| --- | --- |
| `qemu_tested` | Jalur executable diuji berjalan di QEMU dan marker/exit code diamati |
| `build_tested` | Kompilasi/link sukses, tetapi belum membuktikan runtime target |
| `implemented_partial` | Source API ada, tetapi semantik atau cakupan kompatibilitas belum lengkap |
| `not_implemented` | Tidak ada executable native/implementasi target yang terbukti |

Status per komponen, daftar bukti, batas semantik, dan gate berikutnya ada dalam JSON. Validator CI hanya memastikan matriks lengkap dan tidak mengizinkan status compiler `std`/`rustc`/Cargo/backend dinaikkan tanpa bukti baru; validator bukan pengganti tes runtime.

## 3. Temuan yang mempengaruhi port compiler

- **Fondasi yang sudah teruji:** `core`, `alloc`, guest allocator/`Vec`, file/stat, anonymous `mmap`, static TLS, thread preemption, FPU/SSE preservation, dan custom-target executable.
- **Process/filesystem:** ada `fork`, `execve`, `waitpid`, pipe, metadata, rename, `*at`, environment, tetapi limits dan error semantics OCTAGRAM berbeda dari Linux.
- **Synchronization:** futex kernel mendukung WAIT/WAKE, WAIT_BITSET/WAKE_BITSET, dan REQUEUE. Waiter dicari pada PID yang sama; shared futex lintas proses tidak tersedia. `CMP_REQUEUE`, `WAKE_OP`, dan priority-inheritance ops belum ada.
- **Time/entropy:** monotonic clock beresolusi sekitar 1 ms; realtime berasal dari RTC beresolusi satu detik; `getrandom` saat ini menggunakan xorshift seeded TSC. Ini belum cukup untuk mengklaim kontrak entropy kriptografis bagi compiler/package manager.
- **Dynamic linking:** ELF static adalah jalur paling aman untuk Stage 0. Loader PIE/`PT_INTERP` ada tetapi tidak setara Linux; TLS pada shared object dan banyak relocation/runtime edges belum menjadi kontrak umum.
- **Networking:** vendoring dependency lebih dulu lebih realistis daripada mengandalkan crates.io; `sendmsg`/`recvmsg` masih `ENOSYS` dan socket coverage terbatas.

## 4. Urutan kerja yang direkomendasikan

1. Tutup conformance gap process/file/mmap/thread/futex yang dibutuhkan oleh dependency nyata dari Rust compiler host, bukan seluruh Linux ABI.
2. Tentukan Stage 0 compiler dan sysroot yang dapat dijalankan pada build machine; cross-bootstrap host `x86_64-unknown-octagram` memakai build/host/target triple terpisah.
3. Pilih strategi backend: build LLVM/LLD native, bawa backend sebagai library, atau cari backend alternatif. Jangan anggap TinyCC sebagai pengganti.
4. Bangun subset `std` dan runtime yang diperlukan untuk menjalankan rustc; baru kemudian Cargo offline.
5. Buktikan rustc native mengompilasi target kernel `x86_64-unknown-none`, lalu jalankan Stage 1 → Stage 2.

## 5. Batas klaim

Pada audit ini **belum ada** rustc native, Cargo native, Rust `std` OCTAGRAM, compiler backend native, build kernel dari dalam OCTAGRAM, image builder native, ataupun A/B self-update/rollback. Keberhasilan `no_std + alloc` guest adalah prasyarat yang berguna, tetapi belum full self-hosting.
