# Proses, thread, dan sinyal

Ada dua model eksekusi di OCTAGRAM: task kernel kooperatif (`kernel/src/task.rs`)
dan proses user preemptive (`kernel/src/elf.rs`). Keduanya berbagi satu kernel
address space dan satu VFS global, tetapi hanya proses user yang punya page table
dan descriptor sendiri.

## Task kernel

- `task::spawn(future)` mendaftarkan future `Send + 'static`.
- `task::yield_now().await` menyerahkan giliran secara sukarela.
- `TaskHandle::wake()` hanya mengubah flag atomik, jadi aman dipanggil dari
  interrupt handler (dipakai keyboard untuk membangunkan shell).
- Idle loop mem-poll task yang siap sebelum `hlt`.

Task kernel **tidak** dipreempt dan berbagi hak akses kernel. Task yang
memblokir tanpa `yield` menghentikan seluruh kernel; karena itu jangan memanggil
`task::spawn` dari interrupt handler.

## Proses user

Batas dan sumber daya:

| Properti | Nilai | Lokasi |
| --- | --- | --- |
| Proses terjadwal maksimum | 64 | `MAX_SCHEDULED_PROCESSES` |
| Kernel stack per proses | 64 KiB | `PROCESS_KERNEL_STACK_SIZE` |
| Stack user | 8 page (32 KiB) | `STACK_PAGES` |
| Group/descriptor | per proses | tabel descriptor di `UserProcess` |

Setiap proses memiliki PID/TID, page table privat, daftar descriptor, current
working directory, `brk`, kernel stack, status sinyal, dan status keluar. Program
mewarisi cwd dari shell dan berbagi namespace VFS serta daftar mount global.

### Penjadwalan

`run` menjalankan satu program sampai selesai. `runp a.elf b.elf ...` menjalankan
beberapa program dengan round-robin yang dipicu timer LAPIC periodik. Preemption
hanya terjadi bila interrupt datang dari ring 3; interrupt saat kernel menjalankan
syscall tidak mempreempt agar operasi yang memegang lock tidak terputus.

Syscall yang menunggu (pipe, `poll`, futex, `waitpid`, tidur) memakai
`ProcessWait` sehingga proses tidur sampai event atau deadline, bukan busy-loop:
`Child(pid)`, `Thread(tid)`, `PipeReadable(pipe)`, `PipeWritable(pipe)`,
`Futex { address, bitset }`, `PollWait`, dan `Sleep`.

### Membuat dan mengakhiri proses

- `fork` menyalin halaman user secara eager (tanpa copy-on-write).
- `execve`/`execvp` memuat image ELF baru ke page table proses yang sama;
  stack baru diberi entropi, signal handler di-reset sesuai aturan `exec`.
- `waitpid` menerima PID tertentu atau `WAIT_ANY_PID`; status keluar dan sinyal
  pelapor disimpan sampai diambil induk.
- Saat proses keluar atau fault, scheduler memilih proses berikutnya dan page
  milik proses dilepas setelah grupnya selesai.

### Sinyal

Sinyal dikirim lewat `kill`/`tgkill`, disimpan sebagai bitmask pending, dan
disampaikan saat proses kembali ke ring 3. Yang tersedia:

- `sigaction`/`signal`, `sigprocmask`, `sigpending`, `sigsuspend`, `sigreturn`;
- alternate signal stack lewat `sigaltstack` (syscall 133) dengan `SA_ONSTACK`,
  `SS_ONSTACK`, dan `SS_DISABLE`;
- `SIGKILL`/`SIGSTOP` tidak dapat ditahan; sinyal lain bisa diabaikan atau
  ditangani handler user.

### Grup dan sesi

`setpgid`/`getpgrp` (130/129) dan `setsid`/`getsid` (132/131) tersedia. Grup
proses dipakai untuk pengiriman sinyal (`kill(-pgid, ...)`).

## Sinkronisasi dan futex

`pthread_mutex`, condition variable, dan semaphore di libc dibangun di atas
syscall `futex` (94). Kernel mendukung:

- `FUTEX_WAIT`/`FUTEX_WAKE` dengan timeout relatif dan deadline LAPIC;
- `FUTEX_WAIT_BITSET` (timeout absolut, `FUTEX_CLOCK_REALTIME`),
  `FUTEX_WAKE_BITSET`, dan `FUTEX_REQUEUE`;
- flag private (`FUTEX_PRIVATE_FLAG`).

Robust futex (daftar `FUTEX_OWNER_DIED`) belum ada, begitu pula prioritas
scheduler/aging dan watchdog.

## Batasan

- Tanpa prioritas, tanpa real-time scheduling, tanpa fairness terukur.
- Tanpa SMP dan tanpa migrasi thread antar CPU.
- Thread user berbagi address space tetapi belum punya TLS dinamis penuh untuk
  shared object (lihat [dynamic-linker.md](dynamic-linker.md)).
- Blocking syscall tertentu masih menunggu di kernel dengan polling input, bukan
  sepenuhnya digerakkan event.

## Uji

`cargo test` menyertakan uji host untuk helper penjadwalan (`next_runnable`),
pencocokan bitset futex, `wait_target_matches`, tata letak stack, dan
`signal_stack_top`. Perilaku runtime penuh hanya bisa dilihat dengan menjalankan
OS di QEMU.
