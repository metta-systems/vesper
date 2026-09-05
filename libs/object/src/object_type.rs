use crate::CapError;

/// One-byte wire object type, including unknown or reserved kind indices.
///
/// Bit 7 selects architecture-specific types; bits 6–0 hold the category-local
/// index. Decode into `CoreType` or `ArchType` to validate that index.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ObjectType(u8);

impl ObjectType {
    /// Bit indicating an architecture-specific capability.
    pub const ARCH_BIT: u8 = 0x80;

    /// Encode a known core kind as a wire object type.
    pub const fn from_core(kind: CoreType) -> Self {
        Self(kind.as_u8())
    }

    /// Encode a known architecture kind as a wire object type.
    pub const fn from_arch(kind: ArchType) -> Self {
        Self(Self::ARCH_BIT | kind.as_u8())
    }

    /// Check if this is an architecture-specific type.
    #[inline(always)]
    pub const fn is_arch(&self) -> bool {
        (self.0 & Self::ARCH_BIT) != 0
    }

    /// Check if this is a core type.
    #[inline(always)]
    pub const fn is_core(&self) -> bool {
        !self.is_arch()
    }

    /// Get the category-local index, stripping the architecture bit.
    #[inline(always)]
    pub const fn index(&self) -> u8 {
        self.0 & !Self::ARCH_BIT
    }

    /// Get the complete wire value, including the architecture bit.
    #[inline(always)]
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl From<u8> for ObjectType {
    /// Preserve a wire value without assuming its kind is supported or known.
    fn from(value: u8) -> Self {
        Self(value)
    }
}

// Keep each kind's variant, alias, and category-local ID in one declaration.
// Category validation and wire encoding stay outside the catalogue macro.
macro_rules! define_object_types {
    (
        $(#[$meta:meta])*
        $kind:ident, $constructor:ident, $unknown:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $alias:ident = $index:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u8)]
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub enum $kind {
            $(
                $(#[$variant_meta])*
                $variant = $index,
            )+
        }

        impl $kind {
            /// Get the category-local index, without the architecture bit.
            #[inline(always)]
            pub const fn as_u8(self) -> u8 {
                self as u8
            }
        }

        impl TryFrom<u8> for $kind {
            type Error = CapError;

            #[inline]
            fn try_from(index: u8) -> Result<Self, Self::Error> {
                match index {
                    $($index => Ok(Self::$variant),)+
                    _ => Err(CapError::$unknown(index)),
                }
            }
        }

        impl ObjectType {
            $(
                $(#[$variant_meta])*
                pub const $alias: Self = Self::$constructor($kind::$variant);
            )+
        }

        impl From<$kind> for ObjectType {
            fn from(kind: $kind) -> Self {
                Self::$constructor(kind)
            }
        }

        $(const _: () = assert!($index < ObjectType::ARCH_BIT);)+
    };
}

define_object_types! {
    /// Known core object kinds, decoded after checking the category bit.
    CoreType, from_core, UnknownCoreType {
        /// No capability.
        Null => NULL = 0,
        /// Creates memory-backed objects, including new key tables.
        Untyped => UNTYPED = 1,
        /// Protection domain.
        Domain => DOMAIN = 2,
        /// Capability table.
        KeyTable => KEY_TABLE = 3,
        /// CPU time capability.
        Time => TIME = 4,
        /// Synchronous IPC endpoint.
        Endpoint => ENDPOINT = 5,
        /// Coalescing notification endpoint.
        Notification => NOTIFICATION = 6,
        /// Monotonic event count.
        EventCount => EVENT_COUNT = 7,
        /// Shareable buffer.
        Buffer => BUFFER = 8,
        /// One-shot reply authority.
        Reply => REPLY = 9,
        /// Debug console; availability is a separate policy decision.
        DebugConsole => DEBUG_CONSOLE = 127,
    }
}

define_object_types! {
    /// Known architecture kinds. IDs are category-local; support is target-specific.
    ArchType, from_arch, UnknownArchType {
        Frame => FRAME = 0,
        PageTable => PAGE_TABLE = 1,
        VSpace => VSPACE = 2,
        ASIDPool => ASID_POOL = 3,
        ASID => ASID = 4,
        IOSpace => IO_SPACE = 5,
        /// x86 I/O ports.
        IOPort => IO_PORT = 6,
        IRQHandler => IRQ_HANDLER = 7,
        IRQControl => IRQ_CONTROL = 8,
    }
}

impl TryFrom<ObjectType> for CoreType {
    type Error = CapError;

    #[inline]
    fn try_from(object_type: ObjectType) -> Result<Self, Self::Error> {
        if object_type.is_arch() {
            return Err(CapError::NotCoreType(object_type));
        }
        Self::try_from(object_type.index())
    }
}

impl TryFrom<ObjectType> for ArchType {
    type Error = CapError;

    #[inline]
    fn try_from(object_type: ObjectType) -> Result<Self, Self::Error> {
        if object_type.is_core() {
            return Err(CapError::NotArchType(object_type));
        }
        Self::try_from(object_type.index())
    }
}
