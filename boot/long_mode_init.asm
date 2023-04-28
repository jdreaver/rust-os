global long_mode_start

extern gdt64.data_offset
extern multiboot_header

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

        ; Store multiboot header location in rdi so it is passed as the first
        ; parameter to the kernel. (rdi stores the first arg to a function in
        ; the x86_64 ABI)
        xor rdi, rdi            ; Clear rax so we can store pointer in eax and not worry about the upper 32 bits
        mov edi, [multiboot_header]

        ; call the rust main
        extern rust_main
        call rust_main

        ; print `OKAY` to screen
        mov rax, 0x2f592f412f4b2f4f
        mov qword [0xb8000], rax
        hlt