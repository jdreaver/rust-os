section .text

extern main

global _start:

;; See "3.4 Process Initialization" in the System V AMD64 ABI spec, and
;; https://lwn.net/Articles/631631/ for a an explanation of what the stack looks
;; like.
_start:
        ; Pop argc off the stack and into rdi (first argument to main)
        pop	rdi

        ; Get argv from stack and put into rsi
        mov	rsi, rsp

        ; Call main
        call	main

        ; Exit the program with the exit code from main (return value is in rax)
        mov	rdi, rax
        call	syscall_exit

global syscall_print

;; Calls the print syscall. String to be printed is in rdi, length of string is rsi.
syscall_print:
        ; Shift args into correct registers
        mov	rdx, rsi        ; length of string
        mov	rsi, rdi        ; string to print
        mov	rdi, 1          ; print syscall
        syscall
        ret

global syscall_exit

;; Exit code is in rdi
syscall_exit:
        ; Shift args into correct registers
        mov rsi, rdi            ; exit code
        mov rdi, 0              ; exit syscall
        syscall
