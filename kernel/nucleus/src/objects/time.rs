//! `Time` kernel object: a CPU budget on one core.
//!
//! A Time carries no priority, period or deadline; scheduling policy lives in
//! userspace schedulers. Budget only moves: [`Time::transfer`] and
//! [`Time::merge`] compute both new states before the caller commits either,
//! so every failure leaves both Times unchanged. Nothing charges a Time yet.

use {
    crate::objects::{
        NucleusObject, Thread,
        carved::{Carved, CarvedId, CarvedObject},
    },
    libobject::{
        CapError, ObjectType,
        time::{Budget, INFINITE_BUDGET},
    },
};

/// A CPU budget on one core.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    core: usize,
    budget_ns: Budget,
    /// The Thread this Time is bound to by `Scheduler.ConfigureThread`, if
    /// any. While that Thread is live the Time is its own: no budget leaves
    /// it and no other Thread can be bound to it.
    bound: Option<CarvedId>,
}

impl Time {
    /// An empty Time on `core`: zero budget. This is what `Untyped.Retype`
    /// creates.
    pub const fn empty(core: usize) -> Self {
        Self {
            core,
            budget_ns: 0,
            bound: None,
        }
    }

    /// The root Time: `budget_ns` on `core`. Only the boot code creates one.
    pub const fn root(core: usize, budget_ns: Budget) -> Self {
        Self {
            core,
            budget_ns,
            bound: None,
        }
    }

    pub fn core(&self) -> usize {
        self.core
    }

    /// Remaining budget in nanoseconds; [`INFINITE_BUDGET`] when infinite.
    pub fn budget_ns(&self) -> Budget {
        self.budget_ns
    }

    /// Whether this Time is bound to a Thread that is still live. A binding
    /// to a retired Thread no longer counts.
    pub fn is_bound(&self) -> bool {
        self.bound.is_some_and(|thread| {
            // SAFETY: the recorded identity was a live carved Thread when it
            // was bound, and carved memory is never freed; validation checks
            // its header before anything else.
            unsafe { Carved::<Thread>::validate(thread) }.is_ok()
        })
    }

    /// Bind this Time to `thread`. The caller checked [`Self::is_bound`].
    pub fn bind(&mut self, thread: CarvedId) {
        self.bound = Some(thread);
    }

    /// Move `amount` from `source` into `destination`. Returns the new
    /// (source, destination) states; the caller commits both or neither.
    pub fn transfer(
        source: &Self,
        destination: &Self,
        amount: Budget,
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

        let mut new_source = *source;
        if !source_infinite {
            new_source.budget_ns -= amount;
        }
        let new_destination = Self {
            core: destination.core,
            budget_ns: destination_budget,
            bound: destination.bound,
        };
        Ok((new_source, new_destination))
    }

