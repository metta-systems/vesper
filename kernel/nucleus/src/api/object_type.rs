/// Object type discriminant with architectural bit.
///
/// Bit 7 (high bit) indicates architecture-specific type.
///
/// Layout:
///
///   Bit 7    Bits 6-0
///   ─────    ────────
///     0      Core type (0-127)
///     1      Arch type (0-127)
///
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ObjectType(u8);

impl ObjectType {
    /// Bit indicating architecture-specific capability
    pub const ARCH_BIT: u8 = 0x80;

    // ─── Core Types (0x00 - 0x7F) ───
    pub const NULL: Self = Self(0);
    pub const UNTYPED: Self = Self(1);
    pub const DOMAIN: Self = Self(2);
    pub const KEY_TABLE: Self = Self(3);
    pub const NOTIFICATION: Self = Self(4);
    pub const EVENT_COUNT: Self = Self(5);
    pub const ENDPOINT: Self = Self(6);
    pub const TIME: Self = Self(7);
    pub const BUFFER: Self = Self(8);
    pub const REPLY: Self = Self(9);
    // Reserved: 10-126
    pub const DEBUG_CONSOLE: Self = Self(127); // only #cfg(debug)

    // ─── Arch Types (0x80 - 0xFF) ───
    pub const FRAME: Self = Self(Self::ARCH_BIT | 0);
    pub const PAGE_TABLE: Self = Self(Self::ARCH_BIT | 1);
    pub const VSPACE: Self = Self(Self::ARCH_BIT | 2);
    pub const ASID_POOL: Self = Self(Self::ARCH_BIT | 3);
    pub const ASID: Self = Self(Self::ARCH_BIT | 4);
    pub const IO_SPACE: Self = Self(Self::ARCH_BIT | 5);
    pub const IO_PORT: Self = Self(Self::ARCH_BIT | 6); // x86 only
    pub const IRQ_HANDLER: Self = Self(Self::ARCH_BIT | 7);
    pub const IRQ_CONTROL: Self = Self(Self::ARCH_BIT | 8);
    // Reserved: 0x89 - 0xFF

    /// Check if this is an architecture-specific type.
    #[inline(always)]
    pub const fn is_arch(&self) -> bool {
        (self.0 & Self::ARCH_BIT) != 0
    }

    /// Check if this is a core type.
    #[inline(always)]
    pub const fn is_core(&self) -> bool {
        (self.0 & Self::ARCH_BIT) == 0
    }

    /// Get the type index within its category (strips arch bit).
    #[inline(always)]
    pub const fn index(&self) -> u8 {
        self.0 & !Self::ARCH_BIT
    }

    /// Raw value
    #[inline(always)]
    pub const fn as_u8(&self) -> u8 {
        self.0
    }
}

// ═══════════════════════════════════════════════════════════════════
// CORE TYPE ENUM (FOR MATCH)
// ═══════════════════════════════════════════════════════════════════

/// Core object types - used for match dispatch after arch check
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CoreType {
    /// No capability
    Null = 0,
    /// Creates objects (including new key tables)
    Untyped = 1,
    /// Protection domain
    Domain = 2,
    /// capability table itself
    KeyTable = 3,
    /// Time capability
    Time = 4,
    /// Endpoint capability
    Endpoint = 5,
    /// Notification endpoint capability
    Notification = 6,
    /// Event count endpoint capability
    EventCount = 7,
    /// Shareable buffer capability
    Buffer = 8,
    Reply = 9,
    DebugConsole = 127,
}

impl TryFrom<ObjectType> for CoreType {
    type Error = CapError;

    #[inline]
    fn try_from(ot: ObjectType) -> Result<Self, Self::Error> {
        if ot.is_arch() {
            return Err(CapError::NotCoreType);
        }
        match ot.index() {
            0 => Ok(CoreType::Null),
            1 => Ok(CoreType::Untyped),
            2 => Ok(CoreType::Domain),
            3 => Ok(CoreType::KeyTable),
            4 => Ok(CoreType::Notification),
            5 => Ok(CoreType::EventCount),
            6 => Ok(CoreType::Endpoint),
            7 => Ok(CoreType::Time),
            8 => Ok(CoreType::Buffer),
            9 => Ok(CoreType::Reply),
            127 => Ok(CoreType::DebugConsole),
            _ => Err(CapError::UnknownCoreType(ot.index())),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// ARCH TYPE ENUM (ARCHITECTURE-SPECIFIC)
// ═══════════════════════════════════════════════════════════════════

/// Architecture-specific object types
///
/// This is defined per-architecture but the indices are the same.
/// The actual struct types differ per architecture.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArchType {
    Frame = 0,
    PageTable = 1,
    VSpace = 2,
    ASIDPool = 3,
    ASID = 4,
    IOSpace = 5,
    IOPort = 6,
    IRQHandler = 7,
    IRQControl = 8,
}

impl TryFrom<ObjectType> for ArchType {
    type Error = CapError;

    #[inline]
    fn try_from(ot: ObjectType) -> Result<Self, Self::Error> {
        if !ot.is_arch() {
            return Err(CapError::NotArchType);
        }
        match ot.index() {
            0 => Ok(ArchType::Frame),
            1 => Ok(ArchType::PageTable),
            2 => Ok(ArchType::VSpace),
            3 => Ok(ArchType::ASIDPool),
            4 => Ok(ArchType::ASID),
            5 => Ok(ArchType::IOSpace),
            6 => Ok(ArchType::IOPort),
            7 => Ok(ArchType::IRQHandler),
            8 => Ok(ArchType::IRQControl),
            _ => Err(CapError::UnknownArchType(ot.index())),
        }
    }
}
