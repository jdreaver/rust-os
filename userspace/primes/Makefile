.PHONY: all
all: primes

primes: syscalls.o primes.o
	ld -static -z noexecstack -o $@ $?

primes.o: primes.c
	gcc -nostdlib -g -O0 -fno-stack-protector -o $@ -c $<

syscalls: syscalls.o
	ld -o $@ $?

syscalls.o: syscalls.asm
	nasm -f elf64 -F dwarf -g $?

.PHONY: clean
clean:
	rm -f primes *.o
