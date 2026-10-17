//! `Time` kernel object: a CPU budget on one core.
//!
//! A Time carries no priority, period or deadline; scheduling policy lives in
//! userspace schedulers. Budget only moves: [`Time::transfer`] and
//! [`Time::merge`] compute both new states before the caller commits either,
//! so every failure leaves both Times unchanged.
//!
//! Execution is charged in counter ticks and kept exact: a Time holds its
//! granted budget in nanoseconds and the ticks charged against it so far. The
//! charge converts to nanoseconds, rounded up, only when it is compared with
//! the budget or reported, so a Thread never runs past its budget and the
//! remaining budget reads rounded down. An infinite budget is never charged.
//! Every conversion takes the counter frequency, which keeps this module
//! independent of the hardware counter.

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

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Nanoseconds in `ticks` of a counter running at `frequency` Hz, rounded up
/// and below [`INFINITE_BUDGET`].
pub fn ticks_to_ns_ceil(ticks: u64, frequency: u64) -> Budget {
    let nanoseconds = (u128::from(ticks) * NANOS_PER_SECOND).div_ceil(u128::from(frequency.max(1)));
    u64::try_from(nanoseconds)
        .unwrap_or(u64::MAX)
        .min(INFINITE_BUDGET - 1)
}

/// A CPU budget on one core.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    core: usize,
    /// Budget granted so far, in nanoseconds; [`INFINITE_BUDGET`] when
    /// infinite. The charge in `used_ticks` has not been subtracted.
    budget_ns: Budget,
    /// Counter ticks charged against the budget.
    used_ticks: u64,
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
            used_ticks: 0,
            bound: None,
        }
    }

    /// The root Time: `budget_ns` on `core`. Only the boot code creates one.
    pub const fn root(core: usize, budget_ns: Budget) -> Self {
        Self {
            core,
            budget_ns,
            used_ticks: 0,
            bound: None,
        }
    }

    pub fn core(&self) -> usize {
        self.core
    }

    fn is_infinite(&self) -> bool {
        self.budget_ns == INFINITE_BUDGET
    }

    /// Remaining budget in nanoseconds at counter `frequency`, rounded down;
    /// [`INFINITE_BUDGET`] when infinite.
    pub fn remaining_ns(&self, frequency: u64) -> Budget {
        if self.is_infinite() {
            return INFINITE_BUDGET;
        }
        self.budget_ns
            .saturating_sub(ticks_to_ns_ceil(self.used_ticks, frequency))
    }

    /// Whether a finite budget is used up. An infinite one never is.
    pub fn is_exhausted(&self, frequency: u64) -> bool {
        !self.is_infinite() && self.remaining_ns(frequency) == 0
    }

    /// Charge `ticks` of execution. An infinite budget is not charged.
    pub fn charge(&mut self, ticks: u64) {
        if !self.is_infinite() {
            self.used_ticks = self.used_ticks.saturating_add(ticks);
        }
    }

    /// Counter ticks of execution left before the budget is exhausted, or
    /// `None` for an infinite budget. Zero when already exhausted.
    pub fn ticks_until_exhausted(&self, frequency: u64) -> Option<u64> {
        if self.is_infinite() {
            return None;
        }
        // The charge reaches the budget at the first tick count whose
        // rounded-up nanoseconds are at least the budget.
        let ticks_at_exhaustion = match self.budget_ns {
            0 => 0,
            budget => {
                let ticks =
                    u128::from(budget - 1) * u128::from(frequency.max(1)) / NANOS_PER_SECOND + 1;
                u64::try_from(ticks).unwrap_or(u64::MAX)
            }
        };
        Some(ticks_at_exhaustion.saturating_sub(self.used_ticks))
    }

    /// The Thread this Time is bound to, if it is still live. A binding to a
    /// retired Thread no longer counts.
    pub fn bound_thread(&self) -> Option<CarvedId> {
        self.bound.filter(|&thread| {
            // SAFETY: the recorded identity was a live carved Thread when it
            // was bound, and carved memory is never freed; validation checks
            // its header before anything else.
            unsafe { Carved::<Thread>::validate(thread) }.is_ok()
        })
    }

    /// Whether this Time is bound to a Thread that is still live.
    pub fn is_bound(&self) -> bool {
        self.bound_thread().is_some()
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
        frequency: u64,
    ) -> Result<(Self, Self), CapError> {
        if amount == 0 || source.core != destination.core {
            return Err(CapError::InvalidOperation);
        }
        let source_infinite = source.is_infinite();
        let remaining = source.remaining_ns(frequency);
        if !source_infinite && remaining < amount {
            return Err(CapError::InsufficientBudget { remaining });
        }
        let destination_budget = Self::sum(destination.budget_ns, amount)?;

        let mut new_source = *source;
        if !source_infinite {
            new_source.budget_ns -= amount;
        }
        let mut new_destination = *destination;
        new_destination.budget_ns = destination_budget;
        if new_destination.is_infinite() {
            new_destination.used_ticks = 0;
        }
        Ok((new_source, new_destination))
    }

    /// Move all of `source` into `destination`. Returns the new
    /// (destination, source) states; the source is left with nothing
    /// remaining.
    pub fn merge(
        destination: &Self,
        source: &Self,
        frequency: u64,
    ) -> Result<(Self, Self), CapError> {
        if source.core != destination.core {
            return Err(CapError::InvalidOperation);
        }
        let amount = source.remaining_ns(frequency);
        let budget_ns = Self::sum(destination.budget_ns, amount)?;
        let mut new_destination = *destination;
        new_destination.budget_ns = budget_ns;
        if new_destination.is_infinite() {
            new_destination.used_ticks = 0;
        }
        let new_source = if source.is_infinite() {
            Self::empty(source.core)
        } else {
            // What remains is exactly `amount`, so the charge stays.
            let mut emptied = *source;
            emptied.budget_ns -= amount;
            emptied
        };
        Ok((new_destination, new_source))
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
        super::{Time, ticks_to_ns_ceil},
        libobject::{CapError, syscall_status, time::INFINITE_BUDGET},
    };

    /// One tick per nanosecond.
    const NS: u64 = 1_000_000_000;
    /// 19.2 MHz: a tick is 52.08 ns, which is not a whole number of
    /// nanoseconds.
    const MHZ_19_2: u64 = 19_200_000;

    fn error_code<T>(result: Result<T, CapError>) -> (u64, u64, u64) {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(error) => error.code(),
        }
    }

    #[test_case]
    fn empty_time_has_no_budget() {
        let time = Time::empty(0);
        assert_eq!(time.remaining_ns(NS), 0);
        assert!(time.is_exhausted(NS));
    }

    #[test_case]
    fn transfer_from_finite_source_conserves_budget() {
        let root = Time::root(0, 1_000);
        let (source, destination) = Time::transfer(&root, &Time::empty(0), 400, NS)
            .ok()
            .unwrap();
        assert_eq!(source.remaining_ns(NS), 600);
        assert_eq!(destination.remaining_ns(NS), 400);
    }

    #[test_case]
    fn transfer_from_infinite_source_keeps_it_infinite() {
        let root = Time::root(0, INFINITE_BUDGET);
        let (source, destination) = Time::transfer(&root, &Time::empty(0), 5, NS).ok().unwrap();
        assert_eq!(source.remaining_ns(NS), INFINITE_BUDGET);
        assert_eq!(destination.remaining_ns(NS), 5);

        let (_, infinite) = Time::transfer(&root, &Time::empty(0), INFINITE_BUDGET, NS)
            .ok()
            .unwrap();
        assert_eq!(infinite.remaining_ns(NS), INFINITE_BUDGET);
    }

    #[test_case]
    fn transfer_more_than_a_finite_source_holds_reports_its_remaining_budget() {
        let root = Time::root(0, 10);
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(0), 11, NS)),
            (syscall_status::INSUFFICIENT_BUDGET, 10, 0)
        );
        // An infinite amount needs an infinite source.
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(0), INFINITE_BUDGET, NS)),
            (syscall_status::INSUFFICIENT_BUDGET, 10, 0)
        );
    }

    #[test_case]
    fn transfer_rejects_zero_amount_other_core_and_overflowing_sum() {
        let root = Time::root(0, INFINITE_BUDGET);
        let invalid = (syscall_status::INVALID_OPERATION, 0, 0);
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(0), 0, NS)),
            invalid
        );
        assert_eq!(
            error_code(Time::transfer(&root, &Time::empty(1), 1, NS)),
            invalid
        );
        let (_, nearly_full) = Time::transfer(&root, &Time::empty(0), INFINITE_BUDGET - 1, NS)
            .ok()
            .unwrap();
        assert_eq!(
            error_code(Time::transfer(&root, &nearly_full, 1, NS)),
            invalid
        );
    }

    #[test_case]
    fn failed_transfer_returns_no_new_state() {
        // The functions are pure: a failure returns no new states, so the
        // caller's Times are the unchanged originals.
        let root = Time::root(0, 10);
        let destination = Time::empty(0);
        assert!(Time::transfer(&root, &destination, 11, NS).is_err());
        assert_eq!(root.remaining_ns(NS), 10);
        assert_eq!(destination.remaining_ns(NS), 0);
    }

    #[test_case]
    fn nested_transfers_conserve_the_total() {
        let root = Time::root(0, 1_000);
        let (root, child) = Time::transfer(&root, &Time::empty(0), 500, NS)
            .ok()
            .unwrap();
        let (child, grandchild) = Time::transfer(&child, &Time::empty(0), 100, NS)
            .ok()
            .unwrap();
        assert_eq!(
            root.remaining_ns(NS) + child.remaining_ns(NS) + grandchild.remaining_ns(NS),
            1_000
        );
    }

    #[test_case]
    fn merge_moves_everything_and_empties_the_source() {
        let root = Time::root(0, 1_000);
        let (_, first) = Time::transfer(&root, &Time::empty(0), 300, NS)
            .ok()
            .unwrap();
        let (_, second) = Time::transfer(&root, &Time::empty(0), 200, NS)
            .ok()
            .unwrap();
        let (merged, emptied) = Time::merge(&second, &first, NS).ok().unwrap();
        assert_eq!(merged.remaining_ns(NS), 500);
        assert_eq!(emptied.remaining_ns(NS), 0);
    }

    #[test_case]
    fn merging_back_restores_the_parent_budget() {
        let root = Time::root(0, 1_000);
        let (root, child) = Time::transfer(&root, &Time::empty(0), 500, NS)
            .ok()
            .unwrap();
        let (child, grandchild) = Time::transfer(&child, &Time::empty(0), 100, NS)
            .ok()
            .unwrap();

        // Grandchild back into the root, two levels up.
        let (merged, emptied) = Time::merge(&root, &grandchild, NS).ok().unwrap();
        assert_eq!(merged.remaining_ns(NS), 600);
        assert_eq!(emptied.remaining_ns(NS), 0);

        // Child back into the root, one level up.
        let (merged, _) = Time::merge(&merged, &child, NS).ok().unwrap();
        assert_eq!(merged.remaining_ns(NS), 1_000);
    }

    #[test_case]
    fn merge_of_an_infinite_source_makes_the_destination_infinite() {
        let root = Time::root(0, INFINITE_BUDGET);
        let (merged, emptied) = Time::merge(&Time::empty(0), &root, NS).ok().unwrap();
        assert_eq!(merged.remaining_ns(NS), INFINITE_BUDGET);
        assert_eq!(emptied.remaining_ns(NS), 0);
        assert_eq!(
            error_code(Time::merge(&Time::empty(1), &root, NS)),
            (syscall_status::INVALID_OPERATION, 0, 0)
        );
    }

    #[test_case]
    fn charging_drains_the_budget_and_exhausts_it_exactly() {
        let mut time = Time::root(0, 1_000);
        time.charge(400);
        assert_eq!(time.remaining_ns(NS), 600);
        assert!(!time.is_exhausted(NS));
        assert_eq!(time.ticks_until_exhausted(NS), Some(600));
        time.charge(599);
        assert_eq!(time.remaining_ns(NS), 1);
        assert!(!time.is_exhausted(NS));
        time.charge(1);
        assert_eq!(time.remaining_ns(NS), 0);
        assert!(time.is_exhausted(NS));
        assert_eq!(time.ticks_until_exhausted(NS), Some(0));
        // Charging past the budget never wraps.
        time.charge(u64::MAX);
        assert!(time.is_exhausted(NS));
    }

    #[test_case]
    fn an_infinite_budget_is_never_charged() {
        let mut time = Time::root(0, INFINITE_BUDGET);
        time.charge(u64::MAX);
        assert_eq!(time.remaining_ns(NS), INFINITE_BUDGET);
        assert!(!time.is_exhausted(NS));
        assert_eq!(time.ticks_until_exhausted(NS), None);
    }

    #[test_case]
    fn the_charge_rounds_up_so_a_thread_never_runs_past_its_budget() {
        // At 19.2 MHz one tick is 52.08 ns: 20 ticks are 1041.67 ns.
        assert_eq!(ticks_to_ns_ceil(1, MHZ_19_2), 53);
        assert_eq!(ticks_to_ns_ceil(20, MHZ_19_2), 1_042);
        assert_eq!(ticks_to_ns_ceil(0, MHZ_19_2), 0);

        // A 1000 ns budget is exhausted at the 20th tick, not the 19th:
        // 19 ticks are 989.6 ns, 20 ticks round up to 1042 ns.
        let mut time = Time::root(0, 1_000);
        assert_eq!(time.ticks_until_exhausted(MHZ_19_2), Some(20));
        time.charge(19);
        assert_eq!(time.remaining_ns(MHZ_19_2), 1_000 - 990);
        assert!(!time.is_exhausted(MHZ_19_2));
        time.charge(1);
        assert!(time.is_exhausted(MHZ_19_2));
    }

    #[test_case]
    fn exact_ticks_survive_many_small_charges() {
        // 1000 charges of one tick at 19.2 MHz are 52083.33 ns in all,
        // charged as 52084 once, not as 1000 times 53 ns.
        let mut time = Time::root(0, 100_000);
        for _ in 0..1_000 {
            time.charge(1);
        }
        assert_eq!(time.remaining_ns(MHZ_19_2), 100_000 - 52_084);
    }

    #[test_case]
    fn a_refill_after_exhaustion_makes_the_time_runnable_again() {
        let root = Time::root(0, INFINITE_BUDGET);
        let (_, mut thread_time) = Time::transfer(&root, &Time::empty(0), 1_000, NS)
            .ok()
            .unwrap();
        thread_time.charge(1_000);
        assert!(thread_time.is_exhausted(NS));

        let (_, refilled) = Time::transfer(&root, &thread_time, 500, NS).ok().unwrap();
        assert!(!refilled.is_exhausted(NS));
        assert_eq!(refilled.remaining_ns(NS), 500);
    }

    #[test_case]
    fn transfer_and_merge_out_of_a_partly_charged_time_use_what_remains() {
        let root = Time::root(0, INFINITE_BUDGET);
        let (_, mut time) = Time::transfer(&root, &Time::empty(0), 1_000, NS)
            .ok()
            .unwrap();
        time.charge(300);
        assert_eq!(
            error_code(Time::transfer(&time, &Time::empty(0), 701, NS)),
            (syscall_status::INSUFFICIENT_BUDGET, 700, 0)
        );
        let (source, destination) = Time::transfer(&time, &Time::empty(0), 700, NS)
            .ok()
            .unwrap();
        assert_eq!(source.remaining_ns(NS), 0);
        assert_eq!(destination.remaining_ns(NS), 700);

        let (merged, emptied) = Time::merge(&Time::empty(0), &time, NS).ok().unwrap();
        assert_eq!(merged.remaining_ns(NS), 700);
        assert_eq!(emptied.remaining_ns(NS), 0);
        assert!(emptied.is_exhausted(NS));
    }
}
