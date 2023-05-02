use bitvec::prelude::AsBits;

use crate::font::{FONT_START_CHAR, FONT_WIDTH_PIXELS, OPENGL_FONT};
use crate::framebuffer::{VESAFramebuffer32Bit, ARGB32BIT_BLACK, ARGB32BIT_WHITE};

/// A cursor-based text buffer that can print text to a framebuffer.
pub struct TextBuffer<'a> {
    framebuffer: &'a mut VESAFramebuffer32Bit,
}

impl<'a> TextBuffer<'a> {
    /// Creates a new text buffer.
    pub fn new(framebuffer: &'a mut VESAFramebuffer32Bit) -> TextBuffer<'a> {
        TextBuffer {
            framebuffer,
        }
    }

    pub fn write_char(&mut self, x: usize, y: usize, byte: u8) {
        let index: usize = match byte.checked_sub(FONT_START_CHAR) {
            Some(index) => index as usize,
            None => return,
        };

        let char_bytes = match OPENGL_FONT.get(index) {
            Some(bytes) => bytes,
            None => return,
        };
        let bitmap = char_bytes.as_bits::<bitvec::order::Msb0>();

        self.framebuffer.draw_bitmap(
            x,
            y,
            bitmap,
            FONT_WIDTH_PIXELS,
            ARGB32BIT_WHITE,
            ARGB32BIT_BLACK,
        );
    }
}
