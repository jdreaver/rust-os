ISO = kernel.iso

.DEFAULT_GOAL := all
.PHONY: all
all: $(ISO)

.PHONY: run
run: $(ISO)
	qemu-system-x86_64 -M q35 -m 2G -cdrom $(ISO) -boot d

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
