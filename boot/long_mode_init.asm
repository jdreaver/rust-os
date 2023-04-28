global long_mode_start

extern gdt64.data_offset

section .text
bits 64
long_mode_start:
    ; Point all data segment registers to the GDT data segment
    mov ax, gdt64.data_offset
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    ; call the rust main
    extern rust_main
    call rust_main

    ; print `OKAY` to screen
    mov rax, 0x2f592f412f4b2f4f
    mov qword [0xb8000], rax
    hlt
