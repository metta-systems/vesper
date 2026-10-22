//! Stack pools: the stacks a call into an `AddressSpace` runs on.
//!
//! An `AddressSpace` holds a small fixed table of pools, so the nucleus
//! allocates nothing. A pool is `count` stacks of `stack_bytes` each, back to
//! back from `base` in the exporter's user range, and a mask of the free ones.
//! The kernel takes the lowest free stack for each call, starts the callee with
//! `SP` at its end, and gives the stack back when the call returns. It never
//! reads or writes the stack memory and does not prove it mapped: what is
//! mapped there, guard regions included, is the exporter's business.
//!
//! A pool is named by a [`StackPoolHandle`]: its slot and the slot's
//! generation. Destroying a pool bumps the generation, so every handle and
//! every Invocation that names it fails afterwards.

use libobject::{
    CapError, InvalidStackReason,
    stack_pool::{MAX_POOL_STACKS, MAX_STACK_POOLS, StackPoolHandle},
};

/// The generation of a slot's first pool.
const FIRST_GENERATION: u32 = 1;

#[derive(Clone, Copy)]
struct Pool {
    base: u64,
    stack_bytes: u64,
    count: u8,
    /// Bit `i` is set when stack `i` is free.
    free: u64,
}

impl Pool {
    /// The mask with every stack of the pool free.
    const fn all_free(count: u8) -> u64 {
        if count as u64 == MAX_POOL_STACKS {
            u64::MAX
        } else {
            (1_u64 << count) - 1
        }
    }
}

#[derive(Clone, Copy)]
struct Slot {
    /// The generation of the pool in the slot, or of the next one if it is
    /// empty. A slot whose generation would overflow is never used again.
    generation: u32,
    pool: Option<Pool>,
    retired: bool,
}

/// A stack taken from a pool for one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackLease {
    /// The pool's slot.
    pub slot: u8,
    /// The stack's index in the pool.
    pub index: u8,
    /// Where the callee's `SP` starts: the end of the stack.
    pub sp: u64,
}

/// The stack pools of one `AddressSpace`.
#[derive(Clone, Copy)]
pub struct StackPools {
    slots: [Slot; MAX_STACK_POOLS],
}

impl StackPools {
    /// A table with no pools.
    pub const fn new() -> Self {
        Self {
            slots: [Slot {
                generation: FIRST_GENERATION,
                pool: None,
                retired: false,
            }; MAX_STACK_POOLS],
        }
    }

