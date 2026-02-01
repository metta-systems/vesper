#![no_std]

pub mod debug_console;
pub mod domain;
pub mod key;
pub mod key_table;
pub mod object_type;
pub mod rights;

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
