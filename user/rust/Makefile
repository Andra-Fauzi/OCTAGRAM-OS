CARGO ?= cargo
AS ?= as
READELF ?= readelf
SHELL ?= /bin/sh

RUST_TARGET_JSON := targets/x86_64-unknown-octagram.json
RUST_TARGET := x86_64-unknown-octagram
RUST_HELLO := build/rust_hello.elf
RUST_MAKE := build/make.elf
RUST_OCTAGRAM_HELLO := build/rust_hello_octagram.elf
RUST_NATIVE_OBJECT := build/rust_native_probe.o
RUST_BUILD_FLAGS := +nightly -Z unstable-options -Z build-std=core,alloc -Z json-target-spec

# The native-linker probe is emitted as a single relocatable object for the
# guest's own `ld`. The link step is skipped by pointing rustc at a no-op
# linker, so this object depends on no crt0, libc, or runtime archive.
RUST_OBJECT_FLAGS := -C linker=true -C relocation-model=static -C no-redzone=yes \
	-C target-feature=+sse,+sse2

.PHONY: all clean make build-octagram check stage0-inventory
all: $(RUST_HELLO) $(RUST_MAKE) $(RUST_NATIVE_OBJECT)

make: $(RUST_MAKE) $(RUST_NATIVE_OBJECT)

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
	$(CARGO) $(RUST_BUILD_FLAGS) build --release --bin rust_hello \
		--target $(RUST_TARGET_JSON)
	cp target/$(RUST_TARGET)/release/rust_hello $@

$(RUST_MAKE): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		$(RUST_TARGET_JSON) linker.ld build/crt0.o make/Cargo.toml \
		make/src/main.rs runtime/Cargo.toml runtime/src/lib.rs
	$(CARGO) $(RUST_BUILD_FLAGS) build --release --bin make \
		--target $(RUST_TARGET_JSON)
	cp target/$(RUST_TARGET)/release/make $@

$(RUST_OCTAGRAM_HELLO): $(RUST_HELLO)
	cp $< $@

$(RUST_NATIVE_OBJECT): Cargo.toml rust-toolchain.toml .cargo/config.toml \
		$(RUST_TARGET_JSON) native-probe/Cargo.toml native-probe/src/main.rs
	RUSTFLAGS="$(RUST_OBJECT_FLAGS)" $(CARGO) $(RUST_BUILD_FLAGS) rustc \
		-p octagram-native-probe --release --bin rust_native_probe \
		--target $(RUST_TARGET_JSON) -- --emit=obj
	cp "$$(ls -t target/$(RUST_TARGET)/release/build/octagram-native-probe/*/out/rust_native_probe.o | head -n 1)" $@

# Verify that the produced program is a static x86-64 ELF executable.
check: all
	$(READELF) -h $(RUST_NATIVE_OBJECT)
	$(READELF) -h $(RUST_HELLO)
	$(READELF) -l $(RUST_HELLO)
	$(READELF) -h $(RUST_MAKE)
	$(READELF) -l $(RUST_MAKE)

stage0-inventory:
	mkdir -p build
	python3 ../../tools/rust_stage0_inventory.py --output build/stage0-inventory.json

clean:
	rm -rf build target
