use core::fmt::Write;

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

/// A wrapper around a `Write` that handles indentation.
pub(crate) struct IndentWriter<'a, W: Write> {
    writer: &'a mut W,
    indent: usize,
    indent_delta: usize,
    on_start_of_line: bool,
}

impl<W: Write> IndentWriter<'_, W> {
    pub(crate) fn new(writer: &mut W, indent_delta: usize) -> IndentWriter<W> {
        IndentWriter {
            writer,
            indent: 0,
            indent_delta,
            on_start_of_line: true,
        }
    }

    pub(crate) fn indent(&mut self) {
        self.indent += self.indent_delta;
    }

    pub(crate) fn unindent(&mut self) {
        self.indent -= self.indent_delta;
    }
}

impl<W: Write> Write for IndentWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            // Do indentation if we are at the start of a line. If we are about
            // to print a newline though, don't worry about it.
            if self.on_start_of_line && c != '\n' {
                write!(self.writer, "{}", " ".repeat(self.indent))?;
                self.on_start_of_line = false;
            }

            // Write the character
            write!(self.writer, "{c}")?;

            // If we just printed a newline, we are at the start of a line.
            self.on_start_of_line = c == '\n';
        }
        Ok(())
    }
}
