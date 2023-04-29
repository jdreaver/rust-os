global long_mode_start

extern multiboot_header

section .text
bits 64
long_mode_start:
        ; Point all data segment registers to the null GDT segment. In 64 bit
        ; mode you don't need an actual data segment, null is okay. Many
        ; instructions, including iretq (returning from exception handlers)
        ; require a data segment descriptor _or_ the null descriptor.
        ;
        ; I used to have an actual data segment as the second entry in the GDT
        ; (or third if you count then null segment), but when I created a new
        ; GDT with the TSS as the second non-null segment, all of these
        ; registers were pointing at the TSS. This caused a general protection
        ; fault when I tried to return from an exception handler (including when
        ; returning from the general protection fault handler itself, causing an
        ; infinite loop of exception handling).
        mov ax, 0
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

        ; Call the kernel main function
        extern kmain
        call kmain

        ; print `OKAY` to screen
        mov rax, 0x2f592f412f4b2f4f
        mov qword [0xb8000], rax
        hlt