    /// Make a pool of `count` stacks of `stack_bytes` from `base`, all in the
    /// user range below `user_va_end`.
    ///
    /// Checks, in order, each reporting the submitted value it names: the
    /// count (`StackCountOutOfRange`), the stack size (`StackBytesTooSmall`,
    /// `StackBytesMisaligned`), the base (`BaseMisaligned`,
    /// `BaseOutsideUserRange`) and the end (`EndOutsideUserRange`, reporting the
    /// computed end, or the stack size when the arithmetic overflows). A full
    /// table is `PoolExhausted`.
    pub fn create(
        &mut self,
        base: u64,
        stack_bytes: u64,
        count: u64,
        user_va_end: u64,
    ) -> Result<StackPoolHandle, CapError> {
        use InvalidStackReason as Reason;

        let invalid = |value, reason| CapError::InvalidStack { value, reason };
        if count == 0 || count > MAX_POOL_STACKS {
            return Err(invalid(count, Reason::StackCountOutOfRange));
        }
        if stack_bytes < 16 {
            return Err(invalid(stack_bytes, Reason::StackBytesTooSmall));
        }
        if stack_bytes & 15 != 0 {
            return Err(invalid(stack_bytes, Reason::StackBytesMisaligned));
        }
        if base & 15 != 0 {
            return Err(invalid(base, Reason::BaseMisaligned));
        }
        if base >= user_va_end {
            return Err(invalid(base, Reason::BaseOutsideUserRange));
        }
        let end = stack_bytes
            .checked_mul(count)
            .and_then(|length| base.checked_add(length))
            .ok_or_else(|| invalid(stack_bytes, Reason::EndOutsideUserRange))?;
        if end > user_va_end {
            return Err(invalid(end, Reason::EndOutsideUserRange));
        }
        // `count` is at most 64 and fits a byte.
        let count = u8::try_from(count).map_err(|_too_many| CapError::InvalidOperation)?;
        let (index, slot) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.pool.is_none() && !slot.retired)
            .ok_or(CapError::PoolExhausted)?;
        slot.pool = Some(Pool {
            base,
            stack_bytes,
            count,
            free: Pool::all_free(count),
        });
        let index = u8::try_from(index).map_err(|_too_many| CapError::InvalidOperation)?;
        Ok(StackPoolHandle::new(index, slot.generation))
    }

    /// The pool `handle` names, or `StaleGeneration` (reporting the slot's
    /// current generation) when it names none.
    fn pool(&self, handle: StackPoolHandle) -> Result<&Pool, CapError> {
        let stale = |generation| CapError::StaleGeneration {
            current: u64::from(generation),
        };
        let slot = self.slots.get(usize::from(handle.slot())).ok_or(stale(0))?;
        match &slot.pool {
            Some(pool) if slot.generation == handle.generation() => Ok(pool),
            _ => Err(stale(slot.generation)),
        }
    }

    /// Destroy the pool `handle` names. Fails with `InvalidOperation` while any
    /// of its stacks is in use; afterwards the handle and every Invocation that
    /// names the pool fail with `StaleGeneration`.
    pub fn destroy(&mut self, handle: StackPoolHandle) -> Result<(), CapError> {
        let pool = *self.pool(handle)?;
        if pool.free != Pool::all_free(pool.count) {
            return Err(CapError::InvalidOperation);
        }
        let slot = &mut self.slots[usize::from(handle.slot())];
        slot.pool = None;
        match slot.generation.checked_add(1) {
            Some(next) => slot.generation = next,
            None => slot.retired = true,
        }
        Ok(())
    }

    /// Whether a call through an Invocation that names `handle` would find a
    /// stack: the pool is live (`StaleGeneration` otherwise) and has a free
    /// stack (`NoFreeStack` otherwise). Changes nothing.
    pub fn admit(&self, handle: StackPoolHandle) -> Result<(), CapError> {
        let pool = self.pool(handle)?;
        if pool.free == 0 {
            return Err(CapError::NoFreeStack {
                count: u64::from(pool.count),
            });
        }
        Ok(())
    }

    /// Take the lowest free stack of the pool `handle` names.
    pub fn take(&mut self, handle: StackPoolHandle) -> Result<StackLease, CapError> {
        let pool = *self.pool(handle)?;
        if pool.free == 0 {
            return Err(CapError::NoFreeStack {
                count: u64::from(pool.count),
            });
        }
        let index = pool.free.trailing_zeros();
        let slot = &mut self.slots[usize::from(handle.slot())];
        if let Some(pool) = &mut slot.pool {
            pool.free &= !(1_u64 << index);
        }
        // The index is below 64 and the sum below the validated end.
        let end = pool.base + (u64::from(index) + 1) * pool.stack_bytes;
        Ok(StackLease {
            slot: handle.slot(),
            index: u8::try_from(index).map_err(|_too_many| CapError::InvalidOperation)?,
            sp: end,
        })
    }

    /// Give back the stack `index` of the pool in `slot`. Returns whether it was
    /// in use: a stack that is free already, or an index or slot that names
    /// nothing, changes nothing.
    pub fn give(&mut self, slot: u8, index: u8) -> bool {
        let Some(Slot {
            pool: Some(pool), ..
        }) = self.slots.get_mut(usize::from(slot))
        else {
            return false;
        };
        if index >= pool.count || pool.free & (1_u64 << index) != 0 {
            return false;
        }
        pool.free |= 1_u64 << index;
        true
    }

    /// How many stacks of the pool `handle` names are in use.
    pub fn in_use(&self, handle: StackPoolHandle) -> Result<u32, CapError> {
        let pool = self.pool(handle)?;
        Ok(u32::from(pool.count) - pool.free.count_ones())
    }
}

impl Default for StackPools {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER_END: u64 = 1 << 48;

