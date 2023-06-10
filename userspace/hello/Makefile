.PHONY: all
all: hello

hello: hello.o
	ld -o $@ $?

hello.o: hello.asm
	nasm -f elf64 -O2 $?

# Debugging symbols
# hello.o: hello.asm
# 	nasm -f elf64 -g -F dwarf $? -l hello4.lst

.PHONY: clean
clean:
	rm -f hello hello.o
