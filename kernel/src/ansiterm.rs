//! ANSI terminal escape codes. See:
//! - <https://en.wikipedia.org/wiki/ANSI_escape_code>
//! - <https://wiki.osdev.org/Terminals>

use core::fmt;

/// Start of an ANSI escape or command sequence. Often represented as `\033` (octal) or
/// `\x1B` (hexadecimal), but equal to 27.
pub(crate) const ANSI_ESCAPE: u8 = 27;

pub(crate) const CLEAR_FORMAT: AnsiEscapeSequence =
    AnsiEscapeSequence::SelectGraphicRendition(SelectGraphicRendition::Reset);

pub(crate) const BOLD: AnsiEscapeSequence =
    AnsiEscapeSequence::SelectGraphicRendition(SelectGraphicRendition::Bold);

pub(crate) const GREEN: AnsiEscapeSequence = AnsiEscapeSequence::SelectGraphicRendition(
    SelectGraphicRendition::ForegroundColor(Color::Green),
);

/// An ANSI escape sequence value that can be used in format strings. The meat
/// of the logic for printing the sequence is in the `Display` trait
/// implementation.
pub(crate) enum AnsiEscapeSequence {
    SelectGraphicRendition(SelectGraphicRendition),
    MoveCursorTopLeft,
    ClearScreenFromCursorToEnd,
    ClearEntireLine,
}

impl fmt::Display for AnsiEscapeSequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x1B[")?;
        match self {
            Self::SelectGraphicRendition(sgr) => write!(f, "{sgr}")?,
            Self::MoveCursorTopLeft => write!(f, "H")?,
            Self::ClearScreenFromCursorToEnd => write!(f, "J")?,
            Self::ClearEntireLine => write!(f, "2K")?,
        }
        Ok(())
    }
}

/// <https://en.wikipedia.org/wiki/ANSI_escape_code#SGR_(Select_Graphic_Rendition)_parameters>
#[allow(dead_code)]
pub(crate) enum SelectGraphicRendition {
    Reset,
    Bold,
    ForegroundColor(Color),
    BackgroundColor(Color),
    DefaultForegroundColor,
    DefaultBackgroundColor,
}

impl fmt::Display for SelectGraphicRendition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // N.B. \x1B[ already added from outer `AnsiEscapeSequence` impl
        match self {
            Self::Reset => write!(f, "0")?,
            Self::Bold => write!(f, "1")?,
            Self::ForegroundColor(color) => write!(f, "{}", color.foreground_byte())?,
            Self::BackgroundColor(color) => write!(f, "{}", color.background_byte())?,
            Self::DefaultForegroundColor => write!(f, "39")?,
            Self::DefaultBackgroundColor => write!(f, "49")?,
        }
        write!(f, "m")
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
}

impl Color {
    fn foreground_byte(self) -> u8 {
        match self {
            Self::Black => 30,
            Self::Red => 31,
            Self::Green => 32,
            Self::Yellow => 33,
            Self::Blue => 34,
            Self::Magenta => 35,
            Self::Cyan => 36,
            Self::White => 37,
        }
    }

    fn background_byte(self) -> u8 {
        match self {
            Self::Black => 40,
            Self::Red => 41,
            Self::Green => 42,
            Self::Yellow => 43,
            Self::Blue => 44,
            Self::Magenta => 45,
            Self::Cyan => 46,
            Self::White => 47,
        }
    }
}
