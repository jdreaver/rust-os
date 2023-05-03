KERNEL = kernel.elf
ISO = kernel.iso
LIMINE = $(shell nix build ./flake#limine --print-out-paths --no-link)

RUST_BUILD_MODE = debug
RUST_BUILD_MODE_FLAG =
ifeq ($(RUST_BUILD_MODE),release)
  RUST_BUILD_MODE_FLAG = --release
endif

# Not all crates support `cargo test`
TEST_CRATES = vesa_framebuffer ring_buffer
ALL_CRATES = $(TEST_CRATES) kernel

.DEFAULT_GOAL := all
.PHONY: all
all: $(ISO)

QEMU_ARGS += -cdrom $(ISO)
QEMU_ARGS += -display gtk,zoom-to-fit=on # Makes it so increasing screen size zooms in, useful for tiny fonts
QEMU_ARGS += -vga virtio # More modern, better performance than default -vga std
QEMU_ARGS += -M q35 # Use the q35 chipset
QEMU_ARGS += -m 2G # More memory
QEMU_ARGS += -serial stdio # Add serial output to terminal

.PHONY: run
run: $(ISO)
	qemu-system-x86_64 $(QEMU_ARGS)

# N.B. Run `make run-debug` in one terminal, and `make gdb` in another.
QEMU_DEBUG_ARGS += $(QEMU_ARGS)
QEMU_DEBUG_ARGS += -d int,cpu_reset,guest_errors # Log some unexpected things. Run qemu-system-x86_64 -d help to see more.
# QEMU_DEBUG_ARGS += -M q35,accel=tcg # Disable hardware acceleration which makes logging interrupts give more info.
.PHONY: run-debug
run-debug: $(ISO)
	qemu-system-x86_64 $(QEMU_DEBUG_ARGS) -s -S

.PHONY: gdb
gdb: # No deps because we don't want an accidental rebuild if `make debug` already ran.
	gdb $(KERNEL) -ex "target remote :1234"

.PHONY: kernel
kernel:
	cd kernel && cargo build $(RUST_BUILD_MODE_FLAG)
	cp kernel/target/x86_64-rust_os/$(RUST_BUILD_MODE)/rust-os $(KERNEL)

$(ISO): kernel
	rm -rf iso_root
	mkdir -p iso_root

	cp $(KERNEL) \
		limine.cfg $(LIMINE)/limine.sys $(LIMINE)/limine-cd.bin $(LIMINE)/limine-cd-efi.bin iso_root/

	xorriso -as mkisofs -b limine-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot limine-cd-efi.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o $@
	$(LIMINE)/limine-deploy $@
	rm -rf iso_root

.PHONY: test
test:
	for crate in $(TEST_CRATES); do \
		(cd $$crate && cargo test) \
	done

	for crate in $(ALL_CRATES); do \
		(cd $$crate && cargo clippy && cargo fmt --check) \
	done

.PHONY: clean
clean:
	rm -rf target iso_root *.iso *.elf
