//! Stack pools: the stacks a call into an `AddressSpace` runs on.
//!
//! A pool belongs to the target `AddressSpace` and is made with
//! `AddressSpace.CreateStackPool`. It is `count` stacks of `stack_bytes` each,
//! back to back from a base in the exporter's user range, and the kernel hands
//! one to each call and takes it back at `Thread.Return`. The kernel never
//! touches the stack memory; what is mapped there, guard regions included, is
//! the exporter's business.

/// Most stack pools one `AddressSpace` holds.
pub const MAX_STACK_POOLS: usize = 16;

/// Most stacks one pool holds: the width of its free mask.
pub const MAX_POOL_STACKS: u64 = 64;

/// Names a stack pool of one `AddressSpace`: the pool's slot in the table and
/// the slot's generation, which changes when the pool is destroyed so a stale
/// handle names nothing.
///
/// A handle is a number, not authority: every operation on a pool needs
/// `GRANT` on the `AddressSpace` key.
///
/// Wire form: the slot in bits 7:0, the generation in bits 39:8.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StackPoolHandle {
    slot: u8,
    generation: u32,
}

impl StackPoolHandle {
    /// The handle of the pool in `slot` at `generation`.
    pub const fn new(slot: u8, generation: u32) -> Self {
        Self { slot, generation }
    }

    /// Decode a wire word; `None` if a reserved bit is set.
    pub const fn from_wire(word: u64) -> Option<Self> {
        if word >> 40 != 0 {
            return None;
        }
        // The two fields are exactly the masked and shifted halves.
        #[allow(clippy::cast_possible_truncation)]
        Some(Self::new(word as u8, (word >> 8) as u32))
    }

    /// The wire word.
    pub const fn to_wire(self) -> u64 {
        (self.slot as u64) | ((self.generation as u64) << 8)
    }

    /// The pool's slot in its `AddressSpace`'s table.
    pub const fn slot(self) -> u8 {
        self.slot
    }

    /// The slot's generation when the pool was made.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}
