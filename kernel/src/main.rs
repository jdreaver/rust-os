#![no_std]
#![no_main]
#![feature(allocator_api)]

extern crate alloc;

use rust_os::{panic_handler, start};

#[no_mangle]
extern "C" fn _start() -> ! {
    start();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    panic_handler(info)
}
