use core::fmt;

use bitflags::bitflags;

/// See "2.2 Feature Bits"
///
/// N.B. The spec technically supports up to 128 bits of features, but
/// all the devices we use only need 64 bits.
pub(super) struct Features(u64);

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

impl Features {
    pub(super) fn new(features: u64) -> Self {
        Self(features)
    }

    pub(super) fn as_u64(&self) -> u64 {
        self.0
    }

    pub(super) fn negotiate_reserved_bits(&mut self, f: impl FnOnce(&mut ReservedFeatureBits)) {
        let mut reserved_bits = ReservedFeatureBits::from_bits_truncate(self.0);
        f(&mut reserved_bits);
        self.0 = reserved_bits.bits();
    }
}

impl fmt::Debug for Features {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reserved = ReservedFeatureBits::from_bits_truncate(self.0);
        f.debug_tuple("Features").field(&reserved).finish()
    }
}
