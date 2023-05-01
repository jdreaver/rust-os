#![cfg_attr(not(test), no_std)]

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
pub struct VESAFrambuffer32Bit {
    /// Pointer to the start of the framebuffer.
    ///
    /// N.B. We store this as `u8` so we do per-byte pointer arithmetic, and we
    /// convert to a wider pointer when we do writes.
    address: *mut u8,

    width_pixels: u64,
    height_pixels: u64,

    /// Number of bytes per horizontal line. Note that this is _not_ the same as
    /// `width_pixels` times `bits_per_pixel` (converting to bytes per pixel
    /// first, of course). Some exotic resolutions have a different number of
    /// bytes per line than that.
    pitch: u64,
}

impl VESAFrambuffer32Bit {
    /// Create a `VESAFrambuffer` from a `limine` framebuffer, returned to the
    /// kernel from the `limine` bootloader.
    ///
    /// # Safety
    ///
    /// Caller needs to ensure that the framebuffer is valid, and probably
    /// blindly trust `limine`. (We also check for 32 bits per pixel, but we
    /// panic if that fails. It isn't `unsafe`.)
    pub unsafe fn from_limine_framebuffer(fb: &limine::LimineFramebuffer) -> Self {
        assert_eq!(fb.bpp, 32, "limine framebuffer must be 32 bits per pixel");

        // These assertions ensure we can write `ARGB` values directly to the
        // framebuffer.
        assert_eq!(
            fb.red_mask_size, 8,
            "limine framebuffer must use 8 bits for red"
        );
        assert_eq!(
            fb.green_mask_size, 8,
            "limine framebuffer must use 8 bits for green"
        );
        assert_eq!(
            fb.blue_mask_size, 8,
            "limine framebuffer must use 8 bits for blue"
        );

        assert_eq!(
            fb.red_mask_shift, 16,
            "limine framebuffer must shift red mask by 16 bits"
        );
        assert_eq!(
            fb.green_mask_shift, 8,
            "limine framebuffer must shift green mask by 8 bits"
        );
        assert_eq!(
            fb.blue_mask_shift, 0,
            "limine framebuffer must shift blue mask by 0 bits"
        );

        let address = fb
            .address
            .as_ptr()
            .expect("failed to convert limine address to pointer");

        Self {
            address: address as *mut u8,
            width_pixels: fb.width,
            height_pixels: fb.height,
            pitch: fb.pitch,
        }
    }

    /// Draw the give `ARGB32Bit` color to the given pixel coordinates, where x
    /// is the column, y is the row, and (0, 0) is the top left corner.
    ///
    /// NOTE: Calling this in a loop to draw a bunch of pixels can be slow
    /// because byte offsets need to be recomputed every time, and we aren't
    /// necessarily accessing memory in the most efficient way. More specific
    /// functions should be used, like functions to draw specific shapes or
    /// glyphs.
    pub fn draw_pixel(&self, x: usize, y: usize, color: ARGB32Bit) {
        assert!((x as u64) < self.width_pixels, "x coordinate out of bounds");
        assert!(
            (y as u64) < self.height_pixels,
            "y coordinate out of bounds"
        );

        // This is safe to the caller as long as the framebuffer is valid. The
        // asserts above might panic, but that isn't `unsafe`.
        let bytes_per_pixel = 4; // Assumption of this type is 32 bits (4 bytes) per pixel
        let pixel_offset = y * (self.pitch as usize) + x * bytes_per_pixel;
        unsafe {
            *(self.address.add(pixel_offset) as *mut ARGB32Bit) = color;
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
/// `VESAFrambuffer32Bit`.
#[repr(C)]
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
