//! Shared capability-invocation status words (`x0` on `AArch64`).
//!
//! These values are wire ABI, shared by kernel encoding and client decoding.
//! Unknown nonzero values must remain observable rather than becoming success.

pub const SUCCESS: u64 = 0;
pub const UNKNOWN: u64 = 1;
pub const NULL_CAPABILITY: u64 = 2;
pub const INVALID_DOMAIN: u64 = 3;
pub const INVALID_POINTER: u64 = 4;
pub const INSUFFICIENT_RIGHTS: u64 = 5;
pub const NOT_MAPPED: u64 = 6;
pub const ALREADY_MAPPED: u64 = 7;
pub const INVALID_OPERATION: u64 = 8;
pub const ASID_POOL_EXHAUSTED: u64 = 9;
pub const NO_ASID_ASSIGNED: u64 = 10;
pub const INVALID_SLOT: u64 = 11;
pub const EMPTY_SLOT: u64 = 12;
pub const SLOT_OCCUPIED: u64 = 13;
pub const NOT_CORE_TYPE: u64 = 14;
pub const UNKNOWN_CORE_TYPE: u64 = 15;
pub const UNSUPPORTED_CORE_TYPE: u64 = 16;
pub const NOT_ARCH_TYPE: u64 = 17;
pub const UNKNOWN_ARCH_TYPE: u64 = 18;
pub const UNSUPPORTED_ARCH_TYPE: u64 = 19;
pub const INVALID_OBJECT_TYPE: u64 = 20;
pub const TYPE_MISMATCH: u64 = 21;
pub const INSUFFICIENT_MEMORY: u64 = 22;
pub const POOL_EXHAUSTED: u64 = 23;
pub const INVALID_SIZE: u64 = 24;
pub const INVALID_FRAME_SIZE: u64 = 25;
