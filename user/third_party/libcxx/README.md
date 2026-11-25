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

## What is vendored now

- `include/` — libc++ headers (see above).
- `src/` — libc++'s compiled-library sources from the same tag, unmodified.
  They are built into `user/build/libc++.a` by `make -C user`.

Only the subset needed by the current guest probe is compiled so far
(`string`, `vector`, `algorithm`, `stdexcept`, `mutex`, `mutex_destructor`,
`error_category`, `system_error`). The rest of `src/` is vendored so later
stages can widen the archive without another download: `iostream`/locale,
`thread`, `filesystem`, and floating-point `<charconv>` still depend on libc
facilities OCTAGRAM does not provide yet.

`user/libcxx_runtime.cpp` supplies the symbols those translation units leave
out instead of compiling the upstream file when the upstream file drags in
unavailable machinery:

- `std::__1::__libcpp_verbose_abort` (so `src/verbose_abort.cpp` is not built);
  aborting on allocation or length errors is the intended behaviour while the
  port has no exceptions.
- `std::terminate`, `std::exception::~exception`, and `std::exception::what`
  (`src/exception.cpp` also contains the C++17-removed `unexpected_handler`).
- the `std::condition_variable` members that `src/mutex.cpp` links against,
  implemented directly over `pthread_cond_*` (`src/condition_variable.cpp`
  depends on `src/thread.cpp` and promise/future support).

`new`/`delete` stay in `user/cxx_runtime.cpp`, so `src/new.cpp` is not built
either.

## Building the library

`user/Makefile` compiles the vendored sources with the same freestanding flags
and include order as the header probe (`-nostdinc -nostdinc++`, libc++ first)
but with `-std=c++20`: `src/algorithm.cpp` instantiates `std::ranges`-based
sort internals that libc++ only declares for C++20 and later. The resulting
`libc++.a` is linked into `build/libcxx_string_smoke.elf`, which the guest
runs as `/bin/libcxx-string-smoke`.

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
cp -r libcxx/include libcxx/src user/third_party/libcxx/
cp libcxx/LICENSE.TXT libcxx/CREDITS.TXT user/third_party/libcxx/
cp libcxx/vendor/llvm/default_assertion_handler.in \
    user/third_party/libcxx/include/__assertion_handler
```

Then re-create `include/__config_site` as described above and run
`make -C user libcxx-header-check build/libc++.a` to see which headers and
library translation units still compile.