    /// The value of a result that must have succeeded.
    fn ok<T>(result: Result<T, CapError>) -> T {
        result.unwrap_or_else(|error| panic!("unexpected failure: {:?}", error.code()))
    }

    fn pools_with_two_stacks() -> (StackPools, StackPoolHandle) {
        let mut pools = StackPools::new();
        let handle = ok(pools.create(0x2000_0000, 0x2000, 2, USER_END));
        (pools, handle)
    }

    #[test_case]
    fn a_pool_hands_out_the_lowest_free_stack_ending_at_its_top() {
        let (mut pools, handle) = pools_with_two_stacks();
        let first = ok(pools.take(handle));
        assert_eq!((first.index, first.sp), (0, 0x2000_2000));
        let second = ok(pools.take(handle));
        assert_eq!((second.index, second.sp), (1, 0x2000_4000));
        assert_eq!(ok(pools.in_use(handle)), 2);
        // Giving the first back makes it the next one.
        assert!(pools.give(first.slot, first.index));
        assert_eq!(ok(pools.take(handle)).index, 0);
    }

    #[test_case]
    fn an_exhausted_pool_reports_its_size_and_changes_nothing() {
        let (mut pools, handle) = pools_with_two_stacks();
        ok(pools.take(handle));
        ok(pools.take(handle));
        for _ in 0..2 {
            assert!(matches!(
                pools.admit(handle),
                Err(CapError::NoFreeStack { count: 2 })
            ));
            assert!(matches!(
                pools.take(handle),
                Err(CapError::NoFreeStack { count: 2 })
            ));
        }
        assert_eq!(ok(pools.in_use(handle)), 2);
    }

    #[test_case]
    fn giving_back_twice_or_a_stranger_changes_nothing() {
        let (mut pools, handle) = pools_with_two_stacks();
        let lease = ok(pools.take(handle));
        assert!(pools.give(lease.slot, lease.index));
        assert!(!pools.give(lease.slot, lease.index), "already free");
        assert!(!pools.give(lease.slot, 2), "no such stack");
        assert!(!pools.give(1, 0), "no such pool");
        assert!(!pools.give(u8::MAX, 0), "no such slot");
        assert_eq!(ok(pools.in_use(handle)), 0);
    }

    #[test_case]
    fn a_pool_of_sixty_four_uses_the_whole_mask() {
        let mut pools = StackPools::new();
        let handle = ok(pools.create(0x1000, 0x10, 64, USER_END));
        for index in 0..64 {
            let lease = ok(pools.take(handle));
            assert_eq!(lease.index, index);
            assert_eq!(lease.sp, 0x1000 + (u64::from(index) + 1) * 0x10);
        }
        assert!(matches!(
            pools.take(handle),
            Err(CapError::NoFreeStack { count: 64 })
        ));
        for index in 0..64 {
            assert!(pools.give(handle.slot(), index));
        }
        assert_eq!(ok(pools.in_use(handle)), 0);
        assert!(pools.destroy(handle).is_ok());
    }

    #[test_case]
    fn destroying_needs_every_stack_back_and_stales_the_handle() {
        let (mut pools, handle) = pools_with_two_stacks();
        let lease = ok(pools.take(handle));
        assert!(matches!(
            pools.destroy(handle),
            Err(CapError::InvalidOperation)
        ));
        assert!(pools.give(lease.slot, lease.index));
        assert!(pools.destroy(handle).is_ok());
        // The handle is stale for every use, and the slot's next pool is a new one.
        assert!(matches!(
            pools.admit(handle),
            Err(CapError::StaleGeneration { current: 2 })
        ));
        assert!(matches!(
            pools.take(handle),
            Err(CapError::StaleGeneration { current: 2 })
        ));
        assert!(matches!(
            pools.destroy(handle),
            Err(CapError::StaleGeneration { current: 2 })
        ));
        let again = ok(pools.create(0x2000_0000, 0x2000, 2, USER_END));
        assert_eq!(again.slot(), handle.slot());
        assert_eq!(again.generation(), 2);
        assert!(pools.admit(again).is_ok());
        assert!(pools.admit(handle).is_err());
    }

