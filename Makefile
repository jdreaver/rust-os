ISO = build/kernel.iso
KERNEL = build/kernel.bin
RUST_OS = target/x86_64-rust_os/debug/librust_os.a
GRUB_CFG = boot/grub.cfg

.DEFAULT_GOAL := all
.PHONY: all
all: $(ISO)

QEMU_ARGS += -cdrom $(ISO)
QEMU_ARGS += -serial stdio # Add serial output to terminal
QEMU_ARGS += -d int,cpu_reset,guest_errors # Log some unexpected things. Run qemu-system-x86_64 -d help to see more.

.PHONY: run
run: $(ISO)
	qemu-system-x86_64 $(QEMU_ARGS)


# N.B. Run `make debug` in one terminal, and `make debug-gdb` in another.
.PHONY: debug
debug: $(ISO)
	qemu-system-x86_64 $(QEMU_ARGS) -s -S

.PHONY: debug-gdb
debug-gdb: # No deps because we don't want an accidental rebuild if `make debug` already ran.
	gdb $(KERNEL) -ex "target remote :1234"

$(KERNEL): build/multiboot_header.o build/boot.o build/long_mode_init.o boot/linker.ld kernel
	ld -n -o $@ -T boot/linker.ld build/multiboot_header.o build/boot.o build/long_mode_init.o $(RUST_OS)

build/multiboot_header.o: boot/multiboot_header.asm
	mkdir -p build
	nasm -f elf64 -o $@ $<

build/boot.o: boot/boot.asm
	mkdir -p build
	nasm -f elf64 -o $@ $<

build/long_mode_init.o: boot/long_mode_init.asm
	mkdir -p build
	nasm -f elf64 -o $@ $<

.PHONY: kernel
kernel:
	cargo build

$(ISO): $(KERNEL) $(GRUB_CFG)
	mkdir -p build/isofiles/boot/grub
	cp $(KERNEL) build/isofiles/boot/kernel.bin
	cp $(GRUB_CFG) build/isofiles/boot/grub
	grub-mkrescue -o $(ISO) build/isofiles
	rm -r build/isofiles

.PHONY: clean
clean:
	rm -rf target build boot/*.o
