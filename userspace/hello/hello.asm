global _start

section .text

_start:
        int3                    ; Test interrupts

        mov rax, 1              ; write(
        mov rdi, 1              ;  STDOUT_FILENO,
        mov rsi, msg            ;  "Hello, world!\n",
        mov rdx, msglen         ;  sizeof("Hello, world!\n")
        syscall                 ; );

        mov rax, 0             ; exit(
        mov rdi, 0              ;  EXIT_SUCCESS
        syscall                 ; );

section .rodata
msg: db "Hello, world!", 10
msglen: equ $ - msg
