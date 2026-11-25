# user/toolchain — cross toolchain for `x86_64-unknown-octagram`

Direktori ini bukan toolchain baru: ia membungkus GCC host supaya dapat dipakai
sebagai compiler target OCTAGRAM oleh build system yang membutuhkan satu
program compiler (CMake, rustc). Semua flag, header, dan library berasal dari
userspace OCTAGRAM yang sama dengan yang dipakai kernel image.

## Isi

| Path | Peran |
| --- | --- |
| `bin/octagram-cc` | driver bersama: flag freestanding, sysroot, crt0, `libc.a`/`libc++.a` |
| `bin/x86_64-octagram-gcc` | wrapper C (`CMAKE_C_COMPILER`) |
| `bin/x86_64-octagram-g++` | wrapper C++ (`CMAKE_CXX_COMPILER`), `-fno-exceptions -fno-rtti` |
| `bin/x86_64-octagram-ld` | `ld` host dengan `-m elf_x86_64 -static -nostdlib -T octagram.ld` |
| `octagram-toolchain.cmake` | CMake toolchain file untuk build libLLVM (M4.3.3) |

## Sysroot

Sysroot dibuat dari artefak build userspace, bukan dari salinan manual:

```sh
make -C user sysroot
# hasil: user/build/sysroot-octagram/
#   usr/include/            header publik OCTAGRAM
#   usr/include/c++/v1/     header libc++ 18
#   usr/include/c++/src/    header shim libc++ (__has_include_next)
#   usr/lib/crt0.o          _start, TLS, .init_array/.fini_array
#   usr/lib/libc.a          runtime, posix_extra, regex, math, compiler_rt
#   usr/lib/libc++.a        objek libc++ 18
#   usr/lib/libcxxrt.a      cxx_runtime + libcxx_runtime
#   usr/lib/octagram.ld     user/linker.ld
#   sysroot.json            manifest file + sha256
```

`sysroot.json` mencatat hash setiap file, jadi build toolchain dapat dibandingkan
antar mesin. Karena dibangun dari objek yang sama dengan program guest, sysroot
tidak bisa menyimpang dari image.

## Pemakaian

```sh
export OCTAGRAM_SYSROOT="$PWD/user/build/sysroot-octagram"
user/toolchain/bin/x86_64-octagram-gcc -O2 -c hello.c -o hello.o
user/toolchain/bin/x86_64-octagram-gcc hello.o -o hello.elf

cmake -G Ninja -S llvm -B build/llvm \
  -DCMAKE_TOOLCHAIN_FILE="$PWD/user/toolchain/octagram-toolchain.cmake"
```

Untuk rustc, driver dipakai sebagai linker eksternal:

```sh
rustc -C linker=user/toolchain/bin/x86_64-octagram-gcc ...
```

## Batasan yang disengaja

- Bukan cross-compiler sejati: target adalah x86_64 SysV dengan libc OCTAGRAM,
  dan toolchain host tetap GCC. Ini cukup untuk ABI userspace saat ini
  (static `ET_EXEC`, tanpa dynamic loader, TLS local-exec/initial-exec).
- `-nostdlib` selalu dipakai; startup object ditambahkan otomatis kecuali
  argumen berisi `crt0.o` atau `-nostartfiles`.
- Header freestanding GCC (`stdarg.h`, `stddef.h`) tetap dari instalasi host,
  semua header lain dari sysroot agar header glibc tidak pernah terpakai.
- Belum ada `libsupc++`, exception, atau RTTI. Kode C++ target harus
  `-fno-exceptions -fno-rtti`, sama seperti `CXXFLAGS` di `user/Makefile`.
