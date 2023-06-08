KERNEL_HDD = kernel.hdd
LIMINE = $(shell nix build ./flake#limine --print-out-paths --no-link)
OVMF = $(shell nix build ./flake#OVMF --print-out-paths --no-link)/OVMF.fd
QEMU_DEBUG_BIN = $(shell nix build ./flake#qemu-x86_64-debug --print-out-paths --no-link)/bin/qemu-system-x86_64
QEMU_SOURCE_CODE = $(shell nix build ./flake#qemu-x86_64-debug --print-out-paths --no-link)/raw

TEST_FAT_HDD = test_fat.hdd
TEST_EXT2_HDD = test_ext2.hdd

RUST_BUILD_MODE = debug
RUST_BUILD_MODE_FLAG =
ifeq ($(RUST_BUILD_MODE),release)
  RUST_BUILD_MODE_FLAG = --release
endif

KERNEL = kernel/target/x86_64-rust_os/$(RUST_BUILD_MODE)/rust-os

# Not all crates support `cargo test`
TEST_CRATES += crates/bitmap-alloc
TEST_CRATES += crates/fat
TEST_CRATES += crates/ring_buffer
TEST_CRATES += crates/vesa_framebuffer
ALL_CRATES = $(TEST_CRATES) kernel

.DEFAULT_GOAL := all
.PHONY: all
all: $(KERNEL_HDD)

QEMU=qemu-system-x86_64
RUN_QEMU_GDB=no
ifeq ($(RUN_QEMU_GDB),yes)
  QEMU=gdb --directory $(QEMU_SOURCE_CODE)/build --args $(QEMU_DEBUG_BIN)
else
  # GTK is a much nicer display than SDL, but to compile QEMU with debug symbols
  # in Nix, we had to disable the GTK wrappers.
  QEMU_COMMON_ARGS += -display gtk,zoom-to-fit=on
endif

# Good reference for QEMU options: https://wiki.gentoo.org/wiki/QEMU/Options
UEFI = on
ifeq ($(UEFI),on)
  $(info UEFI is enabled)
  QEMU_COMMON_ARGS += -bios $(OVMF)
else
  $(info UEFI is disabled)
endif

GRAPHICS=off
ifeq ($(GRAPHICS),on)
  $(info QEMU graphics are enabled)
  QEMU_COMMON_ARGS += -vga virtio # More modern, better performance than default -vga std
  QEMU_COMMON_ARGS += -serial stdio # Send serial output to terminal
else
  $(info QEMU graphics are disabled)
  QEMU_COMMON_ARGS += -nographic
  # N.B. -nographic implies -serial stdio
endif

# Use virtio for the disk:
QEMU_COMMON_ARGS += -drive file=$(KERNEL_HDD),if=none,id=drive-virtio-disk0,format=raw -device virtio-blk-pci,scsi=off,drive=drive-virtio-disk0,id=virtio-disk0,bootindex=0,serial=hello-blk
QEMU_COMMON_ARGS += -drive file=$(TEST_FAT_HDD),if=none,id=drive-virtio-disk1,format=raw -device virtio-blk-pci,scsi=off,drive=drive-virtio-disk1,id=virtio-disk1,serial=test-fat
QEMU_COMMON_ARGS += -drive file=$(TEST_EXT2_HDD),if=none,id=drive-virtio-disk2,format=raw -device virtio-blk-pci,scsi=off,drive=drive-virtio-disk2,id=virtio-disk2,serial=test-ext2
QEMU_COMMON_ARGS += -smp 2 # Use 2 cores
QEMU_COMMON_ARGS += -m 2G # More memory
QEMU_COMMON_ARGS += -device virtio-rng-pci-non-transitional # RNG is the simplest virtio device. Good for testing.

QEMU_ARGS += $(QEMU_COMMON_ARGS)
QEMU_ARGS += -M q35,accel=kvm # Use the q35 chipset. accel=kvm enables hardware acceleration, makes things way faster.

.PHONY: run
run: $(KERNEL_HDD) $(TEST_FAT_HDD) $(TEST_EXT2_HDD)
	$(QEMU) $(QEMU_ARGS)

# N.B. Run `make run-debug` in one terminal, and `make gdb` in another.
QEMU_DEBUG_ARGS += $(QEMU_COMMON_ARGS)
QEMU_DEBUG_ARGS += -M q35 # Use the q35 chipset, but don't use kvm acceleration for debug mode because it makes logging interrupts give less info.
QEMU_DEBUG_ARGS += -d int,cpu_reset,guest_errors # Log some unexpected things. Run qemu-system-x86_64 -d help to see more.

.PHONY: run-debug
run-debug: $(KERNEL_HDD) $(TEST_FAT_HDD) $(TEST_EXT2_HDD)
	qemu-system-x86_64 $(QEMU_DEBUG_ARGS) -s -S

.PHONY: gdb
gdb: # No deps because we don't want an accidental rebuild if `make debug` already ran.
	rust-gdb $(KERNEL) -ex "target remote :1234"

.PHONY: kernel
kernel:
	cd kernel && cargo build $(RUST_BUILD_MODE_FLAG)

# Adapted from https://github.com/limine-bootloader/limine-barebones/blob/trunk/GNUmakefile
$(KERNEL_HDD): kernel
	rm -f $(KERNEL_HDD)
	dd if=/dev/zero bs=1M count=0 seek=64 of=$(KERNEL_HDD)
	parted -s $(KERNEL_HDD) mklabel gpt
	parted -s $(KERNEL_HDD) mkpart ESP fat32 2048s 100%
	parted -s $(KERNEL_HDD) set 1 esp on
	$(LIMINE)/limine-deploy $(KERNEL_HDD)
	sudo losetup -Pf --show $(KERNEL_HDD) >loopback_dev
	sudo mkfs.fat -F 32 `cat loopback_dev`p1
	mkdir -p img_mount
	sudo mount `cat loopback_dev`p1 img_mount
	sudo mkdir -p img_mount/EFI/BOOT
	sudo cp -v $(KERNEL) img_mount/kernel.elf
	sudo cp -v limine.cfg $(LIMINE)/limine.sys img_mount/
	sudo cp -v $(LIMINE)/BOOTX64.EFI img_mount/EFI/BOOT/
	sync img_mount
	sudo umount img_mount
	sudo losetup -d `cat loopback_dev`
	rm -rf loopback_dev img_mount

$(TEST_FAT_HDD):
	./crates/fat/scripts/create-test-image.sh $(TEST_FAT_HDD)

$(TEST_EXT2_HDD):
	./scripts/create-test-ext2-image.sh $(TEST_EXT2_HDD)

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
	rm -rf target img_mount iso_root *.iso *.elf *.hdd
