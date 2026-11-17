# Nuke built-in rules and variables.
MAKEFLAGS += -rR
.SUFFIXES:

# This is the name that our final executable will have.
# Change as needed.
override OUTPUT := kernel

# Convenience macro to reliably declare user overridable variables.
override USER_VARIABLE = $(if $(filter $(origin $(1)),default undefined),$(eval override $(1) := $(2)))

# Target architecture to build for. Default to x86_64.
$(call USER_VARIABLE,KARCH,x86_64)

ifeq ($(RUST_TARGET),)
    override RUST_TARGET := $(KARCH)-unknown-none
	ifeq ($(KARCH),riscv64)
    	override RUST_TARGET := riscv64gc-unknown-none-elf
	endif
endif

ifeq ($(RUST_PROFILE),)
    override RUST_PROFILE := dev
endif

override RUST_PROFILE_SUBDIR := $(RUST_PROFILE)
ifeq ($(RUST_PROFILE),dev)
    override RUST_PROFILE_SUBDIR := debug
endif

# Default target.
.PHONY: all
all:
	$(MAKE) -C ../user/rust make
	$(MAKE) -C ../user tcc
	$(MAKE) -C ../user build/file_api_demo.elf
	$(MAKE) -C ../user build/path_api_demo.elf
	$(MAKE) -C ../user build/at_api_demo.elf
	$(MAKE) -C ../user build/execve_demo.elf
	$(MAKE) -C ../user build/fork_demo.elf
	$(MAKE) -C ../user build/time_demo.elf
	$(MAKE) -C ../user build/syscall_compat_demo.elf
	$(MAKE) -C ../user build/mmap_demo.elf
	$(MAKE) -C ../user build/tls_demo.elf
	$(MAKE) -C ../user build/thread_demo.elf
	$(MAKE) -C ../user dynamic
	RUSTFLAGS="-C relocation-model=static" cargo build --target $(RUST_TARGET) --profile $(RUST_PROFILE)
	cp target/$(RUST_TARGET)/$(RUST_PROFILE_SUBDIR)/octagram-kernel $(OUTPUT)

# Remove object files and the final executable.
.PHONY: clean
clean:
	cargo clean
	rm -rf kernel

.PHONY: distclean
distclean: clean
