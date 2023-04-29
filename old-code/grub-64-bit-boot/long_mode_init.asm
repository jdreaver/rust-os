global long_mode_start

extern multiboot_header

section .text
bits 64
long_mode_start:
        ; Store multiboot header location in rdi so it is passed as the first
        ; parameter to the kernel. (rdi stores the first arg to a function in
        ; the x86_64 ABI)
        xor rdi, rdi            ; Clear rax so we can store pointer in eax and not worry about the upper 32 bits
        mov edi, [multiboot_header]

        ; Call the kernel main function
        extern kmain
        call kmain

        ; print `OKAY` to screen
        mov rax, 0x2f592f412f4b2f4f
        mov qword [0xb8000], rax
        hlt
