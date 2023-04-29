ISO = kernel.iso

.DEFAULT_GOAL := all
.PHONY: all
all: $(ISO)

QEMU_ARGS += -cdrom $(ISO)
QEMU_ARGS += -M q35 # Use the q35 chipset
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
	cargo build
	cp target/x86_64-rust_os/debug/rust-os kernel.elf

.PHONY: limine
limine:
	cd limine && git submodule update --remote --merge && make

$(ISO): limine kernel
	rm -rf iso_root
	mkdir -p iso_root

	cp kernel.elf \
		limine.cfg limine/limine.sys limine/limine-cd.bin limine/limine-cd-efi.bin iso_root/

	xorriso -as mkisofs -b limine-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot limine-cd-efi.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o $@
	limine/limine-deploy $@
	rm -rf iso_root

.PHONY: clean
clean:
	rm -rf target iso_root *.iso *.elf
