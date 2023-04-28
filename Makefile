ISO = build/kernel.iso
KERNEL = build/kernel.bin
GRUB_CFG = boot/grub.cfg

.DEFAULT_GOAL := all
.PHONY: all
all: $(ISO)

.PHONY: run
run: $(ISO)
	qemu-system-x86_64 -cdrom $(ISO)

$(KERNEL): build/multiboot_header.o build/boot.o boot/linker.ld
	ld -n -o $@ -T boot/linker.ld build/multiboot_header.o build/boot.o

build/multiboot_header.o: boot/multiboot_header.asm
	mkdir -p build
	nasm -f elf64 -o $@ $<

build/boot.o: boot/boot.asm
	mkdir -p build
	nasm -f elf64 -o $@ $<

$(ISO): $(KERNEL) $(GRUB_CFG)
	mkdir -p build/isofiles/boot/grub
	cp $(KERNEL) build/isofiles/boot/kernel.bin
	cp $(GRUB_CFG) build/isofiles/boot/grub
	grub-mkrescue -o $(ISO) build/isofiles
	rm -r build/isofiles

.PHONY: clean
clean:
	rm -rf target build boot/*.o
