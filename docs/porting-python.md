# Port CPython ke OCTAGRAM-OS

OCTAGRAM menggunakan ELF64 x86_64, syscall `int 0x80`, dan libc user-space
ringkas. Karena ABI ini bukan ABI Linux/glibc, binary Linux CPython tidak dapat
dijalankan langsung.

## Persiapan source

Target build reproducible tersedia untuk CPython 3.11 dan 3.12:

```sh
make python311
make python312
# atau unduh dan siapkan keduanya
make python
```

Script `tools/cpython_port.py` mengambil source resmi dari python.org ke
`build/cpython/`, lalu mencoba konfigurasi cross-build dengan include libc
OCTAGRAM. Satu source tidak boleh dianggap terport sebelum executable ELF
berhasil ditautkan, ditempatkan di `/bin/python3.11` atau `/bin/python3.12`,
dan melewati smoke test boot.

## API yang masih perlu ditutup

Port penuh perlu memperluas runtime user untuk beberapa area CPython:

- `dlopen`/`dlfcn`, `clock_*`, dan dynamic-module loading
- `getaddrinfo`/`freeaddrinfo` serta DNS/IPv4 networking tambahan
- filesystem/stat layout lengkap untuk data `stat` Python
- locale, `termios`, `pty`, resource, dan signal edge cases
- thread/Pthread parity untuk interpreter free-threading dan worker modules
- standard-library modules yang depend pada API POSIX tambahan

Status pengujian terakhir: source 3.12.10 berhasil diunduh dan cross-target
`x86_64-octagram` dikenali, termasuk bootstrap build-Python 3.12 host.
Cross-config berhasil selesai dan lapisan POSIX user-space diperluas untuk
CPython, termasuk `pread/pwrite/readv/writev`, descriptor vector, resource API,
signal compatibility, spawn, identity, dan filesystem helpers. Build lengkap
Python masih tetap experimental sampai executable OCTAGRAM melewati smoke test
interaktif; jangan memasukkan binary Linux/non-ABI ke `/bin`.

Jangan menggunakan konfigurasi `host=x86_64-octagram` sebagai bukti port:
build baru dianggap selesai setelah berhasil menghasilkan ELF OCTAGRAM, muncul
sebagai `/bin/python*` di VFS awal, dan melewati interpreter smoke test.
