.PHONY: all
all: primes

primes: runtime.o primes.o
	ld -static -z noexecstack -o $@ $?

primes.o: primes.c
	gcc -nostdlib -g -O0 -fno-stack-protector -o $@ -c $<

runtime: runtime.o
	ld -o $@ $?

runtime.o: runtime.asm
	nasm -f elf64 -F dwarf -g $?

.PHONY: clean
clean:
	rm -f primes *.o
