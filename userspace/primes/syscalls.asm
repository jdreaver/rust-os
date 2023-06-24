section .text

global syscall_print

;; Calls the print syscall. String to be printed is in rdi, length of string is rsi.
syscall_print:
        ; Shift args into correct registers
        mov	rdx, rsi        ; length of string
        mov	rsi, rdi        ; string to print
        mov	rdi, 1          ; print syscall
        syscall

global syscall_exit

;; Exit code is in rdi
syscall_exit:
        ; Shift args into correct registers
        mov rsi, rdi            ; exit code
        mov rdi, 0              ; exit syscall
        syscall
