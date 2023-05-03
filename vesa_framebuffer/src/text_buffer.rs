use core::fmt;

use bitvec::prelude::AsBits;
use ring_buffer::RingBuffer;

use crate::font::{
    FONT_HEIGHT_PIXELS, FONT_SPACE_CHARACTER_INDEX, FONT_START_CHAR_ASCII_CODE, FONT_WIDTH_PIXELS,
    OPENGL_FONT,
};
use crate::framebuffer::{ARGB32Bit, VESAFramebuffer32Bit, ARGB32BIT_BLACK, ARGB32BIT_WHITE};

/// ASCII character along with a color.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ColorChar {
    char_byte: u8,
    color: ARGB32Bit,
}

impl ColorChar {
    pub fn new(char_byte: u8, color: ARGB32Bit) -> Self {
        Self { char_byte, color }
    }

    pub fn white_char(char_byte: u8) -> Self {
        Self::new(char_byte, ARGB32BIT_WHITE)
    }
}

/// A cursor-based text buffer that can print text to a framebuffer.
pub struct TextBuffer<const N: usize = 50, const W: usize = 100> {
    /// Ring buffer that holds the text lines.
    buffer: RingBuffer<[ColorChar; W], N>,

    /// Cursor into the current line of text.
    cursor: usize,
}

impl<const N: usize, const W: usize> TextBuffer<N, W> {
    pub const fn new() -> Self {
        Self {
            buffer: RingBuffer::new(),
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
            return;
        }

        // Wrap text to next line but don't consume char
        if self.cursor == W {
            self.new_line();
        }

        // Get the current line, ensuring one exists.
        let current_line = if let Some(line) = self.buffer.get_mut(0) {
            line
        } else {
            self.new_line();
            self.buffer.get_mut(0).unwrap()
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
        let mut lines_from_bottom: usize = 0;

        loop {
            if lines_from_bottom == self.buffer.len() {
                break;
            }

            let Some(line) = self.buffer.get_mut(lines_from_bottom) else { break };
            lines_from_bottom += 1;

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
            });
        }
        Ok(())
    }
}

fn draw_char(framebuffer: &mut VESAFramebuffer32Bit, x: usize, y: usize, c: ColorChar) {
    let index: usize = c
        .char_byte
        .checked_sub(FONT_START_CHAR_ASCII_CODE)
        .map_or(FONT_SPACE_CHARACTER_INDEX, |index| index as usize);

    let Some(char_bytes) = OPENGL_FONT.get(index) else { return };
    let bitmap = char_bytes.as_bits::<bitvec::order::Msb0>();

    framebuffer.draw_bitmap(x, y, bitmap, FONT_WIDTH_PIXELS, c.color, ARGB32BIT_BLACK);
}

#[cfg(test)]
mod test {
    use super::*;

    fn assert_line_text_equal(line: &[ColorChar], expected: &[u8]) {
        assert_eq!(line.len(), expected.len());

        let left_chars = line
            .iter()
            .map(|c| c.char_byte as char)
            .collect::<Vec<char>>();
        let right_chars = expected.iter().map(|c| *c as char).collect::<Vec<char>>();

        assert_eq!(left_chars, right_chars);
    }

    #[test]
    fn test_text_buffer_writer() {
        use core::fmt::Write;
        let mut text_buffer: TextBuffer<4, 4> = TextBuffer::new();
        writeln!(text_buffer, "abc").unwrap();
        writeln!(text_buffer, "1234").unwrap();

        assert_eq!(text_buffer.buffer.len(), 3);
        assert_line_text_equal(text_buffer.buffer.get_mut(0).unwrap(), &[0; 4]);
        assert_line_text_equal(text_buffer.buffer.get_mut(1).unwrap(), b"1234");
        assert_line_text_equal(text_buffer.buffer.get_mut(2).unwrap(), b"abc\0");
    }

    #[test]
    fn test_text_buffer_implicit_line_wrap() {
        use core::fmt::Write;
        let mut text_buffer: TextBuffer<5, 4> = TextBuffer::new();
        writeln!(text_buffer, "abc").unwrap();
        writeln!(text_buffer, "1234567").unwrap();

        assert_eq!(text_buffer.buffer.len(), 4);
        assert_line_text_equal(text_buffer.buffer.get_mut(0).unwrap(), &[0; 4]);
        assert_line_text_equal(text_buffer.buffer.get_mut(1).unwrap(), b"567\0");
        assert_line_text_equal(text_buffer.buffer.get_mut(2).unwrap(), b"1234");
        assert_line_text_equal(text_buffer.buffer.get_mut(3).unwrap(), b"abc\0");
    }
}
