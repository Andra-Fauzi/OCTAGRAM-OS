CARGO ?= cargo
AS ?= as

RUST_HELLO := build/rust_hello.elf
RUST_MAKE := build/make.elf
RUST_TARGET := x86_64-unknown-none
RUST_OCTAGRAM_TARGET := x86_64-unknown-octagram
RUST_OCTAGRAM_HELLO := build/rust_hello_octagram.elf

.PHONY: all clean make build-octagram
all: $(RUST_HELLO) $(RUST_MAKE)

make: $(RUST_MAKE)

# Cross-build an executable whose Rust target cfg identifies OCTAGRAM.
# It is still produced by host Cargo/rustc, not by a native OCTAGRAM compiler.
build-octagram: $(RUST_OCTAGRAM_HELLO)

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

$(RUST_OCTAGRAM_HELLO): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		targets/x86_64-unknown-octagram.json linker.ld build/crt0.o \
		hello/Cargo.toml hello/src/main.rs runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) +nightly -Z build-std=core,alloc -Z json-target-spec build \
		--release --bin rust_hello --target targets/x86_64-unknown-octagram.json
	cp target/$(RUST_OCTAGRAM_TARGET)/release/rust_hello $@

clean:
	rm -rf build target
