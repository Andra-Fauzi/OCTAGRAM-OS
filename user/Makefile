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
	-mno-mmx -msse2 -mfpmath=387 \
	-Iinclude
LDFLAGS := -m elf_x86_64 -static -nostdlib -T linker.ld
PROGRAMS := hello file_demo runtime_demo tty_demo argv_demo graphics_demo spawn_demo pipe_demo env_demo posix_demo stdio_demo posix_spawn_demo scanf_demo
BINARIES := $(addprefix build/,$(addsuffix .elf,$(PROGRAMS)))
HEADERS := $(wildcard include/*.h include/sys/*.h)
CHIBICC_SOURCES := codegen hashmap main parse preprocess strings tokenize type unicode
CHIBICC_OBJECTS := $(addprefix build/chibicc_,$(addsuffix .o,$(CHIBICC_SOURCES)))
CHIBICC_HEADERS := $(wildcard third_party/chibicc/*.h)
CHIBICC_ELF := build/chibicc.elf
CHIBICC_CFLAGS := $(CFLAGS) -DOCTAGRAM -mfpmath=387 -ffunction-sections \
	-fdata-sections -Wno-switch -Wno-unused-function -Wno-unused-variable \
	-Wno-sign-compare -Wno-missing-field-initializers -Ithird_party/chibicc
TCC_SOURCES := tcc libtcc tccpp tccgen tccdbg tccelf tccasm tccrun \
	x86_64-gen x86_64-link i386-asm
TCC_OBJECTS := $(addprefix build/tcc_,$(addsuffix .o,$(TCC_SOURCES)))
TCC_HEADERS := $(wildcard third_party/tinycc/*.h third_party/tinycc/*.c)
TCC_ELF := build/tcc.elf
TCC_CFLAGS := $(CFLAGS) -w -DONE_SOURCE=0 -DCONFIG_TCC_STATIC \
	-DTARGETOS_OCTAGRAM -Ithird_party/tinycc -Ithird_party/tinycc/include
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

.PHONY: all clean rust chibicc scanf-test
all: $(BINARIES) $(CHIBICC_ELF) $(TCC_ELF) $(TCC_LIBTCC1) $(TCC_LIBC)

chibicc: $(CHIBICC_ELF)

tcc: $(TCC_ELF) $(TCC_LIBTCC1) $(TCC_LIBC)

scanf-test: build/scanf_runtime_test
	./build/scanf_runtime_test

rust:
	$(MAKE) -C rust

build:
	mkdir -p $@

build/crt0.o: crt0.S | build
	$(AS) --64 -o $@ $<

build/setjmp.o: setjmp.S | build
	$(AS) --64 -o $@ $<

build/runtime.o: runtime.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/chibicc_%.o: third_party/chibicc/%.c $(CHIBICC_HEADERS) $(HEADERS) | build
	$(CC) $(CHIBICC_CFLAGS) -c -o $@ $<

build/tcc_%.o: third_party/tinycc/%.c $(TCC_HEADERS) $(HEADERS) | build
	$(CC) $(TCC_CFLAGS) -c -o $@ $<

build/tcc_support_%.o: third_party/tinycc/lib/%.c $(HEADERS) | build
	$(CC) $(TCC_SUPPORT_CFLAGS) -c -o $@ $<

build/tcc_support_%.o: third_party/tinycc/lib/%.S | build
	$(CC) $(TCC_SUPPORT_CFLAGS) -c -o $@ $<

build/%.o: examples/%.c $(HEADERS) | build
	$(CC) $(CFLAGS) -c -o $@ $<

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

$(TCC_ELF): $(TCC_OBJECTS) build/crt0.o build/runtime.o $(TCC_SETJMP) linker.ld
	$(LD) $(LDFLAGS) --gc-sections -o $@ build/crt0.o build/runtime.o $(TCC_SETJMP) $(TCC_OBJECTS)

$(TCC_LIBTCC1): $(TCC_SUPPORT_OBJECTS)
	$(AR) rcs $@ $^

$(TCC_LIBC): build/runtime.o $(TCC_SETJMP)
	$(AR) rcs $@ $^

$(CHIBICC_ELF): $(CHIBICC_OBJECTS) build/crt0.o build/runtime.o linker.ld
	$(LD) $(LDFLAGS) --gc-sections -o $@ build/crt0.o build/runtime.o $(CHIBICC_OBJECTS)

clean:
	rm -rf build
	$(MAKE) -C rust clean
