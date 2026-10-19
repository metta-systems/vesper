//! Shared capability-invocation status words (`x0` on `AArch64`).
//!
//! These values are wire ABI, shared by kernel encoding and client decoding.
//! Unknown nonzero values must remain observable rather than becoming success.

use crate::KeySlot;

pub const SUCCESS: u64 = 0;
pub const UNKNOWN: u64 = 1;
pub const NULL_CAPABILITY: u64 = 2;
/// The current or target Thread is missing or stale, or a capability entry
/// came from a stack origin that has no schedulable caller. Details are zero.
pub const INVALID_THREAD: u64 = 3;
/// The `AddressSpace` is stale. Details are zero.
pub const INVALID_ADDRESS_SPACE: u64 = 4;
pub const INVALID_POINTER: u64 = 5;
pub const INSUFFICIENT_RIGHTS: u64 = 6;
pub const NOT_MAPPED: u64 = 7;
pub const ALREADY_MAPPED: u64 = 8;
pub const INVALID_OPERATION: u64 = 9;
pub const ASID_POOL_EXHAUSTED: u64 = 10;
pub const NO_ASID_ASSIGNED: u64 = 11;
pub const INVALID_SLOT: u64 = 12;
pub const EMPTY_SLOT: u64 = 13;
pub const SLOT_OCCUPIED: u64 = 14;
pub const NOT_CORE_TYPE: u64 = 15;
pub const UNKNOWN_CORE_TYPE: u64 = 16;
pub const UNSUPPORTED_CORE_TYPE: u64 = 17;
pub const NOT_ARCH_TYPE: u64 = 18;
pub const UNKNOWN_ARCH_TYPE: u64 = 19;
pub const UNSUPPORTED_ARCH_TYPE: u64 = 20;
pub const INVALID_OBJECT_TYPE: u64 = 21;
pub const TYPE_MISMATCH: u64 = 22;
pub const INSUFFICIENT_MEMORY: u64 = 23;
pub const POOL_EXHAUSTED: u64 = 24;
pub const INVALID_SIZE: u64 = 25;
pub const INVALID_FRAME_SIZE: u64 = 26;
pub const INVALID_KEY: u64 = 27;
pub const INCONSISTENT_KEY: u64 = 28;
pub const KEY_SLOT_EXHAUSTED: u64 = 29;
/// A mapping walk reached an absent intermediate page table.
/// Detail 1 is the faulting virtual address; detail 2 is zero.
pub const MISSING_INTERMEDIATE: u64 = 30;
/// The mapping violates the alias policy: the frame's physical extent
/// overlaps a live mapping in the target `AddressSpace`. Detail 1 is the physical
/// base of the conflicting live mapping; detail 2 is zero.
pub const PHYSICAL_ALIAS: u64 = 31;
/// An `EventCount.Advance` would exceed the counter's `u64` range
/// The counter is unchanged and every queued
/// `Await` completes with this same error. Details are zero.
pub const COUNTER_OVERFLOW: u64 = 32;
/// Invalid stack pool: its extent, stack size or stack count.
/// Detail 1 is the offending submitted value; detail 2 is an `InvalidStackReason`.
pub const INVALID_STACK: u64 = 33;
/// A Call would exceed the fixed depth-16 invocation continuation stack.
/// Detail 1 is the current saved-continuation count; detail 2 is zero.
pub const NESTING_DEPTH: u64 = 34;
/// A `Thread.Return` helper observed a local `SUCCESS` response, which a
/// completed Return never produces. Userspace-synthesized protocol/invariant
/// error, never emitted by the kernel: details are the local x1/x2 verbatim.
pub const UNEXPECTED_RETURN: u64 = 35;
/// A finite source Time holds less budget than a `Time.Transfer` asks for.
/// Detail 1 is the source's remaining budget in nanoseconds; detail 2 is zero.
pub const INSUFFICIENT_BUDGET: u64 = 36;
/// `IRQControl.Issue` named a line that already has a live `IRQHandler`, or
/// the line the nucleus keeps for its own timer. Detail 1 is the line;
/// detail 2 is zero.
pub const IRQ_LINE_ACTIVE: u64 = 37;
/// An `Invocation.CallWithKey` named an Invocation whose exporter declared no
/// key-placement window (count 0), so it accepts no lent key. Details are zero.
pub const KEY_NOT_ACCEPTED: u64 = 38;
/// Every slot of the Invocation's key-placement window is occupied or cannot
/// issue a fresh incarnation, so the lent key has nowhere to be placed.
/// Details are zero.
pub const NO_PLACEMENT_SLOT: u64 = 39;
/// The operation needs the `AddressSpace`'s keytable, and no keytable has been
/// bound to it with `AddressSpace.Bind` yet. Details are zero.
pub const NOT_BOUND: u64 = 40;
/// A `KeyTable` used for an invocation or bound by `AddressSpace.Bind` does
/// not name itself in a well-known self slot. Detail 1 is the slot; detail 2
/// is the key that led to the table (the invoked key at syscall entry, the
/// table key of `Bind`). See [`InvalidSelfReference`].
pub const INVALID_SELF_REFERENCE: u64 = 41;
/// A `Scheduler.Dispatch` named a Thread whose generation is no longer the one
/// the dispatcher decided on. Detail 1 is the Thread's current generation;
/// detail 2 is zero.
pub const STALE_GENERATION: u64 = 42;
/// A wait with a finite timeout (`Notification.Await`, `EventCount.Await`)
/// reached its deadline first. Detail 1 is the `EventCount` value or the
/// Notification bitmap at that moment (the bitmap is always zero: a wait
/// that timed out consumed nothing); detail 2 is zero.
pub const TIMED_OUT: u64 = 43;
/// A Notification is listed in a `WaitSet` and cannot be awaited directly, or
/// a Notification with a Thread waiting on it directly cannot join a `WaitSet`.
/// Details are zero.
pub const WATCHED: u64 = 44;
/// A member of a `WaitSet` is no longer live. Detail 1 is the member's slot;
/// detail 2 is zero.
pub const MEMBER_RETIRED: u64 = 45;
/// A `CopyDerive` asked to set a badge on a key that already has one. A badge
/// is set once and never changed or cleared. Details are zero.
pub const ALREADY_BADGED: u64 = 46;

