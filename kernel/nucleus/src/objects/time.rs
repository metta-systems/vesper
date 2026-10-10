//! `Time` kernel object: a CPU budget on one core plus a priority chain, after
//! Composite's temporal capabilities.
//!
//! The chain holds one (scheduler, priority) entry for each scheduler the
//! budget passed through, in the order the budget arrived; the last entry is
//! the scheduler that most recently gave this Time budget. A scheduler is
//! named by the incarnation-checked identity of the Time its budget came
//! from. A lower priority number is more urgent.
//!
//! Budget only moves: [`Time::transfer`] and [`Time::merge`] compute both new
//! states before the caller commits either, so every failure leaves both
//! Times unchanged. Nothing charges a Time yet.

use {
    crate::objects::{
        NucleusObject,
        access::{ObjectId, PoolTag},
    },
    libobject::{
        CapError, ObjectType,
        time::{Budget, INFINITE_BUDGET, LOWEST_PRIORITY, MAX_PRIORITY_CHAIN, Priority},
    },
};

/// One scheduler's priority for a Time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainEntry {
    /// The Time the scheduler gave budget from.
    pub scheduler: ObjectId,
    pub priority: Priority,
}

/// Bounded priority chain, at most [`MAX_PRIORITY_CHAIN`] entries with one
/// entry per scheduler.
#[derive(Clone, Copy, Debug)]
pub struct PriorityChain {
    entries: [ChainEntry; MAX_PRIORITY_CHAIN],
    len: usize,
}

impl PriorityChain {
    pub const fn new() -> Self {
        Self {
            entries: [ChainEntry {
                scheduler: ObjectId {
                    pool: PoolTag::Region,
                    index: 0,
                    generation: 0,
                },
                priority: LOWEST_PRIORITY,
            }; MAX_PRIORITY_CHAIN],
            len: 0,
        }
    }

    /// The entries in arrival order.
    pub fn entries(&self) -> &[ChainEntry] {
        &self.entries[..self.len]
    }

    /// Priority at the last entry, or [`LOWEST_PRIORITY`] when empty.
    pub fn last_priority(&self) -> Priority {
        self.entries()
            .last()
            .map_or(LOWEST_PRIORITY, |entry| entry.priority)
    }

    fn position(&self, scheduler: ObjectId) -> Option<usize> {
        self.entries()
            .iter()
            .position(|entry| entry.scheduler == scheduler)
    }

    fn push(&mut self, entry: ChainEntry) -> Result<(), CapError> {
        if self.len == MAX_PRIORITY_CHAIN {
            return Err(CapError::PriorityChainFull);
        }
        self.entries[self.len] = entry;
        self.len += 1;
        Ok(())
    }

    /// This chain merged with `other`: one entry per scheduler, and a
    /// scheduler in both keeps the numerically larger (less urgent)
    /// priority, so merging never raises a priority. Entries only in
    /// `other` are appended in their order.
    fn merged(&self, other: &Self) -> Result<Self, CapError> {
        let mut result = *self;
        for entry in other.entries() {
            match result.position(entry.scheduler) {
                Some(index) => {
                    let kept = &mut result.entries[index].priority;
                    *kept = (*kept).max(entry.priority);
                }
                None => result.push(*entry)?,
            }
        }
        Ok(result)
    }

    /// This chain with `scheduler` at `priority` as its last entry, moving an
    /// existing entry for that scheduler to the end.
    fn with_last(&self, scheduler: ObjectId, priority: Priority) -> Result<Self, CapError> {
        let mut result = *self;
        if let Some(index) = result.position(scheduler) {
            result.entries.copy_within(index + 1..result.len, index);
            result.len -= 1;
        }
        result.push(ChainEntry {
            scheduler,
            priority,
        })?;
        Ok(result)
    }
}

impl Default for PriorityChain {
    fn default() -> Self {
        Self::new()
    }
}

/// A CPU budget on one core with its priority chain.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    core: usize,
    budget_ns: Budget,
    chain: PriorityChain,
}

