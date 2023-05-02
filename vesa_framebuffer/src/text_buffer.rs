use core::fmt;

use bitvec::prelude::AsBits;
use ringbuffer::{ConstGenericRingBuffer, RingBuffer, RingBufferExt, RingBufferWrite};

use crate::font::{FONT_HEIGHT_PIXELS, FONT_START_CHAR, FONT_WIDTH_PIXELS, OPENGL_FONT};
use crate::framebuffer::{ARGB32Bit, VESAFramebuffer32Bit, ARGB32BIT_BLACK, ARGB32BIT_WHITE};

/// ASCII character along with a color.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ColorChar {
    char_byte: u8,
    color: ARGB32Bit,
}

impl ColorChar {
    pub fn new(char_byte: u8, color: ARGB32Bit) -> Self {
        ColorChar { char_byte, color }
    }

    pub fn white_char(char_byte: u8) -> Self {
        ColorChar::new(char_byte, ARGB32BIT_WHITE)
    }
}

/// A cursor-based text buffer that can print text to a framebuffer.
pub struct TextBuffer<const N: usize = 50, const W: usize = 100> {
    /// Ring buffer that holds the text lines.
    buffer: ConstGenericRingBuffer<[ColorChar; W], N>,

    /// Cursor into the current line of text.
    cursor: usize,
}

impl<const N: usize, const W: usize> TextBuffer<N, W> {
    pub const fn new() -> Self {
        Self {
            buffer: ConstGenericRingBuffer::new(),
            cursor: 0,
        }
    }

    fn new_line(&mut self) {
        self.buffer.push([ColorChar::new(0x00, ARGB32BIT_WHITE); W]);
        self.cursor = 0;
    }

    /// Writes a character to the internal `TextLineBuffer`, but doesn't flush
    /// the text to the framebuffer. You must call `flush` to draw the text to
    /// the framebuffer.
    pub fn write_char(&mut self, c: ColorChar) {
        // Wrap text for newline and consume char
        if c.char_byte == b'\n' {
            self.new_line();
            return
        }

        // Wrap text to next line but don't consume char
        if self.cursor == W {
            self.new_line();
        }

        // Get the current line, ensuring one exists.
        let current_line = match self.buffer.back_mut() {
            Some(line) => line,
            None => {
                self.new_line();
                self.buffer.back_mut().unwrap()
            }
        };

        current_line[self.cursor] = c;
        self.cursor += 1;
    }

    /// Clear the framebuffer and then draw all the text that fits in the
    /// framebuffer.
    pub fn flush(&mut self, framebuffer: &mut VESAFramebuffer32Bit) {
        framebuffer.clear();

        // Start at the last line of the text buffer and draw lines until we run
        // out of space in the framebuffer or we run out of lines in the text
        // buffer.
        let mut pixel_y: usize = framebuffer.height_pixels();
        let mut lines_from_bottom: isize = 0;

        loop {
            if lines_from_bottom == self.buffer.len() as isize {
                break;
            }

            let buffer_index = -(lines_from_bottom + 1);
            let line = match self.buffer.get(buffer_index) {
                Some(line) => line,
                None => break,
            };
            lines_from_bottom += 1;

            // N.B. We copy the line here because we can't iterate over the line
            // while we are mutating ourselves due to the borrow checker. If we
            // want to get rid of this copy, we can create a new function that
            // takes the framebuffer and the text buffer as separate arguments.
            let line = *line;

            // Find y coordinate for line. The +1 is for spacing between lines
            pixel_y = match pixel_y.checked_sub(FONT_HEIGHT_PIXELS + 1) {
                Some(y) => y,
                None => break,
            };

            let mut x = 1; // A bit of space from left edge of screen
            for c in line.iter() {
                if x + FONT_WIDTH_PIXELS > framebuffer.width_pixels() {
                    break;
                }
                draw_char(framebuffer, x, pixel_y, *c);
                x += FONT_WIDTH_PIXELS + 1; // +1 for padding between characters
            }
        }
    }
}

impl<const N: usize, const W: usize> fmt::Write for TextBuffer<N, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
       for byte in s.bytes() {
           self.write_char(ColorChar {
               char_byte: byte,
               color: ARGB32BIT_WHITE,
           })
        }
        Ok(())
    }
}

fn draw_char(framebuffer: &mut VESAFramebuffer32Bit, x: usize, y: usize, c: ColorChar) {
    let index: usize = match c.char_byte.checked_sub(FONT_START_CHAR) {
        Some(index) => index as usize,
        None => 0, // Index of space character
    };

    let char_bytes = match OPENGL_FONT.get(index) {
        Some(bytes) => bytes,
        None => return,
    };
    let bitmap = char_bytes.as_bits::<bitvec::order::Msb0>();

    framebuffer.draw_bitmap(x, y, bitmap, FONT_WIDTH_PIXELS, c.color, ARGB32BIT_BLACK);
}