    /// Move all of `source` into `destination`. Returns the new
    /// (destination, source) states; the source becomes empty.
    pub fn merge(destination: &Self, source: &Self) -> Result<(Self, Self), CapError> {
        if source.core != destination.core {
            return Err(CapError::InvalidOperation);
        }
        let budget_ns = Self::sum(destination.budget_ns, source.budget_ns)?;
        Ok((
            Self {
                core: destination.core,
                budget_ns,
                bound: destination.bound,
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
}

impl CarvedObject for Time {}

#[cfg(test)]
mod tests {
    use {
        super::Time,
        libobject::{CapError, syscall_status, time::INFINITE_BUDGET},
    };

    fn error_code<T>(result: Result<T, CapError>) -> (u64, u64, u64) {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(error) => error.code(),
        }
    }

    #[test_case]
    fn empty_time_has_no_budget() {
        assert_eq!(Time::empty(0).budget_ns(), 0);
    }

    #[test_case]
    fn transfer_from_finite_source_conserves_budget() {
        let root = Time::root(0, 1_000);
        let (source, destination) = Time::transfer(&root, &Time::empty(0), 400).ok().unwrap();
        assert_eq!(source.budget_ns(), 600);
        assert_eq!(destination.budget_ns(), 400);
        assert_eq!(source.budget_ns() + destination.budget_ns(), 1_000);
    }

    #[test_case]
    fn transfer_from_infinite_source_keeps_it_infinite() {
        let root = Time::root(0, INFINITE_BUDGET);
        let (source, destination) = Time::transfer(&root, &Time::empty(0), 5).ok().unwrap();
        assert_eq!(source.budget_ns(), INFINITE_BUDGET);
        assert_eq!(destination.budget_ns(), 5);

        let (_, infinite) = Time::transfer(&root, &Time::empty(0), INFINITE_BUDGET)
            .ok()
            .unwrap();
        assert_eq!(infinite.budget_ns(), INFINITE_BUDGET);
    }

    #[test_case]
    fn transfer_more_than_a_finite_source_holds_reports_its_remaining_budget() {
        let root = Time::root(0, 10);
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(0), 11)),
            (syscall_status::INSUFFICIENT_BUDGET, 10, 0)
        );
        // An infinite amount needs an infinite source.
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(0), INFINITE_BUDGET)),
            (syscall_status::INSUFFICIENT_BUDGET, 10, 0)
        );
    }

    #[test_case]
    fn transfer_rejects_zero_amount_other_core_and_overflowing_sum() {
        let root = Time::root(0, INFINITE_BUDGET);
        let invalid = (syscall_status::INVALID_OPERATION, 0, 0);
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(0), 0)),
            invalid
        );
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(1), 1)),
            invalid
        );
        let (_, nearly_full) = Time::transfer(&root, &Time::empty(0), INFINITE_BUDGET - 1)
            .ok()
            .unwrap();
        assert_eq!(error_code(Time::transfer(&root, &nearly_full, 1)), invalid);
    }

    #[test_case]
    fn failed_transfer_leaves_both_times_to_the_caller_unchanged() {
        // The functions are pure: a failure returns no new states, so the
        // caller's Times are the unchanged originals.
        let root = Time::root(0, 10);
        let destination = Time::empty(0);
        assert!(Time::transfer(&root, &destination, 11).is_err());
        assert_eq!(root.budget_ns(), 10);
        assert_eq!(destination.budget_ns(), 0);
    }

    #[test_case]
    fn nested_transfers_conserve_the_total() {
        let root = Time::root(0, 1_000);
        let (root, child) = Time::transfer(&root, &Time::empty(0), 500).ok().unwrap();
        let (child, grandchild) = Time::transfer(&child, &Time::empty(0), 100).ok().unwrap();
        assert_eq!(
            root.budget_ns() + child.budget_ns() + grandchild.budget_ns(),
            1_000
        );
    }

    #[test_case]
    fn merge_moves_everything_and_empties_the_source() {
        let root = Time::root(0, 1_000);
        let (_, first) = Time::transfer(&root, &Time::empty(0), 300).ok().unwrap();
        let (_, second) = Time::transfer(&root, &Time::empty(0), 200).ok().unwrap();
        let (merged, emptied) = Time::merge(&second, &first).ok().unwrap();
        assert_eq!(merged.budget_ns(), 500);
        assert_eq!(emptied.budget_ns(), 0);
    }

    #[test_case]
    fn merging_back_restores_the_parent_budget() {
        let root = Time::root(0, 1_000);
        let (root, child) = Time::transfer(&root, &Time::empty(0), 500).ok().unwrap();
        let (child, grandchild) = Time::transfer(&child, &Time::empty(0), 100).ok().unwrap();

        // Grandchild back into the root, two levels up.
        let (merged, emptied) = Time::merge(&root, &grandchild).ok().unwrap();
        assert_eq!(merged.budget_ns(), 600);
        assert_eq!(emptied.budget_ns(), 0);

        // Child back into the root, one level up.
        let (merged, _) = Time::merge(&merged, &child).ok().unwrap();
        assert_eq!(merged.budget_ns(), 1_000);
    }

    #[test_case]
    fn merge_of_an_infinite_source_makes_the_destination_infinite() {
        let root = Time::root(0, INFINITE_BUDGET);
        let (merged, emptied) = Time::merge(&Time::empty(0), &root).ok().unwrap();
        assert_eq!(merged.budget_ns(), INFINITE_BUDGET);
        assert_eq!(emptied.budget_ns(), 0);
        assert_eq!(
            error_code(Time::merge(&Time::empty(1), &root)),
            (syscall_status::INVALID_OPERATION, 0, 0)
        );
    }
}
