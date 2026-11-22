CROSS_COMPILE ?=
CC := $(CROSS_COMPILE)gcc
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
PROGRAMS := sh hello file_demo runtime_demo tty_demo argv_demo spawn_demo pipe_demo env_demo posix_demo stdio_demo posix_spawn_demo file_api_demo path_api_demo at_api_demo execve_demo fork_demo time_demo mmap_demo scanf_demo tls_demo thread_demo thread_tsd_demo fb_demo syscall_compat_demo account_demo socket_msg_demo netdb_demo dns_demo cxa_atexit_demo credential_demo robust_mutex_demo
BINARIES := $(addprefix build/,$(addsuffix .elf,$(PROGRAMS)))
HEADERS := $(wildcard include/*.h include/sys/*.h include/netinet/*.h)
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
TCC_SUPPORT_SOURCES := libtcc1 stdatomic builtin va_list dsohandle
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

.PHONY: all clean rust scanf-test cxa-test resolv-test ipv6-test stdint-test dynamic
all: $(BINARIES) $(TCC_ELF) $(TCC_LIBTCC1) $(TCC_LIBC) $(LDSO_ELF) $(DYNAMIC_DEMO) $(COPY_DEMO) $(VERSIONED_DEMO) $(DLOPEN_DEMO) $(ASLR_DEMO)

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

stdint-test:
	$(HOSTCC) -std=c11 -Wall -Wextra -Werror -Iinclude -fsyntax-only tests/stdint_header_test.c

rust:
	$(MAKE) -C rust

build:
	mkdir -p $@

build/crt0.o: crt0.S | build
	$(AS) --64 -o $@ $<

build/crt0.pic.o: crt0.S | build
	$(AS) --64 -o $@ $<

build/setjmp.o: setjmp.S | build
	$(AS) --64 -o $@ $<

build/runtime.o: runtime.c $(HEADERS) | build
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

build/%.elf: build/%.o build/crt0.o build/runtime.o linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o $<

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

$(TCC_ELF): $(TCC_OBJECTS) build/tcc_signals.o build/crt0.o build/runtime.o $(TCC_SETJMP) linker.ld
	$(LD) $(LDFLAGS) --gc-sections -o $@ build/crt0.o build/runtime.o $(TCC_SETJMP) $(TCC_OBJECTS) build/tcc_signals.o

$(TCC_LIBTCC1): $(TCC_SUPPORT_OBJECTS)
	$(AR) rcs $@ $^

$(TCC_LIBC): build/runtime.o $(TCC_SETJMP)
	$(AR) rcs $@ $^

clean:
	rm -rf build
	$(MAKE) -C rust clean