impl Time {
    /// An empty Time on `core`: zero budget and an empty chain. This is what
    /// `Untyped.Retype` creates.
    pub const fn empty(core: usize) -> Self {
        Self {
            core,
            budget_ns: 0,
            chain: PriorityChain::new(),
        }
    }

    /// The root Time: `budget_ns` on `core`, with a chain naming itself
    /// (`own_id`) at `priority`. Only the boot code creates one.
    pub fn root(core: usize, own_id: ObjectId, budget_ns: Budget, priority: Priority) -> Self {
        let mut chain = PriorityChain::new();
        chain.entries[0] = ChainEntry {
            scheduler: own_id,
            priority,
        };
        chain.len = 1;
        Self {
            core,
            budget_ns,
            chain,
        }
    }

    pub fn core(&self) -> usize {
        self.core
    }

    /// Remaining budget in nanoseconds; [`INFINITE_BUDGET`] when infinite.
    pub fn budget_ns(&self) -> Budget {
        self.budget_ns
    }

    pub fn chain(&self) -> &PriorityChain {
        &self.chain
    }

    /// Priority at the most recent scheduler, [`LOWEST_PRIORITY`] when the
    /// chain is empty.
    pub fn priority(&self) -> Priority {
        self.chain.last_priority()
    }

    /// Move `amount` from `source` (identity `source_id`) into `destination`,
    /// which records `priority` for the source's scheduler. Returns the new
    /// (source, destination) states; the caller commits both or neither.
    pub fn transfer(
        source: &Self,
        source_id: ObjectId,
        destination: &Self,
        amount: Budget,
        priority: Priority,
    ) -> Result<(Self, Self), CapError> {
        if amount == 0 || source.core != destination.core {
            return Err(CapError::InvalidOperation);
        }
        let source_infinite = source.budget_ns == INFINITE_BUDGET;
        if !source_infinite && source.budget_ns < amount {
            return Err(CapError::InsufficientBudget {
                remaining: source.budget_ns,
            });
        }
        let destination_budget = Self::sum(destination.budget_ns, amount)?;
        let chain = destination
            .chain
            .merged(&source.chain)?
            .with_last(source_id, priority)?;

        let mut new_source = *source;
        if !source_infinite {
            new_source.budget_ns -= amount;
        }
        let new_destination = Self {
            core: destination.core,
            budget_ns: destination_budget,
            chain,
        };
        Ok((new_source, new_destination))
    }

    /// Move all of `source` into `destination`. The destination keeps its
    /// own chain, so the merged budget runs at the destination's priority; a
    /// destination with an empty chain takes the source's chain instead, so
    /// budget never ends up without a priority. Returns the new
    /// (destination, source) states; the source becomes empty.
    pub fn merge(destination: &Self, source: &Self) -> Result<(Self, Self), CapError> {
        if source.core != destination.core {
            return Err(CapError::InvalidOperation);
        }
        let budget_ns = Self::sum(destination.budget_ns, source.budget_ns)?;
        let chain = if destination.chain.entries().is_empty() {
            source.chain
        } else {
            destination.chain
        };
        Ok((
            Self {
                core: destination.core,
                budget_ns,
                chain,
            },
            Self::empty(source.core),
        ))
    }

    /// Budget after adding `amount` to `budget`: infinite when either is,
    /// otherwise a finite sum that must stay below [`INFINITE_BUDGET`].
    fn sum(budget: Budget, amount: Budget) -> Result<Budget, CapError> {
        if budget == INFINITE_BUDGET || amount == INFINITE_BUDGET {
            return Ok(INFINITE_BUDGET);
        }
        budget
            .checked_add(amount)
            .filter(|total| *total != INFINITE_BUDGET)
            .ok_or(CapError::InvalidOperation)
    }
}

impl NucleusObject for Time {
    const TYPE: ObjectType = ObjectType::TIME;
    const POOL: PoolTag = PoolTag::Time;
}

#[cfg(test)]
mod tests {
    use {
        super::{ChainEntry, Time},
        crate::objects::access::{ObjectId, PoolTag},
        libobject::{
            CapError, syscall_status,
            time::{INFINITE_BUDGET, LOWEST_PRIORITY, MAX_PRIORITY_CHAIN},
        },
    };

