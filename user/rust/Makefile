CARGO ?= cargo
AS ?= as
READELF ?= readelf

RUST_TARGET_JSON := targets/x86_64-unknown-octagram.json
RUST_TARGET := x86_64-unknown-octagram
RUST_HELLO := build/rust_hello.elf
RUST_MAKE := build/make.elf
RUST_OCTAGRAM_HELLO := build/rust_hello_octagram.elf
RUST_BUILD_FLAGS := +nightly -Z unstable-options -Z build-std=core,alloc

.PHONY: all clean make build-octagram check
all: $(RUST_HELLO) $(RUST_MAKE)

make: $(RUST_MAKE)

# All Rust userspace binaries are built for OCTAGRAM's own target. The host
# runs Cargo/rustc; the resulting ELF is loaded by OCTAGRAM's user ELF loader.
build-octagram: $(RUST_OCTAGRAM_HELLO)

build:
	mkdir -p $@

build/crt0.o: ../crt0.S | build
	$(AS) --64 -o $@ $<

$(RUST_HELLO): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		$(RUST_TARGET_JSON) linker.ld build/crt0.o hello/Cargo.toml \
		hello/src/main.rs runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) $(RUST_BUILD_FLAGS) build --offline --release --bin rust_hello \
		--target $(RUST_TARGET_JSON)
	cp target/$(RUST_TARGET)/release/rust_hello $@

$(RUST_MAKE): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		$(RUST_TARGET_JSON) linker.ld build/crt0.o make/Cargo.toml \
		make/src/main.rs runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) $(RUST_BUILD_FLAGS) build --offline --release --bin make \
		--target $(RUST_TARGET_JSON)
	cp target/$(RUST_TARGET)/release/make $@

$(RUST_OCTAGRAM_HELLO): $(RUST_HELLO)
	cp $< $@

# Verify that the produced program is a static x86-64 ELF executable.
check: all
	$(READELF) -h $(RUST_HELLO)
	$(READELF) -l $(RUST_HELLO)
	$(READELF) -h $(RUST_MAKE)
	$(READELF) -l $(RUST_MAKE)

clean:
	rm -rf build target
