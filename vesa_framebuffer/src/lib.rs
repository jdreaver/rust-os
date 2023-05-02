#![cfg_attr(not(test), no_std)]

pub mod framebuffer;
pub mod text_buffer;

pub use framebuffer::*;
pub use text_buffer::*;
