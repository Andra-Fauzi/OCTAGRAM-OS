CROSS_COMPILE ?=
CC := $(CROSS_COMPILE)gcc
CXX := $(CROSS_COMPILE)g++
AS := $(CROSS_COMPILE)as
LD := $(CROSS_COMPILE)ld
AR := $(CROSS_COMPILE)ar
HOSTCC ?= gcc
HOSTAS ?= as
HOSTLD ?= ld

CFLAGS := -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
	-fno-stack-protector -fno-pie -fno-pic -mno-red-zone -mcmodel=small \
	-march=x86-64 -mtune=generic -mno-avx -mno-avx2 -mno-avx512f \
	-mno-mmx -msse2 -mfpmath=387 -DOCTAGRAM_TLS_ERRNO \
	-Iinclude
LDFLAGS := -m elf_x86_64 -static -nostdlib -T linker.ld
# Freestanding C++: no libstdc++/libc++ headers or library, no exceptions and
# no RTTI, so the resulting object only needs OCTAGRAM's C runtime.
CXXFLAGS := -std=c++17 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
	-fno-stack-protector -fno-pie -fno-pic -mno-red-zone -mcmodel=small \
	-march=x86-64 -mtune=generic -mno-avx -mno-avx2 -mno-avx512f \
	-mno-mmx -msse2 -fno-exceptions -fno-rtti -fno-threadsafe-statics \
	-nostdinc++ -Iinclude
