#![no_std]
#![feature(abi_x86_interrupt)]

mod gdt;
mod interrupts;
mod kmain;
mod serial;
mod vga_buffer;
