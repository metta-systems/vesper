#![no_std]
#![no_main]

pub mod debug_console;
pub mod domain;
pub mod key;
pub mod key_table;
pub mod object_type;
pub mod rights;

#[cfg(feature = "debug_kernel")]
pub use debug_console::DebugConsoleKey;

pub use {
    key::Key,
    key_table::KeySlot,
    object_type::{ArchType, CoreType, ObjectType},
    rights::Rights,
};

pub type SyscallResult = core::result::Result<(u64, u64), CapError>;

pub enum CapError {
    Unknown,
    NullCapability,
    InvalidDomain,
    InvalidPointer,
    InsufficientRights,
    NotMapped,
    AlreadyMapped,
    InvalidOperation,
    ASIDPoolExhausted,
    NoASIDAssigned,
    InvalidSlot(KeySlot),
    EmptySlot(KeySlot),
    SlotOccupied(KeySlot),
    // Key types
    NotCoreType(ObjectType),
    UnknownCoreType(u8),
    UnsupportedCoreType(CoreType),
    NotArchType(ObjectType),
    UnknownArchType(u8),
    UnsupportedArchType(ArchType),
    InvalidObjectType(ObjectType),
    TypeMismatch {
        expected: ObjectType,
        found: ObjectType,
    },
    // Object pools
    InsufficientMemory,
    PoolExhausted,
    InvalidSize(usize),
    InvalidFrameSize(usize),
}

impl CapError {
    pub fn code(self) -> (u64, u64, u64) {
        match self {
            CapError::Unknown => (1, 0, 0),
            CapError::NullCapability => (2, 0, 0),
            CapError::InvalidDomain => (3, 0, 0),
            CapError::InvalidPointer => (4, 0, 0),
            CapError::InsufficientRights => (5, 0, 0),
            CapError::NotMapped => (6, 0, 0),
            CapError::AlreadyMapped => (7, 0, 0),
            CapError::InvalidOperation => (8, 0, 0),
            CapError::ASIDPoolExhausted => (9, 0, 0),
            CapError::NoASIDAssigned => (10, 0, 0),
            CapError::InvalidSlot(s) => (11, u64::from(s.0), 0),
            CapError::EmptySlot(s) => (12, u64::from(s.0), 0),
            CapError::SlotOccupied(s) => (13, u64::from(s.0), 0),
            CapError::NotCoreType(t) => (14, u64::from(t.as_u8()), 0),
            CapError::UnknownCoreType(t) => (15, u64::from(t), 0),
            CapError::UnsupportedCoreType(t) => (16, u64::from(t.as_u8()), 0),
            CapError::NotArchType(t) => (17, u64::from(t.as_u8()), 0),
            CapError::UnknownArchType(t) => (18, u64::from(t), 0),
            CapError::UnsupportedArchType(t) => (19, u64::from(t.as_u8()), 0),
            CapError::InvalidObjectType(t) => (20, u64::from(t.as_u8()), 0),
            CapError::TypeMismatch { expected, found } => {
                (21, u64::from(expected.as_u8()), u64::from(found.as_u8()))
            }
            CapError::InsufficientMemory => (22, 0, 0),
            CapError::PoolExhausted => (23, 0, 0),
            CapError::InvalidSize(s) => (24, s.try_into().unwrap(), 0),
            CapError::InvalidFrameSize(s) => (25, s.try_into().unwrap(), 0),
        }
    }
}
