#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod vga_buffer;

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    // ATTENTION: we have a very small stack and no guard page

    println!("Hello World{}", "!");
    println!("Hello again! Some numbers: {} {}", 42, (1.7 * 3.3) as u64);
    // TODO: This print causes some sort of error that makes the OS crash. Also,
    // this print by itself doesn't trigger it. If it is the first print we are
    // fine. I suspect some stack tomfoolery. This only started failing after
    // switching to GRUB2, but was fine on the `bootloader` crate.
    // println!("{}", 1.337);
    panic!("Some panic message");

    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}
