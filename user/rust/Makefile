CARGO ?= cargo
AS ?= as

RUST_HELLO := build/rust_hello.elf
RUST_MAKE := build/make.elf
RUST_TARGET := x86_64-unknown-none

.PHONY: all clean make
all: $(RUST_HELLO) $(RUST_MAKE)

make: $(RUST_MAKE)

build:
	mkdir -p $@

build/crt0.o: ../crt0.S | build
	$(AS) --64 -o $@ $<

$(RUST_HELLO): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		linker.ld build/crt0.o hello/Cargo.toml hello/src/main.rs \
		runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) build --offline --release --bin rust_hello
	cp target/$(RUST_TARGET)/release/rust_hello $@

$(RUST_MAKE): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		linker.ld build/crt0.o make/Cargo.toml make/src/main.rs \
		runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) build --offline --release --bin make
	cp target/$(RUST_TARGET)/release/make $@

clean:
	rm -rf build target
