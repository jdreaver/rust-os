mod font;
mod framebuffer;
mod text_buffer;

pub(crate) use framebuffer::*;
pub(crate) use text_buffer::*;

use core::fmt::Write;

use crate::boot_info;
use crate::sync::SpinLock;

static FRAMEBUFFER: SpinLock<Option<VESAFramebuffer32Bit>> = SpinLock::new(None);

static TEXT_BUFFER: SpinLock<TextBuffer> = SpinLock::new(TextBuffer::new());

pub(crate) fn init(boot_info_data: &boot_info::BootInfo) {
    FRAMEBUFFER.lock().replace(unsafe {
        VESAFramebuffer32Bit::from_limine_framebuffer(boot_info_data.framebuffer)
            .expect("failed to create VESAFramebuffer32Bit")
    });
}

pub(crate) fn write_text_buffer(text: &str) {
    let mut framebuffer_lock = FRAMEBUFFER.lock();
    let framebuffer = framebuffer_lock
        .as_mut()
        .expect("framebuffer not initialized");
    let mut text_buffer = TEXT_BUFFER.lock();
    text_buffer
        .write_str(text)
        .expect("failed to write to text buffer");
    text_buffer.flush(framebuffer);
}
