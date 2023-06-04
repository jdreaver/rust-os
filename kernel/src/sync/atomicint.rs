use core::fmt;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

/// Wrapper around an atomic integer type (via `AtomicInt`) that supports
/// transparently converting to/from a specific type.
#[derive(Debug)]
pub(crate) struct AtomicInt<I, T>
where
    I: AtomicIntTrait,
{
    atom: I::Atomic,
    _phantom: PhantomData<T>,
}

impl<I, T> AtomicInt<I, T>
where
    I: AtomicIntTrait + fmt::Display + Copy,
    T: From<I> + Into<I>,
{
    pub(crate) fn new(val: T) -> Self {
        Self {
            atom: <I as AtomicIntTrait>::new(val.into()),
            _phantom: PhantomData,
        }
    }

    pub(crate) fn load(&self) -> T {
        let val = <I as AtomicIntTrait>::load(&self.atom, Ordering::Acquire);
        T::from(val)
    }

    pub(crate) fn store(&self, val: T) {
        <I as AtomicIntTrait>::store(&self.atom, val.into(), Ordering::Release);
    }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = <I as AtomicIntTrait>::swap(&self.atom, val.into(), Ordering::Acquire);
        T::from(old_val)
    }
}

pub(crate) trait AtomicIntTrait {
    type Atomic;

    fn new(val: Self) -> Self::Atomic;
    fn load(atom: &Self::Atomic, order: Ordering) -> Self;
    fn store(atom: &Self::Atomic, val: Self, order: Ordering);
    fn swap(atom: &Self::Atomic, val: Self, order: Ordering) -> Self;
}

macro_rules! atomic_int_trait_impl {
    ($type:ty, $atom:ty) => {
        impl AtomicIntTrait for $type {
            type Atomic = $atom;

            fn new(val: Self) -> Self::Atomic {
                Self::Atomic::new(val)
            }

            fn load(atom: &Self::Atomic, order: Ordering) -> Self {
                atom.load(order)
            }

            fn store(atom: &Self::Atomic, val: Self, order: Ordering) {
                atom.store(val, order);
            }

            fn swap(atom: &Self::Atomic, val: Self, order: Ordering) -> Self {
                atom.swap(val, order)
            }
        }
    };
}

atomic_int_trait_impl!(u8, AtomicU8);
atomic_int_trait_impl!(u16, AtomicU16);
atomic_int_trait_impl!(u32, AtomicU32);
atomic_int_trait_impl!(u64, AtomicU64);

/// Wrapper around `AtomicInt` that allows fallible conversion, which is super
/// useful for enums.
#[derive(Debug)]
pub(crate) struct AtomicEnum<I, T>
where
    I: AtomicIntTrait,
    I::Atomic: fmt::Debug,
{
    int: AtomicInt<I, I>,
    _phantom: PhantomData<T>,
}

impl<I, T> AtomicEnum<I, T>
where
    I: AtomicIntTrait + fmt::Display + Copy,
    I::Atomic: fmt::Debug,
    T: TryFrom<I> + Into<I>,
{
    pub(crate) fn new(val: T) -> Self {
        Self {
            int: AtomicInt::new(val.into()),
            _phantom: PhantomData,
        }
    }

    fn convert_from_integer(val: I) -> T {
        T::try_from(val).map_or_else(
            |_| {
                panic!("ERROR: Invalid enum value {val}");
            },
            |enum_val| enum_val,
        )
    }

    pub(crate) fn load(&self) -> T {
        let val = self.int.load();
        Self::convert_from_integer(val)
    }

    pub(crate) fn swap(&self, val: T) -> T {
        let old_val = self.int.swap(val.into());
        Self::convert_from_integer(old_val)
    }
}