    fn time_id(index: u16) -> ObjectId {
        ObjectId {
            pool: PoolTag::Time,
            index,
            generation: 1,
        }
    }

    fn entry(index: u16, priority: u16) -> ChainEntry {
        ChainEntry {
            scheduler: time_id(index),
            priority,
        }
    }

    fn error_code<T>(result: Result<T, CapError>) -> (u64, u64, u64) {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(error) => error.code(),
        }
    }

    #[test_case]
    fn empty_time_has_no_budget_and_reports_lowest_priority() {
        let time = Time::empty(0);
        assert_eq!(time.budget_ns(), 0);
        assert!(time.chain().entries().is_empty());
        assert_eq!(time.priority(), LOWEST_PRIORITY);
    }

    #[test_case]
    fn transfer_from_finite_source_conserves_budget_and_records_priority() {
        let root = Time::root(0, time_id(0), 1_000, 0);
        let (source, destination) = Time::transfer(&root, time_id(0), &Time::empty(0), 400, 0x8000)
            .ok()
            .unwrap();
        assert_eq!(source.budget_ns(), 600);
        assert_eq!(destination.budget_ns(), 400);
        assert_eq!(destination.chain().entries(), &[entry(0, 0x8000)]);
        assert_eq!(destination.priority(), 0x8000);
        // The source's own chain is unchanged.
        assert_eq!(source.chain().entries(), &[entry(0, 0)]);
    }

    #[test_case]
    fn transfer_from_infinite_source_keeps_it_infinite() {
        let root = Time::root(0, time_id(0), INFINITE_BUDGET, 0);
        let (source, destination) = Time::transfer(&root, time_id(0), &Time::empty(0), 5, 1)
            .ok()
            .unwrap();
        assert_eq!(source.budget_ns(), INFINITE_BUDGET);
        assert_eq!(destination.budget_ns(), 5);

        let (_, infinite) = Time::transfer(&root, time_id(0), &Time::empty(0), INFINITE_BUDGET, 1)
            .ok()
            .unwrap();
        assert_eq!(infinite.budget_ns(), INFINITE_BUDGET);
    }

    #[test_case]
    fn transfer_more_than_a_finite_source_holds_reports_its_remaining_budget() {
        let root = Time::root(0, time_id(0), 10, 0);
        assert_eq!(
            error_code(Time::transfer(&root, time_id(0), &Time::empty(0), 11, 1)),
            (syscall_status::INSUFFICIENT_BUDGET, 10, 0)
        );
        // An infinite amount needs an infinite source.
        assert_eq!(
            error_code(Time::transfer(
                &root,
                time_id(0),
                &Time::empty(0),
                INFINITE_BUDGET,
                1
            )),
            (syscall_status::INSUFFICIENT_BUDGET, 10, 0)
        );
    }

    #[test_case]
    fn transfer_rejects_zero_amount_other_core_and_overflowing_sum() {
        let root = Time::root(0, time_id(0), INFINITE_BUDGET, 0);
        let invalid = (syscall_status::INVALID_OPERATION, 0, 0);
        assert_eq!(
            error_code(Time::transfer(&root, time_id(0), &Time::empty(0), 0, 1)),
            invalid
        );
        assert_eq!(
            error_code(Time::transfer(&root, time_id(0), &Time::empty(1), 1, 1)),
            invalid
        );
        let (_, nearly_full) =
            Time::transfer(&root, time_id(0), &Time::empty(0), INFINITE_BUDGET - 1, 1)
                .ok()
                .unwrap();
        assert_eq!(
            error_code(Time::transfer(&root, time_id(0), &nearly_full, 1, 1)),
            invalid
        );
    }

    #[test_case]
    fn nested_transfer_extends_the_chain_and_merging_never_raises_priority() {
        let root = Time::root(0, time_id(0), 1_000, 0);
        let (root, child) = Time::transfer(&root, time_id(0), &Time::empty(0), 500, 0x8000)
            .ok()
            .unwrap();
        let (_, grandchild) = Time::transfer(&child, time_id(1), &Time::empty(0), 100, 0x10)
            .ok()
            .unwrap();
        assert_eq!(
            grandchild.chain().entries(),
            &[entry(0, 0x8000), entry(1, 0x10)]
        );

        // A second transfer from the root at a more urgent priority sets the
        // root's entry and moves it last; the child's entry keeps its value.
        let (_, regranted) = Time::transfer(&root, time_id(0), &grandchild, 1, 0x100)
            .ok()
            .unwrap();
        assert_eq!(
            regranted.chain().entries(),
            &[entry(1, 0x10), entry(0, 0x100)]
        );
        assert_eq!(regranted.priority(), 0x100);
    }

    #[test_case]
    fn merge_moves_everything_and_empties_the_source() {
        let root = Time::root(0, time_id(0), 1_000, 0);
        let (_, first) = Time::transfer(&root, time_id(0), &Time::empty(0), 300, 0x9000)
            .ok()
            .unwrap();
        let (_, second) = Time::transfer(&root, time_id(0), &Time::empty(0), 200, 0x8000)
            .ok()
            .unwrap();
        // The destination keeps its own chain: the merged budget runs at
        // its priority.
        let (merged, emptied) = Time::merge(&second, &first).ok().unwrap();
        assert_eq!(merged.budget_ns(), 500);
        assert_eq!(merged.chain().entries(), &[entry(0, 0x8000)]);
        assert_eq!(emptied.budget_ns(), 0);
        assert!(emptied.chain().entries().is_empty());
    }

    #[test_case]
    fn merging_back_keeps_the_parent_priority() {
        let root = Time::root(0, time_id(0), 1_000, 0);
        let (root, child) = Time::transfer(&root, time_id(0), &Time::empty(0), 500, 0x8000)
            .ok()
            .unwrap();
        let (child, grandchild) = Time::transfer(&child, time_id(1), &Time::empty(0), 100, 0x8001)
            .ok()
            .unwrap();

        // Grandchild back into the root, two levels up.
        let (merged, emptied) = Time::merge(&root, &grandchild).ok().unwrap();
        assert_eq!(merged.budget_ns(), 600);
        assert_eq!(merged.chain().entries(), &[entry(0, 0)]);
        assert_eq!(merged.priority(), 0);
        assert!(emptied.chain().entries().is_empty());

        // Child back into the root, one level up.
        let (merged, _) = Time::merge(&merged, &child).ok().unwrap();
        assert_eq!(merged.budget_ns(), 1_000);
        assert_eq!(merged.chain().entries(), &[entry(0, 0)]);
    }

    #[test_case]
    fn merge_of_an_infinite_source_makes_the_destination_infinite() {
        let root = Time::root(0, time_id(0), INFINITE_BUDGET, 0);
        let (merged, emptied) = Time::merge(&Time::empty(0), &root).ok().unwrap();
        assert_eq!(merged.budget_ns(), INFINITE_BUDGET);
        assert_eq!(merged.chain().entries(), &[entry(0, 0)]);
        assert_eq!(emptied.budget_ns(), 0);
        assert_eq!(
            error_code(Time::merge(&Time::empty(1), &root)),
            (syscall_status::INVALID_OPERATION, 0, 0)
        );
    }

    #[test_case]
    fn chain_beyond_sixteen_entries_is_rejected() {
        let mut time = Time::root(0, time_id(0), INFINITE_BUDGET, 0);
        let mut id = 0_u16;
        // Each hop from a new scheduler identity adds one entry (the first
        // hop names the root again and only rewrites its entry).
        while time.chain().entries().len() < MAX_PRIORITY_CHAIN {
            let (_, next) = Time::transfer(&time, time_id(id), &Time::empty(0), 1_000, 1)
                .ok()
                .unwrap();
            id += 1;
            time = next;
        }
        assert_eq!(
            error_code(Time::transfer(&time, time_id(id), &Time::empty(0), 1, 1)),
            (syscall_status::PRIORITY_CHAIN_FULL, 0, 0)
        );
        // Re-granting from a scheduler already in the chain still fits.
        assert!(Time::transfer(&time, time_id(0), &Time::empty(0), 1, 1).is_ok());
    }
}
