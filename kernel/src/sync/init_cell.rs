use super::once_cell::OnceCell;

/// A cell that can be initialized only once. This is useful because we can
/// share it between multiple threads without having to use a mutex, and since
/// the value can only be written once, we don't need a mutable reference to
/// write to it, so we can store this value as a static.
#[derive(Debug)]
pub(crate) struct InitCell<T> {
    cell: OnceCell<T>,
}

impl<T> InitCell<T> {
    pub(crate) const fn new() -> Self {
        Self {
            cell: OnceCell::new(),
        }
    }

    pub(crate) fn init(&self, value: T) {
        unsafe {
            self.cell.set(value);
        }
    }

    pub(crate) fn get(&self) -> Option<&T> {
        self.cell.get_ref()
    }
}
