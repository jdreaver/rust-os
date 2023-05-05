/// # Safety
///
/// If the string is not null-terminated, this will happily iterate through
/// memory and print garbage until it finds a null byte or we hit a protection
/// fault because we tried to ready a page we don't have access to.
pub(crate) unsafe fn c_str_from_pointer(ptr: *const u8, max_size: usize) -> &'static str {
    let mut len: usize = 0;
    while len < max_size {
        let c = *ptr.add(len);
        if c == 0 {
            break;
        }
        len += 1;
    }

    let slice = core::slice::from_raw_parts(ptr, len);
    core::str::from_utf8(slice).unwrap_or("<invalid utf8>")
}
