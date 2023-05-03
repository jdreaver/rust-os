//! This module implements a simple, static, append-only ring buffer.
//!
//! The ring buffer is implemented as a fixed-size array of elements with a
//! pointer to the next free location.

#![cfg_attr(not(test), no_std)]
#![warn(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cargo_common_metadata,
    clippy::implicit_hasher,
    clippy::implicit_return,
    clippy::missing_const_for_fn,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::multiple_crate_versions,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::suboptimal_flops,
    clippy::wildcard_imports
)]

/// A simple, static, append-only ring buffer.
#[derive(Debug)]
pub struct RingBuffer<T, const N: usize> {
    /// The fixed-size array of elements.
    elements: [Option<T>; N],

    /// The index of the next free location.
    next_free: usize,
}

impl<T: Copy, const N: usize> RingBuffer<T, N> {
    pub const fn new() -> Self {
        Self {
            elements: [None; N],
            next_free: 0,
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        // If we have wrapped around, then the length is the number of elements.
        if self.elements[self.next_free].is_some() {
            N
        } else {
            self.next_free
        }
    }

    /// Append an element to the ring buffer. This will overwrite the oldest
    /// element if the buffer is full.
    pub fn push(&mut self, element: T) {
        self.elements[self.next_free] = Some(element);
        self.next_free = (self.next_free + 1) % N;
    }

    /// Get the element at the given index, where the index counts backwards
    /// from the latest element. For example, `0` is the element most recently
    /// pushed, `1` is the second most recent element, and `N-1` is the oldest
    /// element. Returns `None` if no element exists at the given index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        // Out of bounds
        if index >= self.elements.len() {
            return None;
        }

        if index < self.next_free {
            self.elements
                .get_mut(self.next_free - index - 1)
                .unwrap()
                .as_mut()
        } else {
            self.elements
                .get_mut(N - 1 - (index - self.next_free))
                .unwrap()
                .as_mut()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO proptest that compares this with a fixed-size Deque

    #[test]
    fn push_and_get() {
        let mut buffer = RingBuffer::<u8, 3>::new();
        assert_eq!(buffer.len(), 0);

        buffer.push(1);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.get_mut(0), Some(&mut 1));
        assert_eq!(buffer.get_mut(1), None);

        buffer.push(2);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.get_mut(0), Some(&mut 2));
        assert_eq!(buffer.get_mut(1), Some(&mut 1));
        assert_eq!(buffer.get_mut(2), None);

        buffer.push(3);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.get_mut(0), Some(&mut 3));
        assert_eq!(buffer.get_mut(1), Some(&mut 2));
        assert_eq!(buffer.get_mut(2), Some(&mut 1));
        assert_eq!(buffer.get_mut(3), None);

        // Wrap around
        buffer.push(4);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.get_mut(0), Some(&mut 4));
        assert_eq!(buffer.get_mut(1), Some(&mut 3));
        assert_eq!(buffer.get_mut(2), Some(&mut 2));
        assert_eq!(buffer.get_mut(3), None);
    }
}