/// A `Call` found no free stack in the stack pool of its Invocation. Detail 1
/// is the pool's stack count; detail 2 is zero.
pub const NO_FREE_STACK: u64 = 47;

/// Which self reference of a `KeyTable` is wrong, by the well-known slot that
/// must hold it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InvalidSelfReference {
    /// The `SELF_KEYTABLE` slot does not hold a `KeyTable` capability naming
    /// the table itself.
    KeyTableKey(KeySlot),
}

impl InvalidSelfReference {
    /// The self reference that `slot` must hold, if it is a well-known self
    /// slot.
    pub fn from_slot(slot: u64) -> Option<Self> {
        (slot == u64::from(KeySlot::SELF_KEYTABLE.0))
            .then_some(Self::KeyTableKey(KeySlot::SELF_KEYTABLE))
    }

    /// The well-known slot of this self reference.
    pub const fn slot(self) -> KeySlot {
        match self {
            Self::KeyTableKey(slot) => slot,
        }
    }
}

/// Field-specific stack pool diagnostics, carried as the complete `x2` word.
///
/// Zero is invalid. Numeric IDs do not define validation precedence, and no
/// operand index is packed into the reason. Each reports the submitted value
/// it names.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvalidStackReason {
    /// The pool's base lies outside the target user VA range; reports `base`.
    BaseOutsideUserRange = 1,
    /// The pool's exclusive end (`base + count * stack_bytes`) exceeds the
    /// permitted user extent, or the size overflows; reports that end, or the
    /// stack size when the end overflows.
    EndOutsideUserRange = 2,
    /// The pool's base is not 16-byte aligned; reports `base`.
    BaseMisaligned = 3,
    /// The stack size is not a multiple of 16; reports the size.
    StackBytesMisaligned = 4,
    /// The stack size is below 16 bytes; reports the size.
    StackBytesTooSmall = 5,
    /// The stack count is zero or above 64; reports the count.
    StackCountOutOfRange = 6,
}

impl TryFrom<u64> for InvalidStackReason {
    type Error = ();

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::BaseOutsideUserRange),
            2 => Ok(Self::EndOutsideUserRange),
            3 => Ok(Self::BaseMisaligned),
            4 => Ok(Self::StackBytesMisaligned),
            5 => Ok(Self::StackBytesTooSmall),
            6 => Ok(Self::StackCountOutOfRange),
            _ => Err(()),
        }
    }
}
