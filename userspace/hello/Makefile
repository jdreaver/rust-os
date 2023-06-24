.PHONY: all
all: hello

hello: hello.o
	ld -o $@ $?

hello.o: hello.asm
	nasm -f elf64 -F dwarf -g $?

.PHONY: clean
clean:
	rm -f hello hello.o
