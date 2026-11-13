CROSS_COMPILE ?=
CC := $(CROSS_COMPILE)gcc
AS := $(CROSS_COMPILE)as
LD := $(CROSS_COMPILE)ld

CFLAGS := -std=c11 -O2 -Wall -Wextra -Werror -ffreestanding -fno-builtin \
	-fno-stack-protector -fno-pie -fno-pic -mno-red-zone -mcmodel=small \
	-Iinclude
LDFLAGS := -m elf_x86_64 -static -nostdlib -T linker.ld
PROGRAMS := hello file_demo runtime_demo tty_demo argv_demo
BINARIES := $(addprefix build/,$(addsuffix .elf,$(PROGRAMS)))

.PHONY: all clean
all: $(BINARIES)

build:
	mkdir -p $@

build/crt0.o: crt0.S | build
	$(AS) --64 -o $@ $<

build/runtime.o: runtime.c include/octagram.h | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/%.o: examples/%.c include/octagram.h | build
	$(CC) $(CFLAGS) -c -o $@ $<

build/%.elf: build/%.o build/crt0.o build/runtime.o linker.ld
	$(LD) $(LDFLAGS) -o $@ build/crt0.o build/runtime.o $<

clean:
	rm -rf build
