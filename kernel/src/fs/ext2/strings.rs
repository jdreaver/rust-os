use core::fmt;

/// Wrapper around a byte array that represents a nul-terminated string.
#[repr(transparent)]
#[derive(Copy, Clone)]
pub(crate) struct CStringBytes<const N: usize>([u8; N]);

impl<const N: usize> CStringBytes<N> {
    pub(crate) fn as_str(&self) -> &str {
        c_str_from_bytes(&self.0)
    }
}

impl<const N: usize> fmt::Debug for CStringBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CStringBytes").field(&self.as_str()).finish()
    }
}

/// Creates a null-terminated string from a byte slice.
pub(crate) fn c_str_from_bytes(bytes: &[u8]) -> &str {
    let nul_location = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..nul_location]).unwrap_or("<invalid UTF-8>")
}
