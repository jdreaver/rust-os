use bitvec::prelude::AsBits;
use ringbuffer::{ConstGenericRingBuffer, RingBufferExt, RingBufferWrite};

use crate::font::{FONT_HEIGHT_PIXELS, FONT_START_CHAR, FONT_WIDTH_PIXELS, OPENGL_FONT};
use crate::framebuffer::{ARGB32Bit, VESAFramebuffer32Bit, ARGB32BIT_BLACK, ARGB32BIT_WHITE};

/// ASCII character along with a color.
#[derive(Debug, Copy, Clone)]
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
pub struct TextBuffer<'a> {
    framebuffer: &'a mut VESAFramebuffer32Bit,
    text_buffer: TextLineBuffer,
}

impl<'a> TextBuffer<'a> {
    /// Creates a new text buffer.
    pub fn new(framebuffer: &'a mut VESAFramebuffer32Bit) -> TextBuffer<'a> {
        TextBuffer {
            framebuffer,
            text_buffer: TextLineBuffer::new(),
        }
    }

    /// Writes a character to the internal `TextLineBuffer`, but doesn't flush
    /// the text to the framebuffer. You must call `flush` to draw the text to
    /// the framebuffer.
    pub fn write_char(&mut self, c: ColorChar) {
        self.text_buffer.write_char(c);
    }

    /// Clear the framebuffer and then draw all the text that fits in the
    /// framebuffer.
    pub fn flush(&mut self) {
        flush(self.framebuffer, &mut self.text_buffer);
    }
}

// This function only exists to make the borrow checker happy.
fn flush(framebuffer: &mut VESAFramebuffer32Bit, text_buffer: &mut TextLineBuffer) {
    framebuffer.clear();

    // Start at the last line of the text buffer and draw lines until we run
    // out of space in the framebuffer or we run out of lines in the text
    // buffer.
    let mut pixel_y: usize = framebuffer.height_pixels();
    let mut lines_from_bottom: isize = 0;

    while let Some(line) = text_buffer.buffer.get(lines_from_bottom) {
        // N.B. We copy the line here because we can't iterate over the line
        // while we are mutating ourselves due to the borrow checker. If we
        // want to get rid of this copy, we can create a new function that
        // takes the framebuffer and the text buffer as separate arguments.
        let line = *line;
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

fn draw_char(framebuffer: &mut VESAFramebuffer32Bit, x: usize, y: usize, c: ColorChar) {
    let index: usize = match c.char_byte.checked_sub(FONT_START_CHAR) {
        Some(index) => index as usize,
        None => return,
    };

    let char_bytes = match OPENGL_FONT.get(index) {
        Some(bytes) => bytes,
        None => return,
    };
    let bitmap = char_bytes.as_bits::<bitvec::order::Msb0>();

    framebuffer.draw_bitmap(x, y, bitmap, FONT_WIDTH_PIXELS, c.color, ARGB32BIT_BLACK);
}

/// Buffer that holds `N` lines of colored text that are `W` characters wide.
struct TextLineBuffer<const N: usize = 50, const W: usize = 100> {
    /// Ring buffer that holds the text lines.
    buffer: ConstGenericRingBuffer<[ColorChar; W], N>,

    /// Cursor into the current line of text.
    cursor: usize,
}

impl<const N: usize, const W: usize> TextLineBuffer<N, W> {
    fn new() -> Self {
        let mut buffer = Self {
            buffer: ConstGenericRingBuffer::new(),
            cursor: 0,
        };
        buffer.new_line();
        buffer
    }

    fn new_line(&mut self) {
        self.buffer.push([ColorChar::new(0x00, ARGB32BIT_WHITE); W]);
        self.cursor = 0;
    }

    fn write_char(&mut self, c: ColorChar) {
        if self.cursor == W || c.char_byte == b'\n' {
            self.new_line();
        }

        let current_line = self
            .buffer
            .back_mut()
            .expect("TextLineBuffer invariant failed: must always have a current line");

        current_line[self.cursor] = c;
        self.cursor += 1;
    }
}
