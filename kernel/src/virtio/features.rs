use core::fmt;
use core::marker::PhantomData;

use bitflags::{bitflags, Flags};

/// See "2.2 Feature Bits". The `F` type parameter is used to specify
/// the device-specific feature bits via a `Flags` implementation.
///
/// N.B. The spec technically supports up to 128 bits of features, but
/// all the devices we use only need 64 bits.
pub(super) struct Features<F> {
    bits: u64,
    _phantom: PhantomData<F>,
}

bitflags! {
    #[derive(Debug)]
    #[repr(transparent)]
    /// See "2.2 Feature Bits" and "6 Reserved Feature Bits" in the VirtIO spec.
    pub(super) struct ReservedFeatureBits: u64 {
        const INDIRECT_DESC      = 1 << 28;
        const EVENT_IDX          = 1 << 29;
        const VERSION_1          = 1 << 32;
        const ACCESS_PLATFORM    = 1 << 33;
        const RING_PACKED        = 1 << 34;
        const IN_ORDER           = 1 << 35;
        const ORDER_PLATFORM     = 1 << 36;
        const SR_IOV             = 1 << 37;
        const NOTIFICATION_DATA  = 1 << 38;
        const NOTIF_CONFIG_DATA  = 1 << 39;
        const RING_RESET         = 1 << 40;
    }
}

impl<F> Features<F>
where
    F: Flags<Bits = u64>,
{
    pub(super) fn new(bits: u64) -> Self {
        Self {
            bits,
            _phantom: PhantomData,
        }
    }

    pub(super) fn as_u64(&self) -> u64 {
        self.bits
    }

    pub(super) fn negotiate_reserved_bits(&mut self, f: impl FnOnce(&mut ReservedFeatureBits)) {
        self.negotiate_flags_impl(f);
    }

    pub(super) fn negotiate_device_bits(&mut self, f: impl FnOnce(&mut F))
    where
        F: Flags<Bits = u64>,
    {
        self.negotiate_flags_impl(f);
    }

    // Separate function to make type parameter stuff clearer between reserved
    // and device flags.
    fn negotiate_flags_impl<I>(&mut self, f: impl FnOnce(&mut I))
    where
        I: Flags<Bits = u64>,
    {
        let mut bits = I::from_bits_retain(self.bits);
        f(&mut bits);
        self.bits = bits.bits();
    }
}

impl<F> fmt::Debug for Features<F>
where
    F: Flags<Bits = u64> + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Features")
            .field(
                "reserved",
                &ReservedFeatureBits::from_bits_truncate(self.bits),
            )
            .field("device_specific", &F::from_bits_truncate(self.bits))
            .finish()
    }
}