PROGRAMS := sh hello file_demo runtime_demo tty_demo argv_demo spawn_demo pipe_demo env_demo posix_demo stdio_demo memstream_demo posix_search_demo posix_spawn_demo file_api_demo path_api_demo at_api_demo execve_demo fork_demo time_demo mmap_demo scanf_demo tls_demo thread_demo thread_tsd_demo fb_demo syscall_compat_demo account_demo socket_msg_demo netdb_demo dns_demo cxa_atexit_demo credential_demo robust_mutex_demo
BINARIES := $(addprefix build/,$(addsuffix .elf,$(PROGRAMS)))
HEADERS := $(wildcard include/*.h include/sys/*.h include/netinet/*.h)
# Vendored libc++ headers, including __config_site. Without this dependency the
# archive keeps objects compiled against an older configuration.
LIBCXX_HEADERS := $(shell find third_party/libcxx/include -name '*.h' -o -name '*.inc' 2>/dev/null)
TCC_SOURCES := tcc libtcc tccpp tccgen tccdbg tccelf tccasm tccrun \
	x86_64-gen x86_64-link i386-asm
TCC_OBJECTS := $(addprefix build/tcc_,$(addsuffix .o,$(TCC_SOURCES)))
TCC_HEADERS := $(wildcard third_party/tinycc/*.h third_party/tinycc/*.c)
# TinyCC's `./configure` generates this header and its `.gitignore` keeps it out
# of the repository, so a fresh clone does not have it. The template carries the
# values this freestanding build needs; the rule below only copies it when the
# generated file is missing.
TCC_CONFIG := third_party/tinycc/config.h
TCC_CONFIG_TEMPLATE := third_party/tinycc/config.h.octagram
TCC_ELF := build/tcc.elf
TCC_CFLAGS := $(CFLAGS) -w -DONE_SOURCE=0 -DCONFIG_TCC_STATIC \
	-DTARGETOS_OCTAGRAM -Ithird_party/tinycc -Ithird_party/tinycc/include \
	-DCONFIG_SYSROOT=\"\" -DCONFIG_TCCDIR=\"/lib/tcc\" \
	-DCONFIG_TCC_CRTPREFIX=\"/lib/tcc\" -DCONFIG_TCC_LIBPATHS=\"{B}\" \
	-DCONFIG_TCC_SYSINCLUDEPATHS=\"/include\"
PIC_CFLAGS := $(filter-out -fno-pic -fno-pie -DOCTAGRAM_TLS_ERRNO,$(CFLAGS)) -fPIC \
	-DOCTAGRAM_RUNTIME_ERRNO_ACCESSOR
RUNTIME_PIC_CFLAGS := $(PIC_CFLAGS) -DOCTAGRAM_SHARED
TCC_SETJMP := build/setjmp.o
TCC_SUPPORT_SOURCES := libtcc1 stdatomic builtin va_list dsohandle overflow
TCC_SUPPORT_ASSEMBLY := atomic alloca alloca-bt
TCC_SUPPORT_OBJECTS := $(addprefix build/tcc_support_,$(addsuffix .o,$(TCC_SUPPORT_SOURCES))) \
	$(addprefix build/tcc_support_,$(addsuffix .o,$(TCC_SUPPORT_ASSEMBLY)))
TCC_SUPPORT_CFLAGS := $(CFLAGS) -fPIC -fno-omit-frame-pointer \
	-w -Wno-error -Wno-unused-function -Wno-unused-variable \
	-Ithird_party/tinycc
TCC_LIBTCC1 := build/libtcc1.a
TCC_LIBC := build/libc.a
LDSO_ELF := build/ld-octagram.elf
DYNAMIC_DEMO := build/dynamic_demo.elf
COPY_DEMO := build/copy_reloc_demo.elf
VERSIONED_DEMO := build/versioned_demo.elf
DLOPEN_DEMO := build/dlopen_demo.elf
ASLR_DEMO := build/aslr_demo.elf
SHARED_ERRNO_DEMO := build/shared_errno_demo.elf
DYNLIB_VERSIONS := ldso/dynlib.versions
DYNAMIC_LIBRARY := build/libdyn_demo.so
SHARED_LIBC := build/liboctagram.so
LDSO_OBJECTS := build/ldso_start.o build/ldso_transfer.o build/ldso_loader.o

.PHONY: all clean rust scanf-test cxa-test resolv-test ipv6-test stdint-test math-header-test overflow-test math-test wait-status-test wctype-test compiler-rt-test cpp-header-test libcxx-header-check dynamic
CPP_SMOKE := build/cpp_smoke.elf

all: $(BINARIES) $(CPP_SMOKE) $(LIBCXX_SMOKE) $(LIBCXX_STRING_SMOKE) $(LIBCXX_IOSTREAM_SMOKE) $(LIBCXX_THREAD_SMOKE) $(LIBCXX_FS_SMOKE) $(LIBCXX_CONTAINER_SMOKE) $(LIBCXX_FORMAT_SMOKE) $(OCTAGRAM_CARGO) $(TCC_ELF) $(TCC_LIBTCC1) $(TCC_LIBC) $(LDSO_ELF) $(DYNAMIC_DEMO) $(COPY_DEMO) $(VERSIONED_DEMO) $(DLOPEN_DEMO) $(ASLR_DEMO)

tcc: $(TCC_ELF) $(TCC_LIBTCC1) $(TCC_LIBC)

dynamic: $(LDSO_ELF) $(SHARED_LIBC) $(DYNAMIC_LIBRARY) $(DYNAMIC_DEMO) $(COPY_DEMO) $(VERSIONED_DEMO) $(DLOPEN_DEMO) $(ASLR_DEMO) $(SHARED_ERRNO_DEMO)

scanf-test: build/scanf_runtime_test
	./build/scanf_runtime_test

cxa-test: build/cxa_atexit_test
	./build/cxa_atexit_test

resolv-test: build/resolv_conf_test
	./build/resolv_conf_test

ipv6-test: build/ipv6_test
	./build/ipv6_test

wait-status-test: build/wait_status_test
	./build/wait_status_test

build/wait_status_test: tests/wait_status_test.c include/sys/wait.h | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -Iinclude -o $@ $<

stdint-test:
	$(HOSTCC) -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/stdint_header_test.c

math-header-test: build/math_header_test
	./build/math_header_test

build/math_header_test: tests/math_header_test.c math.c | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin -Iinclude -o $@ $^

overflow-test: build/tcc_overflow_runtime_test
	./build/tcc_overflow_runtime_test

math-test: build/math_runtime_test
	./build/math_runtime_test

wctype-test: build/wctype_runtime_test
	./build/wctype_runtime_test

compiler-rt-test: build/compiler_rt_host_test
	./build/compiler_rt_host_test

cpp-header-test:
	python3 ../tools/check_cpp_headers.py

libcxx-header-check:
	python3 ../tools/check_libcxx_headers.py

build/compiler_rt_host_test: tests/compiler_rt_test.c compiler_rt.c | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -o $@ tests/compiler_rt_test.c \
		-D__udivmodti4=oct_udivmodti4 -D__divmodti4=oct_divmodti4 \
		-D__udivti3=oct_udivti3 -D__umodti3=oct_umodti3 \
		-D__divti3=oct_divti3 -D__modti3=oct_modti3 \
		-D__ashlti3=oct_ashlti3 -D__ashrti3=oct_ashrti3 \
		-D__lshrti3=oct_lshrti3 \
		-D__floatuntidf=oct_floatuntidf -D__floattidf=oct_floattidf \
		-D__floatuntisf=oct_floatuntisf -D__floattisf=oct_floattisf \
		-D__fixunsdfti=oct_fixunsdfti -D__fixdfti=oct_fixdfti \
		-D__fixunssfti=oct_fixunssfti -D__fixsfti=oct_fixsfti \
		-D__floatuntixf=oct_floatuntixf -D__floattixf=oct_floattixf \
		-D__fixunsxfti=oct_fixunsxfti -D__fixxfti=oct_fixxfti compiler_rt.c

build/wctype_runtime_test: tests/wctype_runtime_test.c wctype.c include/wctype.h | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-Iinclude -o $@ tests/wctype_runtime_test.c wctype.c

build/math_runtime_test: tests/math_runtime_test.c math.c | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-Iinclude -o $@ $^

build/tcc_overflow_runtime_test: tests/tcc_overflow_runtime_test.c third_party/tinycc/lib/overflow.c | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -fno-builtin -Iinclude -o $@ $^

rust:
	$(MAKE) -C rust

build:
	mkdir -p $@

build/crt0.o: crt0.S | build
	$(AS) --64 -o $@ $<

build/crt0.pic.o: crt0.S | build
	$(AS) --64 -o $@ $<

build/cpp_smoke.o: examples/cpp_smoke.cpp $(HEADERS) | build
	$(CXX) $(CXXFLAGS) -c -o $@ $<

build/cxx_runtime.o: cxx_runtime.cpp $(HEADERS) | build
	$(CXX) $(CXXFLAGS) -c -o $@ $<

# Vendored libc++ 18 headers (see third_party/libcxx/README.md). The generated
# configuration lives in the include directory, so only the search path differs
# from the freestanding C++ build.
LIBCXX_INCLUDE := third_party/libcxx/include
# libc++'s own C headers forward to OCTAGRAM's with #include_next, so the
# libc++ directory must be searched before include/ -- -I would always win over
# -isystem, hence both are -isystem here.
# third_party/libcxx/src is also on the search path: libc++'s own headers use
# __has_include_next(<...>), which needs at least one directory after include/
# even when the next header is never actually used.
# OCTAGRAM keeps errno in TLS; libc++ sources that touch <cerrno> must see the
# same declaration as user/runtime.c, otherwise ld rejects the non-TLS
# reference to the TLS definition of errno.
LIBCXX_FLAGS := -DOCTAGRAM_TLS_ERRNO -nostdinc -nostdinc++ \
	-isystem $(LIBCXX_INCLUDE) -isystem include -isystem third_party/libcxx/src \
	-Wno-error
LIBCXX_CXXFLAGS := $(filter-out -Iinclude,$(CXXFLAGS)) $(LIBCXX_FLAGS)
LIBCXX_SMOKE := build/libcxx_smoke.elf

# Compiled libc++ (libc++.a). Headers alone cannot provide std::string,
# std::sort, or std::mutex: they need libc++'s explicit instantiations and the
# runtime shims in third_party/libcxx/src. The sources are compiled with the
# same freestanding flags and include order as the header probe. C++20 is used
# for the library objects because src/algorithm.cpp instantiates
# std::ranges-based internals that libc++ only declares for C++20 and later.
LIBCXX_SRC := third_party/libcxx/src
LIBCXX_STD ?= c++20
LIBCXX_ARCHIVE := build/libc++.a
LIBCXX_OBJECTS := \
	build/libcxx/string.o \
	build/libcxx/vector.o \
	build/libcxx/algorithm.o \
	build/libcxx/stdexcept.o \
	build/libcxx/mutex.o \
	build/libcxx/mutex_destructor.o \
	build/libcxx/error_category.o \
	build/libcxx/system_error.o \
	build/libcxx/charconv.o \
	build/libcxx/ryu/d2fixed.o \
	build/libcxx/ryu/d2s.o \
	build/libcxx/ryu/f2s.o \
	build/libcxx/locale.o \
	build/libcxx/ios.o \
	build/libcxx/ios.instantiations.o \
	build/libcxx/iostream.o \
	build/libcxx/ostream.o \
	build/libcxx/call_once.o \
	build/libcxx/memory.o \
	build/libcxx/fstream.o \
	build/libcxx/thread.o \
	build/libcxx/future.o \
	build/libcxx/random.o \
	build/libcxx/chrono.o \
	build/libcxx/regex.o \
	build/libcxx/filesystem/path.o \
	build/libcxx/filesystem/directory_entry.o \
	build/libcxx/filesystem/directory_iterator.o \
	build/libcxx/filesystem/operations.o \
	build/libcxx/filesystem/filesystem_clock.o \
	build/libcxx/filesystem/filesystem_error.o \
	build/libcxx/filesystem/int128_builtins.o \
	build/libcxx/hash.o \
	build/libcxx/typeinfo.o \
	build/libcxx/valarray.o \
	build/libcxx/strstream.o \
	build/libcxx/shared_mutex.o \
	build/libcxx/barrier.o \
	build/libcxx/condition_variable_destructor.o \
	build/libcxx/any.o \
	build/libcxx/optional.o \
	build/libcxx/variant.o \
	build/libcxx/functional.o \
	build/libcxx/bind.o \
	build/libcxx/atomic.o \
	build/libcxx/memory_resource.o \
	build/libcxx/print.o \
	build/libcxx/verbose_abort.o \
	build/libcxx/legacy_pointer_safety.o \
	build/libcxx/new_handler.o \
	build/libcxx/new_helpers.o \
	build/libcxx/tz.o \
	build/libcxx/tzdb_list.o
LIBCXX_STRING_SMOKE := build/libcxx_string_smoke.elf
LIBCXX_IOSTREAM_SMOKE := build/libcxx_iostream_smoke.elf
LIBCXX_THREAD_SMOKE := build/libcxx_thread_smoke.elf
LIBCXX_FS_SMOKE := build/libcxx_fs_smoke.elf
# Cargo bootstrap driver: a C++ program that resolves Cargo.toml/Cargo.lock
# offline and invokes rustc. Not upstream Cargo (see user/cargo/main.cpp).
OCTAGRAM_CARGO := build/octagram-cargo.elf
LIBCXX_CONTAINER_SMOKE := build/libcxx_container_smoke.elf
LIBCXX_FORMAT_SMOKE := build/libcxx_format_smoke.elf

# Only the translation units that back the features exercised by the guest
# probe are compiled so far. std::iostream (locale/ios/fstream), std::thread,
# std::filesystem, and floating-point <charconv> still need library pieces and
# libc functions that OCTAGRAM does not have yet.
# Vendored sources are compiled without -Werror: libc++ is not written for
# OCTAGRAM's warning set, and -Walloc-size-larger-than= fires on intentional
# "impossible size" branches inside basic_string.
LIBCXX_LIB_FLAGS := $(filter-out -Werror,$(LIBCXX_CXXFLAGS)) -std=$(LIBCXX_STD)

# std::print/std::println are C++23-only declarations, so print.cpp must be
# compiled as C++23 even though the rest of the archive is C++20.
build/libcxx/print.o: $(LIBCXX_SRC)/print.cpp $(HEADERS) $(LIBCXX_HEADERS) | build
	@mkdir -p $(dir $@)
	$(CXX) $(LIBCXX_LIB_FLAGS) -std=c++23 -c -o $@ $<

build/libcxx/%.o: $(LIBCXX_SRC)/%.cpp $(HEADERS) $(LIBCXX_HEADERS) | build
	@mkdir -p $(dir $@)
	$(CXX) $(LIBCXX_LIB_FLAGS) -c -o $@ $<

$(LIBCXX_ARCHIVE): $(LIBCXX_OBJECTS)
	rm -f $@
	ar rcs $@ $(LIBCXX_OBJECTS)

build/libcxx_string_smoke.o: examples/libcxx_string_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=$(LIBCXX_STD) -c -o $@ $<

$(LIBCXX_STRING_SMOKE): build/libcxx_string_smoke.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		$(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/cxx_runtime.o build/libcxx_runtime.o build/libcxx_string_smoke.o \
		$(LIBCXX_ARCHIVE)

build/libcxx_iostream_smoke.o: examples/libcxx_iostream_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=$(LIBCXX_STD) -c -o $@ $<

$(LIBCXX_IOSTREAM_SMOKE): build/libcxx_iostream_smoke.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		$(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/cxx_runtime.o build/libcxx_runtime.o build/libcxx_iostream_smoke.o \
		$(LIBCXX_ARCHIVE)

build/libcxx_thread_smoke.o: examples/libcxx_thread_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=$(LIBCXX_STD) -c -o $@ $<

$(LIBCXX_THREAD_SMOKE): build/libcxx_thread_smoke.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		build/setjmp.o $(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/setjmp.o build/cxx_runtime.o build/libcxx_runtime.o \
		build/libcxx_thread_smoke.o $(LIBCXX_ARCHIVE)

build/libcxx_fs_smoke.o: examples/libcxx_fs_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=$(LIBCXX_STD) -c -o $@ $<

$(LIBCXX_FS_SMOKE): build/libcxx_fs_smoke.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		build/setjmp.o $(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/setjmp.o build/cxx_runtime.o build/libcxx_runtime.o \
		build/libcxx_fs_smoke.o $(LIBCXX_ARCHIVE)

build/libcxx_container_smoke.o: examples/libcxx_container_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=$(LIBCXX_STD) -c -o $@ $<

$(LIBCXX_CONTAINER_SMOKE): build/libcxx_container_smoke.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		build/setjmp.o $(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/setjmp.o build/cxx_runtime.o build/libcxx_runtime.o \
		build/libcxx_container_smoke.o $(LIBCXX_ARCHIVE)

build/libcxx_format_smoke.o: examples/libcxx_format_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=c++23 -c -o $@ $<

$(LIBCXX_FORMAT_SMOKE): build/libcxx_format_smoke.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		build/setjmp.o $(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/setjmp.o build/cxx_runtime.o build/libcxx_runtime.o \
		build/libcxx_format_smoke.o $(LIBCXX_ARCHIVE)

build/octagram_cargo.o: cargo/main.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -std=$(LIBCXX_STD) -c -o $@ $<

$(OCTAGRAM_CARGO): build/octagram_cargo.o build/libcxx_runtime.o \
		build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o \
		build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o \
		build/setjmp.o $(LIBCXX_ARCHIVE) linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o \
		build/math.o build/math_wide.o build/wchar.o build/wctype.o \
		build/setjmp.o build/cxx_runtime.o build/libcxx_runtime.o \
		build/octagram_cargo.o $(LIBCXX_ARCHIVE)

build/libcxx_runtime.o: libcxx_runtime.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -c -o $@ $<

build/libcxx_smoke.o: examples/libcxx_smoke.cpp $(HEADERS) | build
	$(CXX) $(LIBCXX_CXXFLAGS) -c -o $@ $<

$(LIBCXX_SMOKE): build/libcxx_smoke.o build/libcxx_runtime.o build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o build/math.o build/math_wide.o build/wchar.o build/wctype.o build/cxx_runtime.o build/libcxx_runtime.o build/libcxx_smoke.o

$(CPP_SMOKE): build/cpp_smoke.o build/cxx_runtime.o build/wctype.o build/wchar.o build/math_wide.o build/math.o build/compiler_rt.o build/crt0.o build/runtime.o build/posix_extra.o linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/math.o build/wchar.o build/math_wide.o build/wctype.o build/compiler_rt.o build/cxx_runtime.o build/cpp_smoke.o

build/setjmp.o: setjmp.S | build
	$(AS) --64 -o $@ $<

build/runtime.o: runtime.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

# POSIX/GNU compatibility routines (memmem, fnmatch, glob, getline, mkdtemp,
# affinity and sysinfo queries). Separate from runtime.c because they need only
# the public C API.
build/posix_extra.o: posix_extra.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/wctype.o: wctype.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/compiler_rt.o: compiler_rt.c | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/wchar.o: wchar.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/math_wide.o: math_wide.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/runtime.pic.o: runtime.c $(HEADERS) | build
	$(CC) $(RUNTIME_PIC_CFLAGS) -c -o $@ $<

$(TCC_CONFIG): $(TCC_CONFIG_TEMPLATE)
	cp $< $@

# TinyCC's `include/tccdefs.h` is embedded as a C string by `tccpp.c` when
# `CONFIG_TCC_PREDEFS` is on. Upstream runs its `c2str` helper for that, and the
# result (`tccdefs_.h`, ignored by the same `*_.h` rule) is generated here too,
# so a fresh clone builds without running TinyCC's `./configure`.
TCC_DEFS := third_party/tinycc/tccdefs_.h

$(TCC_DEFS): third_party/tinycc/include/tccdefs.h
	sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/^/"/' -e 's/$$/\\n"/' $< > $@

build/tcc_%.o: third_party/tinycc/%.c $(TCC_HEADERS) $(HEADERS) $(TCC_CONFIG) $(TCC_DEFS) | build
	$(CC) $(TCC_CFLAGS) -c -o $@ $<

build/tcc_support_%.o: third_party/tinycc/lib/%.c $(HEADERS) | build
	$(CC) $(TCC_SUPPORT_CFLAGS) -c -o $@ $<

build/tcc_support_%.o: third_party/tinycc/lib/%.S | build
	$(CC) $(TCC_SUPPORT_CFLAGS) -c -o $@ $<

build/%.o: examples/%.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/ldso_start.o: ldso/start.S | build
	$(AS) --64 -o $@ $<

build/ldso_transfer.o: ldso/transfer.S | build
	$(AS) --64 -o $@ $<

build/ldso_loader.o: ldso/loader.c $(HEADERS) | build
	$(CC) $(CFLAGS) -fno-pic -fno-pie -c -o $@ $<

$(LDSO_ELF): $(LDSO_OBJECTS) ldso/linker.ld
	$(LD) -m elf_x86_64 -static -nostdlib -T ldso/linker.ld -o $@ $(LDSO_OBJECTS)

$(SHARED_LIBC): build/runtime.pic.o
	$(LD) -m elf_x86_64 -shared --hash-style=gnu -soname liboctagram.so \
		-o $@ $<

build/dynlib.o: examples/dynlib.c $(HEADERS) | build
	$(CC) $(PIC_CFLAGS) -c -o $@ $<

$(DYNAMIC_LIBRARY): build/dynlib.o $(DYNLIB_VERSIONS)
	$(LD) -m elf_x86_64 -shared --hash-style=gnu -soname libdyn_demo.so \
		--version-script=$(DYNLIB_VERSIONS) -o $@ $<

build/dynamic_demo.o: examples/dynamic_demo.c $(HEADERS) | build
	$(CC) $(PIC_CFLAGS) -c -o $@ $<

# Executable non-PIC (ET_EXEC) agar ld menghasilkan relokasi R_X86_64_COPY
# untuk data yang berasal dari shared library.
build/copy_reloc_demo.o: examples/copy_reloc_demo.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

# Simbol berversi diperiksa pada executable non-PIC yang sama-sama ET_EXEC.
build/versioned_demo.o: examples/versioned_demo.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

# Demo dlopen tidak menautkan libdyn_demo; library dimuat saat runtime.
build/dlopen_demo.o: examples/dlopen_demo.c $(HEADERS) | build
	$(CC) $(PIC_CFLAGS) -c -o $@ $<

build/aslr_demo.o: examples/aslr_demo.c $(HEADERS) | build
	$(CC) $(PIC_CFLAGS) -c -o $@ $<

build/tls_demo.o: examples/tls_demo.c $(HEADERS) | build
	$(CC) $(CFLAGS) -ftls-model=local-exec -c -o $@ $<

$(DYNAMIC_DEMO): build/dynamic_demo.o build/crt0.pic.o $(SHARED_LIBC) $(DYNAMIC_LIBRARY)
	$(LD) -m elf_x86_64 -pie --hash-style=gnu -z now \
		-dynamic-linker /lib/ld-octagram.so -e _start \
		-o $@ build/crt0.pic.o build/dynamic_demo.o \
		-Lbuild -loctagram -ldyn_demo

$(COPY_DEMO): build/copy_reloc_demo.o build/crt0.o $(SHARED_LIBC) $(DYNAMIC_LIBRARY)
	$(LD) -m elf_x86_64 --hash-style=gnu -z now \
		-dynamic-linker /lib/ld-octagram.so -e _start \
		-o $@ build/crt0.o build/copy_reloc_demo.o \
		-Lbuild -loctagram -ldyn_demo

$(VERSIONED_DEMO): build/versioned_demo.o build/crt0.o $(SHARED_LIBC) $(DYNAMIC_LIBRARY)
	$(LD) -m elf_x86_64 --hash-style=gnu -z now \
		-dynamic-linker /lib/ld-octagram.so -e _start \
		-o $@ build/crt0.o build/versioned_demo.o \
		-Lbuild -loctagram -ldyn_demo

$(DLOPEN_DEMO): build/dlopen_demo.o build/crt0.pic.o $(SHARED_LIBC) $(DYNAMIC_LIBRARY)
	$(LD) -m elf_x86_64 -pie --hash-style=gnu -z now \
		-dynamic-linker /lib/ld-octagram.so -e _start \
		-o $@ build/crt0.pic.o build/dlopen_demo.o \
		-Lbuild -loctagram

$(ASLR_DEMO): build/aslr_demo.o build/crt0.pic.o $(SHARED_LIBC) $(DYNAMIC_LIBRARY)
	$(LD) -m elf_x86_64 -pie --hash-style=gnu -z now \
		-dynamic-linker /lib/ld-octagram.so -e _start \
		-o $@ build/crt0.pic.o build/aslr_demo.o \
		-Lbuild -loctagram -ldyn_demo

build/shared_errno_demo.o: examples/shared_errno_demo.c $(HEADERS) | build
	$(CC) $(PIC_CFLAGS) -c -o $@ $<

$(SHARED_ERRNO_DEMO): build/shared_errno_demo.o build/crt0.pic.o $(SHARED_LIBC)
	$(LD) -m elf_x86_64 -pie --hash-style=gnu -z now \
		-dynamic-linker /lib/ld-octagram.so -e _start \
		-o $@ build/crt0.pic.o build/shared_errno_demo.o -Lbuild -loctagram

build/%.elf: build/%.o build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o build/math.o build/math_wide.o build/wctype.o build/wchar.o linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o build/math.o build/math_wide.o build/wctype.o build/wchar.o $<

build/scanf_runtime.o: runtime.c $(HEADERS) | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-fno-stack-protector -fno-pie -ffunction-sections -fdata-sections \
		-Iinclude -c -o $@ $<

build/scanf_runtime_test.o: tests/scanf_runtime_test.c $(HEADERS) | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-fno-stack-protector -fno-pie -ffunction-sections -fdata-sections \
		-Iinclude -c -o $@ $<

build/scanf_runtime_start.o: tests/scanf_runtime_start.S | build
	$(HOSTAS) --64 -o $@ $<

build/scanf_runtime_test: build/scanf_runtime_start.o build/scanf_runtime_test.o build/scanf_runtime.o
	$(HOSTLD) -m elf_x86_64 -static --gc-sections -e _start -o $@ $^

build/cxa_atexit_test.o: tests/cxa_atexit_test.c $(HEADERS) | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-fno-stack-protector -fno-pie -ffunction-sections -fdata-sections \
		-Iinclude -c -o $@ $<

build/cxa_atexit_test: build/scanf_runtime_start.o build/cxa_atexit_test.o build/scanf_runtime.o
	$(HOSTLD) -m elf_x86_64 -static --gc-sections -e _start -o $@ $^

build/resolv_conf_test.o: tests/resolv_conf_test.c $(HEADERS) | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-fno-stack-protector -fno-pie -ffunction-sections -fdata-sections \
		-Iinclude -c -o $@ $<

build/resolv_conf_test: build/scanf_runtime_start.o build/resolv_conf_test.o build/scanf_runtime.o
	$(HOSTLD) -m elf_x86_64 -static --gc-sections -e _start -o $@ $^

build/ipv6_test.o: tests/ipv6_test.c $(HEADERS) | build
	$(HOSTCC) -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
		-fno-stack-protector -fno-pie -ffunction-sections -fdata-sections \
		-Iinclude -c -o $@ $<

build/ipv6_test: build/scanf_runtime_start.o build/ipv6_test.o build/scanf_runtime.o
	$(HOSTLD) -m elf_x86_64 -static --gc-sections -e _start -o $@ $^

build/tcc_signals.o: tcc_signals.c | build
	$(CC) $(TCC_CFLAGS) -c -o $@ $<

$(TCC_ELF): $(TCC_OBJECTS) build/tcc_signals.o build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o build/wchar.o build/math_wide.o build/math.o $(TCC_SETJMP) linker.ld
	$(LD) $(LDFLAGS) --gc-sections -o $@ build/crt0.o build/runtime.o build/posix_extra.o build/compiler_rt.o build/wchar.o build/math_wide.o build/math.o $(TCC_SETJMP) $(TCC_OBJECTS) build/tcc_signals.o

$(TCC_LIBTCC1): $(TCC_SUPPORT_OBJECTS)
	$(AR) rcs $@ $^

build/math.o: math.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

$(TCC_LIBC): build/runtime.o build/posix_extra.o build/math.o build/math_wide.o build/wctype.o build/wchar.o build/compiler_rt.o $(TCC_SETJMP)
	$(AR) rcs $@ $^

clean:
	rm -rf build
	$(MAKE) -C rust clean
