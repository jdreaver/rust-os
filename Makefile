KERNEL = kernel.elf
HDD = kernel.hdd
LIMINE = $(shell nix build ./flake#limine --print-out-paths --no-link)
OVMF = $(shell nix build ./flake#OVMF --print-out-paths --no-link)/OVMF.fd

RUST_BUILD_MODE = debug
RUST_BUILD_MODE_FLAG =
ifeq ($(RUST_BUILD_MODE),release)
  RUST_BUILD_MODE_FLAG = --release
endif

# Not all crates support `cargo test`
TEST_CRATES += crates/fat
TEST_CRATES += crates/ring_buffer
TEST_CRATES += crates/vesa_framebuffer
ALL_CRATES = $(TEST_CRATES) kernel

.DEFAULT_GOAL := all
.PHONY: all
all: $(HDD)

# Good reference for QEMU options: https://wiki.gentoo.org/wiki/QEMU/Options
UEFI = on
ifeq ($(UEFI),on)
  $(info UEFI is enabled)
  QEMU_ARGS += -bios $(OVMF)
else
  $(info UEFI is disabled)
endif
# Use virtio for the disk:
QEMU_ARGS += -drive file=$(HDD),if=none,id=drive-virtio-disk0,format=raw -device virtio-blk-pci,scsi=off,drive=drive-virtio-disk0,id=virtio-disk0,bootindex=0
QEMU_ARGS += -smp 2 # Use 2 cores
QEMU_ARGS += -display gtk,zoom-to-fit=on # Makes it so increasing screen size zooms in, useful for tiny fonts
QEMU_ARGS += -vga virtio # More modern, better performance than default -vga std
QEMU_ARGS += -M q35,accel=kvm # Use the q35 chipset. accel=kvm enables hardware acceleration, makes things way faster.
QEMU_ARGS += -m 2G # More memory
QEMU_ARGS += -serial stdio # Add serial output to terminal
QEMU_ARGS += -device virtio-rng-pci-non-transitional # RNG is the simplest virtio device. Good for testing.

.PHONY: run
run: $(HDD)
	qemu-system-x86_64 $(QEMU_ARGS)

# N.B. Run `make run-debug` in one terminal, and `make gdb` in another.
QEMU_DEBUG_ARGS += $(QEMU_ARGS)
QEMU_DEBUG_ARGS += -d int,cpu_reset,guest_errors # Log some unexpected things. Run qemu-system-x86_64 -d help to see more.
# QEMU_DEBUG_ARGS += -M q35,accel=tcg # Disable hardware acceleration which makes logging interrupts give more info.
.PHONY: run-debug
run-debug: $(HDD)
	qemu-system-x86_64 $(QEMU_DEBUG_ARGS) -s -S

.PHONY: gdb
gdb: # No deps because we don't want an accidental rebuild if `make debug` already ran.
	gdb $(KERNEL) -ex "target remote :1234"

.PHONY: kernel
kernel:
	cd kernel && cargo build $(RUST_BUILD_MODE_FLAG)
	cp kernel/target/x86_64-rust_os/$(RUST_BUILD_MODE)/rust-os $(KERNEL)

# Old ISO build. Run in QEMU with: -cdrom $(ISO)
# ISO = kernel.iso
# $(ISO): kernel
# 	rm -rf iso_root
# 	mkdir -p iso_root
#
# 	cp $(KERNEL) \
# 		limine.cfg $(LIMINE)/limine.sys $(LIMINE)/limine-cd.bin $(LIMINE)/limine-cd-efi.bin iso_root/
#
# 	xorriso -as mkisofs -b limine-cd.bin \
# 		-no-emul-boot -boot-load-size 4 -boot-info-table \
# 		--efi-boot limine-cd-efi.bin \
# 		-efi-boot-part --efi-boot-image --protective-msdos-label \
# 		iso_root -o $@
#
# 	$(LIMINE)/limine-deploy $@
# 	rm -rf iso_root

# Adapted from https://github.com/limine-bootloader/limine-barebones/blob/trunk/GNUmakefile
$(HDD): kernel
	rm -f $(HDD)
	dd if=/dev/zero bs=1M count=0 seek=64 of=$(HDD)
	parted -s $(HDD) mklabel gpt
	parted -s $(HDD) mkpart ESP fat32 2048s 100%
	parted -s $(HDD) set 1 esp on
	$(LIMINE)/limine-deploy $(HDD)
	sudo losetup -Pf --show $(HDD) >loopback_dev
	sudo mkfs.fat -F 32 `cat loopback_dev`p1
	mkdir -p img_mount
	sudo mount `cat loopback_dev`p1 img_mount
	sudo mkdir -p img_mount/EFI/BOOT
	sudo cp -v $(KERNEL) limine.cfg $(LIMINE)/limine.sys img_mount/
	sudo cp -v $(LIMINE)/BOOTX64.EFI img_mount/EFI/BOOT/
	sync img_mount
	sudo umount img_mount
	sudo losetup -d `cat loopback_dev`
	rm -rf loopback_dev img_mount

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
	rm -rf target img_mount iso_root *.iso *.elf
