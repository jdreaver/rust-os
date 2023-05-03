//! This module implements a simple, static, append-only ring buffer.
//!
//! The ring buffer is implemented as a fixed-size array of elements with a
//! pointer to the next free location.

#![cfg_attr(not(test), no_std)]

/// A simple, static, append-only ring buffer.
#[derive(Debug)]
pub struct RingBuffer<T, const N: usize> {
    /// The fixed-size array of elements.
    elements: [Option<T>; N],

    /// The index of the next free location.
    next_free: usize,
}

impl<T: Copy, const N: usize> RingBuffer<T, N> {
    pub fn new() -> Self {
        Self {
            elements: [None; N],
            next_free: 0,
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
    pub fn get(&self, index: usize) -> Option<T> {
        // Out of bounds
        if index >= self.elements.len() {
            return None;
        }

        if index < self.next_free {
            self.elements[self.next_free - index - 1]
        } else {
            self.elements[N - 1 - (index - self.next_free)]
        }


        // if index < self.next_free {
        //     return self.elements[self.next_free - index - 1];
        // } else if index < self.next_free + N {
        //     return self.elements[N - (index - self.next_free - 1)];
        // }

        // // Index is out of bounds
        // None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO proptest that compares this with a fixed-size Deque

    #[test]
    fn push_and_get() {
        let mut buffer = RingBuffer::<u8, 3>::new();
        buffer.push(1);
        println!("{:?}", buffer);
        assert_eq!(buffer.get(0), Some(1));
        assert_eq!(buffer.get(1), None);

        buffer.push(2);
        assert_eq!(buffer.get(0), Some(2));
        assert_eq!(buffer.get(1), Some(1));
        assert_eq!(buffer.get(2), None);

        buffer.push(3);
        assert_eq!(buffer.get(0), Some(3));
        assert_eq!(buffer.get(1), Some(2));
        assert_eq!(buffer.get(2), Some(1));
        assert_eq!(buffer.get(3), None);

        // Wrap around
        buffer.push(4);
        assert_eq!(buffer.get(0), Some(4));
        assert_eq!(buffer.get(1), Some(3));
        assert_eq!(buffer.get(2), Some(2));
        assert_eq!(buffer.get(3), None);
    }
}
