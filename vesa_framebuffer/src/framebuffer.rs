use bitvec::prelude as bv;

/// A VESA-compatible framebuffer where pixels are drawn directly to a location
/// in memory. Practically speaking, this is a facade over the framebuffer
/// returned from the `limine` bootloader, since that is what our kernel uses.
/// Also, this assumes 32 bits per pixel with 8 bits per color in ARGB (alpha,
/// red, green, blue) order.
///
/// # Resources
///
/// - <https://wiki.osdev.org/Getting_VBE_Mode_Info>
/// - <https://wiki.osdev.org/User:Omarrx024/VESA_Tutorial>
/// - <https://wiki.osdev.org/Drawing_In_a_Linear_Framebuffer>
#[derive(Debug)]
pub struct VESAFramebuffer32Bit {
    /// Pointer to the start of the framebuffer.
    ///
    /// N.B. We store this as `u8` so we do per-byte pointer arithmetic, and we
    /// convert to a wider pointer when we do writes.
    address: *mut u8,

    width_pixels: usize,
    height_pixels: usize,

    /// Number of bytes per horizontal line. Note that this is _not_ the same as
    /// `width_pixels` times `bits_per_pixel` (converting to bytes per pixel
    /// first, of course). Some exotic resolutions have a different number of
    /// bytes per line than that.
    pitch: u64,
}

impl VESAFramebuffer32Bit {
    /// Create a `VESAFrambuffer` from a `limine` framebuffer, returned to the
    /// kernel from the `limine` bootloader. Returns `Err` if the framebuffer
    /// is not 32 bits per pixel, and various other invariants aren't met.
    ///
    /// # Safety
    ///
    /// Caller needs to ensure that the framebuffer is valid, and probably
    /// blindly trust `limine`.
    pub unsafe fn from_limine_framebuffer(fb: &limine::LimineFramebuffer) -> Result<Self, &str> {
        if fb.bpp != 32 {
            return Err("limine framebuffer must be 32 bits per pixel");
        }

        // These assertions ensure we can write `ARGB` values directly to the
        // framebuffer.
        if fb.red_mask_size != 8 {
            return Err("limine framebuffer must use 8 bits for red");
        }
        if fb.green_mask_size != 8 {
            return Err("limine framebuffer must use 8 bits for green");
        }
        if fb.blue_mask_size != 8 {
            return Err("limine framebuffer must use 8 bits for blue");
        }
        if fb.red_mask_shift != 16 {
            return Err("limine framebuffer must shift red mask by 16 bits");
        }
        if fb.green_mask_shift != 8 {
            return Err("limine framebuffer must shift green mask by 8 bits");
        }
        if fb.blue_mask_shift != 0 {
            return Err("limine framebuffer must shift blue mask by 0 bits");
        }

        let address = fb
            .address
            .as_ptr()
            .expect("failed to convert limine address to pointer");

        Ok(Self {
            address: address as *mut u8,
            width_pixels: fb.width as usize,
            height_pixels: fb.height as usize,
            pitch: fb.pitch,
        })
    }

    pub fn width_pixels(&self) -> usize {
        self.width_pixels
    }

    pub fn height_pixels(&self) -> usize {
        self.height_pixels
    }

    /// Draw the give `ARGB32Bit` color to the given pixel coordinates, where x
    /// is the column, y is the row, and (0, 0) is the top left corner.
    ///
    /// NOTE: Calling this in a loop to draw a bunch of pixels can be slow
    /// because byte offsets need to be recomputed every time, and we aren't
    /// necessarily accessing memory in the most efficient way. More specific
    /// functions should be used, like functions to draw specific shapes or
    /// glyphs.
    ///
    /// Also, we don't need `&mut self` here technically since we are doing
    /// `unsafe` under the hood, but it's nice to have to prevent data races.
    pub fn draw_pixel(&mut self, x: usize, y: usize, color: ARGB32Bit) {
        assert!(x < self.width_pixels, "x coordinate out of bounds");
        assert!(y < self.height_pixels, "y coordinate out of bounds");

        // This is safe to the caller as long as the framebuffer is valid. The
        // asserts above might panic, but that isn't `unsafe`.
        let bytes_per_pixel = 4; // Assumption of this type is 32 bits (4 bytes) per pixel
        let pixel_offset = y * (self.pitch as usize) + x * bytes_per_pixel;
        unsafe {
            *(self.address.add(pixel_offset) as *mut ARGB32Bit) = color;
        }
    }

    /// Draws the given Nx8 bitmap to the framebuffer. Use the foreground color
    /// for `1` bits and the background for `0` bits.
    pub fn draw_bitmap<S: bv::BitStore>(
        &mut self,
        x: usize,
        y: usize,
        bitmap: &bv::BitSlice<S, bitvec::order::Msb0>,
        bits_per_row: usize,
        foreground: ARGB32Bit,
        background: ARGB32Bit,
    ) {
        assert!(x < self.width_pixels, "x coordinate out of bounds");
        assert!(y < self.height_pixels, "y coordinate out of bounds");

        for (j, row) in bitmap.chunks(bits_per_row).enumerate() {
            for (i, bit) in row.iter().enumerate() {
                let color = if *bit { foreground } else { background };
                // TODO: Calling draw_pixel is inefficient because we have to
                // recompute the byte position every time.
                self.draw_pixel(x + i, y + j, color);
            }
        }
    }
}

/// The mask size is the number of `1` bits in the color's mask, and the mask
/// shift is how far right to shift the mask over to get the color value. For
/// example, if the red mask size is 8 and the shift is 16, the red value is
/// 0x00FF0000.
///
/// N.B. This function is unused for now, but it is kept around in case we want
/// to support more VESA modes in the future.
#[allow(dead_code)]
fn color_value_from_mask(mask_size: u8, mask_shift: u8) -> u32 {
    // See https://stackoverflow.com/a/1392065 for an explanation of the bit shifts.
    let mask = (1 << mask_size) - 1;
    mask << (24 - mask_shift)
}

/// A 32 bit color with alpha, red, green, and blue components. Used with
/// `VESAFramebuffer32Bit`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ARGB32Bit {
    // Note that the fields are in reverse order from how they are stored in
    // memory. This is because the x86 is little endian and we are expected to
    // write u32 values to memory.
    blue: u8,
    green: u8,
    red: u8,
    alpha: u8,
}

pub const ARGB32BIT_WHITE: ARGB32Bit = ARGB32Bit {
    alpha: 0xFF,
    red: 0xFF,
    green: 0xFF,
    blue: 0xFF,
};

pub const ARGB32BIT_BLACK: ARGB32Bit = ARGB32Bit {
    alpha: 0xFF,
    red: 0x00,
    green: 0x00,
    blue: 0x00,
};

pub const ARGB32BIT_RED: ARGB32Bit = ARGB32Bit {
    alpha: 0x00,
    red: 0xFF,
    green: 0x00,
    blue: 0x00,
};

pub const ARGB32BIT_GREEN: ARGB32Bit = ARGB32Bit {
    alpha: 0xFF,
    red: 0x00,
    green: 0xFF,
    blue: 0x00,
};

pub const ARGB32BIT_BLUE: ARGB32Bit = ARGB32Bit {
    alpha: 0xFF,
    red: 0x00,
    green: 0x00,
    blue: 0xFF,
};

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_color_value_from_mask() {
        assert_eq!(color_value_from_mask(8, 16), 0x0000FF00);
        assert_eq!(color_value_from_mask(8, 8), 0x00FF0000);
        assert_eq!(color_value_from_mask(8, 0), 0xFF000000);
    }
}
