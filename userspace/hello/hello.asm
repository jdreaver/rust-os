global _start

section .text

_start:
        int3                    ; Test interrupts

        mov rdi, 1              ; print(
        mov rsi, msg            ;  "Hello, world!\n",
        mov rdx, msglen         ;  sizeof("Hello, world!\n")
        syscall                 ; );

        mov rdi, 0             ; exit(
        mov rsi, 0              ;  EXIT_SUCCESS
        syscall                 ; );

section .rodata
msg: db "Hello, world!", 10
msglen: equ $ - msg
