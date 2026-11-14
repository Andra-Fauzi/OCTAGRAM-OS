CARGO ?= cargo
AS ?= as

RUST_HELLO := build/rust_hello.elf
RUST_GCC := build/gcc.elf
RUST_TARGET := x86_64-unknown-none

.PHONY: all clean gcc
all: $(RUST_HELLO) $(RUST_GCC)

gcc: $(RUST_GCC)

build:
	mkdir -p $@

build/crt0.o: ../crt0.S | build
	$(AS) --64 -o $@ $<

$(RUST_HELLO): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		linker.ld build/crt0.o hello/Cargo.toml hello/src/main.rs \
		runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) build --offline --release --bin rust_hello
	cp target/$(RUST_TARGET)/release/rust_hello $@

$(RUST_GCC): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		linker.ld build/crt0.o gcc/Cargo.toml gcc/src/main.rs gcc/src/fs.rs \
		runtime/Cargo.toml runtime/src/lib.rs ../../kernel/src/toolchain.rs
	$(CARGO) build --offline --release --bin gcc
	cp target/$(RUST_TARGET)/release/gcc $@

clean:
	rm -rf build target