    #[test_case]
    fn a_handle_to_a_slot_beyond_the_table_is_stale() {
        let pools = StackPools::new();
        for slot in [u8::try_from(MAX_STACK_POOLS).unwrap(), u8::MAX] {
            assert!(matches!(
                pools.admit(StackPoolHandle::new(slot, 1)),
                Err(CapError::StaleGeneration { current: 0 })
            ));
        }
    }

    #[test_case]
    fn the_table_holds_sixteen_pools_and_reuses_destroyed_slots() {
        let mut pools = StackPools::new();
        let mut handles = [None; MAX_STACK_POOLS];
        for (index, handle) in handles.iter_mut().enumerate() {
            let base = 0x1_0000 + index as u64 * 0x1000;
            *handle = Some(ok(pools.create(base, 0x100, 4, USER_END)));
        }
        assert!(matches!(
            pools.create(0x100_0000, 0x100, 4, USER_END),
            Err(CapError::PoolExhausted)
        ));
        let freed = handles[3]
            .take()
            .unwrap_or_else(|| panic!("slot 3 was filled"));
        ok(pools.destroy(freed));
        let reused = ok(pools.create(0x100_0000, 0x100, 4, USER_END));
        assert_eq!(reused.slot(), freed.slot());
        assert_ne!(reused.generation(), freed.generation());
    }

    #[test_case]
    fn a_slot_whose_generation_would_overflow_is_never_used_again() {
        let mut pools = StackPools::new();
        pools.slots[0].generation = u32::MAX;
        let handle = ok(pools.create(0x1000, 0x100, 1, USER_END));
        assert_eq!((handle.slot(), handle.generation()), (0, u32::MAX));
        ok(pools.destroy(handle));
        let next = ok(pools.create(0x1000, 0x100, 1, USER_END));
        assert_eq!(next.slot(), 1, "slot 0 is retired");
    }

    fn reason(result: Result<StackPoolHandle, CapError>) -> (u64, InvalidStackReason) {
        match result {
            Err(CapError::InvalidStack { value, reason }) => (value, reason),
            _ => panic!("expected InvalidStack"),
        }
    }

    #[test_case]
    fn creating_checks_count_size_base_and_end_in_that_order() {
        use InvalidStackReason as Reason;

        let mut pools = StackPools::new();
        let mut create = |base, bytes, count| pools.create(base, bytes, count, USER_END);
        assert_eq!(
            reason(create(0x1000, 0x100, 0)),
            (0, Reason::StackCountOutOfRange)
        );
        assert_eq!(
            reason(create(0x1000, 0x100, 65)),
            (65, Reason::StackCountOutOfRange)
        );
        assert_eq!(
            reason(create(0x1000, 0, 1)),
            (0, Reason::StackBytesTooSmall)
        );
        assert_eq!(
            reason(create(0x1000, 8, 1)),
            (8, Reason::StackBytesTooSmall)
        );
        assert_eq!(
            reason(create(0x1000, 0x108, 1)),
            (0x108, Reason::StackBytesMisaligned)
        );
        assert_eq!(
            reason(create(0x1008, 0x100, 1)),
            (0x1008, Reason::BaseMisaligned)
        );
        assert_eq!(
            reason(create(USER_END, 0x100, 1)),
            (USER_END, Reason::BaseOutsideUserRange)
        );
        assert_eq!(
            reason(create(u64::MAX - 15, 0x100, 1)),
            (u64::MAX - 15, Reason::BaseOutsideUserRange)
        );
        // The end is computed: it may equal the range end but not pass it.
        assert!(create(USER_END - 0x400, 0x100, 4).is_ok());
        assert_eq!(
            reason(create(USER_END - 0x400, 0x100, 5)),
            (USER_END + 0x100, Reason::EndOutsideUserRange)
        );
        // An overflowing size reports the stack size.
        assert_eq!(
            reason(create(0x1000, 1 << 62, 8)),
            (1 << 62, Reason::EndOutsideUserRange)
        );
        // Every failure left the table empty.
        let mut count = 0;
        while pools.create(0x1000, 0x100, 1, USER_END).is_ok() {
            count += 1;
        }
        assert_eq!(count, MAX_STACK_POOLS - 1, "one pool was made above");
    }
}
