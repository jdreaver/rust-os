global start
extern long_mode_start

section .text
bits 32
start:
        mov esp, stack_top

        call check_multiboot
        call check_cpuid
        call check_long_mode

        call set_up_page_tables
        call enable_paging

        ; Load the 64-bit GDT
        lgdt [gdt64.pointer]

        ; Now we need to clear the pipeline of all 16-bit instructions, which we
        ; do with a far jump. The address doesn't actually need to be far away,
        ; but the type of jump needs to be specified as 'far'
        jmp gdt64.code_offset:long_mode_start

check_multiboot:
        cmp eax, 0x36d76289
        jne .no_multiboot

        ; Store multiboot header in dedicated memory location so we can retrieve
        ; it later when we jump to our kernel entrypoint.
        mov [multiboot_header], ebx

        ret
.no_multiboot:
        mov al, "0"
        jmp error

check_cpuid:
        ; Check if CPUID is supported by attempting to flip the ID bit (bit 21)
        ; in the FLAGS register. If we can flip it, CPUID is available.

        ; Copy FLAGS in to EAX via stack
        pushfd
        pop eax

        ; Copy to ECX as well for comparing later on
        mov ecx, eax

        ; Flip the ID bit
        xor eax, 1 << 21

        ; Copy EAX to FLAGS via the stack
        push eax
        popfd

        ; Copy FLAGS back to EAX (with the flipped bit if CPUID is supported)
        pushfd
        pop eax

        ; Restore FLAGS from the old version stored in ECX (i.e. flipping the
        ; ID bit back if it was ever flipped).
        push ecx
        popfd

        ; Compare EAX and ECX. If they are equal then that means the bit
        ; wasn't flipped, and CPUID isn't supported.
        cmp eax, ecx
        je .no_cpuid
        ret

.no_cpuid:
        mov al, "1"
        jmp error

check_long_mode:
        ; test if extended processor info in available
        mov eax, 0x80000000    ; implicit argument for cpuid
        cpuid                  ; get highest supported argument
        cmp eax, 0x80000001    ; it needs to be at least 0x80000001
        jb .no_long_mode       ; if it's less, the CPU is too old for long mode

        ; use extended info to test if long mode is available
        mov eax, 0x80000001    ; argument for extended processor info
        cpuid                  ; returns various feature bits in ecx and edx
        test edx, 1 << 29      ; test if the LM-bit is set in the D-register
        jz .no_long_mode       ; If it's not set, there is no long mode
        ret
.no_long_mode:
        mov al, "2"
        jmp error

set_up_page_tables:
        ; Set up the first entry of each table
        ;
        ; Note that page tables MUST be page aligned. This means the lower 12
        ; bits of the physical address (3 hex digits) MUST be 0. Then, each page
        ; table entry can use the lower 12 bits as flags for that entry.
        ;
        ; You may notice that we're setting our flags to "0b11", because we care only
        ; about bits 0 and 1. Bit 0 is the "exists" bit, and is only set if the entry
        ; corresponds to another page table (for the PML4T, PDPT, and PDT) or a page of
        ; physical memory (in the PT). Obviously we want to set this. Bit 1 is the
        ; "read/write" bit, which allows us to view and modify the given entry. Since we
        ; want our OS to have full control, we'll set this as well.

        ; Map first P4 entry to P3 table
        mov eax, p3_table
        or eax, 0b11 ; present + writable
        mov [p4_table], eax

        ; Map first P3 entry to P2 table
        mov eax, p2_table
        or eax, 0b11 ; present + writable
        mov [p3_table], eax

        ; Map each P2 entry to a huge 2MiB page
        mov ecx, 0         ; counter variable

