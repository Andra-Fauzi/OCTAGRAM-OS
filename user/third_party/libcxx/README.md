# Vendored libc++ headers

OCTAGRAM builds its C++ programs freestanding (`-ffreestanding -fno-exceptions
-fno-rtti -nostdinc++`), so it needs a C++ standard library that carries no
dependency on a hosted C library. This directory contains the libc++ **headers**
from LLVM 18.1.8 (`llvmorg-18.1.8`), which are what makes `std::` containers and
traits available to the guest.

## What is here

- `include/` — `libcxx/include` from that tag, unmodified, except for the two
  files below.
- `LICENSE.TXT`, `CREDITS.TXT` — upstream license and credits (Apache-2.0 with
  LLVM exceptions).

Only headers are vendored: libc++'s compiled library (`src/`) is not built yet.
That is why the build links against OCTAGRAM's own runtime (`user/runtime.c`,
`user/cxx_runtime.cpp`, `user/compiler_rt.c`) instead of `libc++.a`, and why
`user/libcxx_runtime.cpp` supplies the one runtime symbol the headers need
(`std::__1::__libcpp_verbose_abort`).

## Files OCTAGRAM generates

Upstream generates these with CMake. They are checked in here because the ABI
choice belongs to the port, not to a build directory:

- `include/__config_site` — selects the default ABI namespace (`std::__1`; without
  this the inline namespace is literally named `_LIBCPP_ABI_NAMESPACE`, which
  still compiles but mangles every `std::` symbol wrongly), turns
  hardening off (a hardening failure would abort a guest program with no way to
  report it), selects the serial PSTL backend (there is no thread pool yet), and
  sets `_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE` because OCTAGRAM does not implement
  the glibc/musl/newlib locale internals libc++ would otherwise detect.
- `include/__assertion_handler` — copied from
  `libcxx/vendor/llvm/default_assertion_handler.in`.

## Updating

```sh
git clone --filter=blob:none --sparse --depth 1 --branch llvmorg-18.1.8 \
    https://github.com/llvm/llvm-project /tmp/llvm
cd /tmp/llvm && git sparse-checkout set libcxx
cp -r libcxx/include user/third_party/libcxx/
cp libcxx/LICENSE.TXT libcxx/CREDITS.TXT user/third_party/libcxx/
cp libcxx/vendor/llvm/default_assertion_handler.in \
    user/third_party/libcxx/include/__assertion_handler
```

Then re-create `include/__config_site` as described above and run
`make -C user libcxx-header-check` to see which headers still compile.