.map_p2_table:
        .present_bit: equ 1 << 0   ; Page is present (it exists)
        .writeable_bit: equ 1 << 1 ; Allows writes
        .page_size_bit: equ 1 << 7 ; Page size is 2MiB (instead of 4KiB)

        ; Map ecx-th P2 entry to a huge page that starts at address 2MiB*ecx
        mov eax, 0x200000  ; 2MiB
        mul ecx            ; start address of ecx-th page
        or eax, .present_bit | .writeable_bit | .page_size_bit
        mov [p2_table + ecx * 8], eax ; map ecx-th entry

        inc ecx            ; increase counter
        cmp ecx, 512       ; if counter == 512, the whole P2 table is mapped
        jne .map_p2_table  ; else map the next entry

        ret

enable_paging:
        ; Load P4 to cr3 register (cpu uses this to access the P4 table)
        mov eax, p4_table
        mov cr3, eax

        ; Enable PAE-flag in cr4 (Physical Address Extension)
        mov eax, cr4
        or eax, 1 << 5
        mov cr4, eax

        ; Set the long mode bit in the EFER MSR (model specific register)
        mov ecx, 0xC0000080
        rdmsr
        or eax, 1 << 8
        wrmsr

        ; Enable paging in the cr0 register
        mov eax, cr0
        or eax, 1 << 31
        mov cr0, eax

        ret

; Prints `ERR: ` and the given error code to screen and hangs.
; parameter: error code (in ascii) in al
error:
    mov dword [0xb8000], 0x4f524f45
    mov dword [0xb8004], 0x4f3a4f52
    mov dword [0xb8008], 0x4f204f20
    mov byte  [0xb800a], al
    hlt

section .bss

; Page table data entries
;
; Note that we are using 2 MiB pages, which means that each entry in the P2
; table will map 2 MiB of memory. This means that we only need 512 entries in
; the P2 table to map the entire 1 GiB of memory that we have. Each entry is 8
; bytes, so the entire table is 4096 bytes (4 KiB). Also, it means we don't need
; to map to a p1 table; if a p2 entry has the "page size" bit set, then the
; entry points directly to a physical 2 MiB page, not to a p1 entry.
;
; Reference: Intel® 64 and IA-32 Architectures Software Developer Manuals
; (https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)
; Chapter 4.5.4: Linear Address Translation with 4-Level Paging and 5-Level
; Paging, specifically Figure 4-9: Linear-Address Translation to a 2-MByte Page
; using 4-Level Paging

align 4096 ; Page tables must be aligned to 4 KiB

p4_table:
    resb 4096
p3_table:
    resb 4096
p2_table:
    resb 4096

; Save some room for the stack.
; TODO: Designate this in a better spot. Should GRUB2 give us stack space?
stack_bottom:
    resb 4096 * 16
stack_top:

section .data

; Store the multiboot header here so we can access it when we jump to 64 bit
; mode.
global multiboot_header
multiboot_header:
        dw 0x00000000

section .rodata

; Set up Long Mode GDT. See https://wiki.osdev.org/Global_Descriptor_Table
gdt64:
.read_write_bit: equ 1 << 41    ; Allow writing
.executable_bit: equ 1 << 43    ; Allow execution
.descriptor_bit: equ 1 << 44    ; Code/data segment, not system segment
.present_bit:    equ 1 << 47    ; Segment is present
.long_mode_bit:  equ 1 << 53    ; 64 bit mode
.null_bits:
        ; Define the null sector for the 64 bit gdt, which is 8 bytes of nulls. Null
        ; sector is required for memory integrity check.
        dq 0x0000000000000000

.code_offset: equ $ - gdt64     ; Store offset into GDT, not absolute address
        dq .read_write_bit | .executable_bit | .descriptor_bit | .present_bit | .long_mode_bit
global .data_offset
.data_offset: equ $ - gdt64     ; Store offset into GDT, not absolute address
        dq .read_write_bit | .descriptor_bit | .present_bit | .long_mode_bit

; This pointer gives the length and address of the GDT. We will feed this
; structure to the CPU in order to set the protected mode GDT.
.pointer:
    dw $ - gdt64 - 1
    dq gdt64
