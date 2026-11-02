//! `WaitSet` kernel object: a Thread's multiplexer over Notifications and
//! `EventCount`s.
//!
//! A set holds up to [`MAX_MEMBERS`] members, each in a numbered slot with one
//! condition: a Notification member is ready while any bit of its mask is
//! pending, an `EventCount` member while its value has reached its target.
//! `Await` returns the mask of ready slots (bit *n* for slot *n*) and consumes
//! nothing; the caller then polls or reads the ready members. Readiness is
//! level-triggered, so a condition that held before the wait began counts.
//!
//! A set has at most one waiter. The set registers no wake-up with its
//! members. After every Signal or Advance, the nucleus re-evaluates the sets
//! that have a blocked waiter (`Nucleus::wake_wait_sets`), which keeps the
//! members free of back-pointers and the wake-up race-free under the kernel
//! lock.
//!
//! A member whose object is no longer live is reported, not hidden: `Await`
//! fails with `MemberRetired` naming the slot, and a waiter blocked on the
//! set is woken with that error. The member stays in its slot until
//! `RemoveMember`. No Notification or `EventCount` can be retired yet, so
//! nothing reaches this path today.
//!
//! A Notification listed in a set refuses direct waits (`Watched`), and one
//! with a direct waiter refuses to join a set: a direct waiter would take the
//! bits before the set could see them. The count lives in the Notification
//! (`Notification::watch`); the API layer keeps it in step with the members.

use {
    crate::objects::{
        EventCount, Notification, NucleusObject,
        access::ObjectId,
        carved::{Carved, CarvedId, CarvedObject},
        completion::{PendingKind, PendingPool},
        thread::ThreadId,
    },
    libobject::{CapError, KeySlot, ObjectType},
};

/// Members a set holds.
pub const MAX_MEMBERS: usize = 8;

/// One member: the object and its condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub object: CarvedId,
    /// Notification: the bits to watch, nonzero. `EventCount`: the target.
    pub condition: u64,
}

/// How a member stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    /// The condition does not hold.
    NotMet,
    /// The condition holds: the slot is ready.
    Met,
    /// The member's object is no longer live.
    Retired,
}

/// Outcome of an `Await` attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitSetOutcome {
    /// At least one member was ready: the mask of ready slots.
    Ready(u64),
    /// Nothing was ready: the caller blocks. The identity names its
    /// pending-invocation record.
    Blocked(ObjectId),
}

/// `WaitSet` kernel object.
pub struct WaitSet {
    members: [Option<Member>; MAX_MEMBERS],
    /// The record of the blocked waiter, if any.
    waiter: Option<ObjectId>,
}

impl NucleusObject for WaitSet {
    const TYPE: ObjectType = ObjectType::WAIT_SET;
}

impl CarvedObject for WaitSet {}

impl WaitSet {
    pub const fn new() -> Self {
        Self {
            members: [None; MAX_MEMBERS],
            waiter: None,
        }
    }

    /// The member in `slot`, if any.
    pub fn member(&self, slot: usize) -> Option<&Member> {
        self.members.get(slot)?.as_ref()
    }

    fn checked_slot(slot: u64) -> Result<usize, CapError> {
        usize::try_from(slot)
            .ok()
            .filter(|&slot| slot < MAX_MEMBERS)
            .ok_or_else(|| CapError::InvalidSlot(KeySlot(u32::try_from(slot).unwrap_or(u32::MAX))))
    }

    /// A Notification watches at least one bit.
    fn checked_condition(kind: ObjectType, condition: u64) -> Result<(), CapError> {
        if kind == ObjectType::NOTIFICATION && condition == 0 {
            Err(CapError::InvalidOperation)
        } else {
            Ok(())
        }
    }

    /// Put `object` in the vacant `slot` with `condition`.
    ///
    /// # Errors
    ///
    /// `InvalidSlot` past the last slot, `SlotOccupied`, `TypeMismatch` for an
    /// object that is not a Notification or `EventCount`, `InvalidOperation`
    /// for an empty Notification mask.
    pub fn add_member(
        &mut self,
        slot: u64,
        object: CarvedId,
        condition: u64,
    ) -> Result<(), CapError> {
        let index = Self::checked_slot(slot)?;
        if object.kind != ObjectType::NOTIFICATION && object.kind != ObjectType::EVENT_COUNT {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::NOTIFICATION,
                found: object.kind,
            });
        }
        Self::checked_condition(object.kind, condition)?;
        if self.members[index].is_some() {
            return Err(CapError::SlotOccupied(KeySlot(
                u32::try_from(index).unwrap_or(u32::MAX),
            )));
        }
        self.members[index] = Some(Member { object, condition });
        Ok(())
    }

    /// Change the condition of the member in `slot`.
    ///
    /// # Errors
    ///
    /// `InvalidSlot`, `EmptySlot`, `InvalidOperation` for an empty
    /// Notification mask.
    pub fn update_member(&mut self, slot: u64, condition: u64) -> Result<(), CapError> {
        let index = Self::checked_slot(slot)?;
        let member = self.members[index]
            .as_mut()
            .ok_or(CapError::EmptySlot(KeySlot(
                u32::try_from(index).unwrap_or(u32::MAX),
            )))?;
        Self::checked_condition(member.object.kind, condition)?;
        member.condition = condition;
        Ok(())
    }

    /// Empty `slot` and return the member it held.
    ///
    /// # Errors
    ///
    /// `InvalidSlot`, `EmptySlot`.
    pub fn remove_member(&mut self, slot: u64) -> Result<Member, CapError> {
        let index = Self::checked_slot(slot)?;
        self.members[index]
            .take()
            .ok_or(CapError::EmptySlot(KeySlot(
                u32::try_from(index).unwrap_or(u32::MAX),
            )))
    }

    /// The mask of ready slots, with `standing` deciding each member.
    ///
    /// # Errors
    ///
    /// `MemberRetired` for the first member whose object is no longer live,
    /// ahead of any ready slot.
    pub fn ready_mask_with(
        &self,
        standing: impl Fn(&Member) -> Condition,
    ) -> Result<u64, CapError> {
        let mut mask = 0;
        for (slot, member) in self.members.iter().enumerate() {
            let Some(member) = member else {
                continue;
            };
            match standing(member) {
                Condition::NotMet => {}
                Condition::Met => mask |= 1 << slot,
                Condition::Retired => {
                    return Err(CapError::MemberRetired {
                        slot: u64::try_from(slot).unwrap_or(u64::MAX),
                    });
                }
            }
        }
        Ok(mask)
    }

    /// The mask of ready slots, reading the members' objects.
    ///
    /// # Errors
    ///
    /// As [`Self::ready_mask_with`].
    pub fn ready_mask(&self) -> Result<u64, CapError> {
        self.ready_mask_with(member_condition)
    }

    /// `Await`: the mask of ready slots if any, else register the caller's
    /// pending record as the set's waiter. `standing` decides each member.
    ///
    /// # Errors
    ///
    /// `PoolExhausted` if the set already has a waiter, `MemberRetired` as
    /// [`Self::ready_mask_with`].
    pub fn await_ready_with(
        &mut self,
        own: CarvedId,
        waiter: ThreadId,
        pending: &mut PendingPool,
        standing: impl Fn(&Member) -> Condition,
    ) -> Result<WaitSetOutcome, CapError> {
        if self.waiter.is_some() {
            return Err(CapError::PoolExhausted);
        }
        let mask = self.ready_mask_with(standing)?;
        if mask != 0 {
            return Ok(WaitSetOutcome::Ready(mask));
        }
        let record = pending.block(waiter, PendingKind::WaitSetAwait, own)?;
        self.waiter = Some(record);
        Ok(WaitSetOutcome::Blocked(record))
    }

    /// `Await` against the members' live objects.
    ///
    /// # Errors
    ///
    /// As [`Self::await_ready_with`].
    pub fn await_ready(
        &mut self,
        own: CarvedId,
        waiter: ThreadId,
        pending: &mut PendingPool,
    ) -> Result<WaitSetOutcome, CapError> {
        self.await_ready_with(own, waiter, pending, member_condition)
    }

    /// The blocked waiter's record, if any.
    pub fn waiter(&self) -> Option<ObjectId> {
        self.waiter
    }

    /// Forget the blocked waiter (it was completed, timed out or torn down).
    pub fn clear_waiter(&mut self) {
        self.waiter = None;
    }

    /// Forget the waiter if it is `thread`'s (teardown cancellation). Returns
    /// how many records were unqueued, as the other objects' queues do.
    pub fn remove_waiter(&mut self, thread: ThreadId, pending: &PendingPool) -> usize {
        match self.waiter {
            Some(record) if matches!(pending.waiter(record), Ok(blocked) if blocked == thread) => {
                self.waiter = None;
                1
            }
            _ => 0,
        }
    }
}

impl Default for WaitSet {
    fn default() -> Self {
        Self::new()
    }
}

/// How `member` stands right now.
fn member_condition(member: &Member) -> Condition {
    // SAFETY: carved identities name kernel-issued records that are never
    // freed; validation checks the header, so a retired object is rejected
    // before any dereference. The kernel lock excludes concurrent mutation.
    unsafe {
        match member.object.kind {
            ObjectType::NOTIFICATION => match Carved::<Notification>::validate(member.object) {
                Ok(notification) => met((*notification).pending_bits() & member.condition != 0),
                Err(_) => Condition::Retired,
            },
            ObjectType::EVENT_COUNT => match Carved::<EventCount>::validate(member.object) {
                Ok(event_count) => met((*event_count).read() >= member.condition),
                Err(_) => Condition::Retired,
            },
            _ => Condition::Retired,
        }
    }
}

fn met(holds: bool) -> Condition {
    if holds {
        Condition::Met
    } else {
        Condition::NotMet
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{Condition, MAX_MEMBERS, Member, WaitSet, WaitSetOutcome},
        crate::objects::{
            carved::{CarvedId, test_id},
            completion::PendingPool,
            thread::ThreadId,
        },
        libobject::{CapError, KeySlot, ObjectType},
    };

    fn notification(address: u64) -> CarvedId {
        test_id(ObjectType::NOTIFICATION, address)
    }

    fn event_count(address: u64) -> CarvedId {
        test_id(ObjectType::EVENT_COUNT, address)
    }

    fn set_id() -> CarvedId {
        test_id(ObjectType::WAIT_SET, 0x9000)
    }

    fn waiter(index: u32) -> ThreadId {
        test_id(ObjectType::THREAD, 0x10_0000 + u64::from(index) * 0x1000)
    }

    /// Readiness by address: the object at address `a` is ready when `a` is in
    /// `ready` and retired when it is in `retired`.
    fn ready_at(ready: &'static [u64]) -> impl Fn(&Member) -> Condition {
        move |member| {
            if ready.contains(&member.object.address) {
                Condition::Met
            } else {
                Condition::NotMet
            }
        }
    }

    fn retired_at(retired: &'static [u64]) -> impl Fn(&Member) -> Condition {
        move |member| {
            if retired.contains(&member.object.address) {
                Condition::Retired
            } else {
                Condition::Met
            }
        }
    }

    /// The mask of ready slots, which must not fail.
    fn mask_of(set: &WaitSet, standing: impl Fn(&Member) -> Condition) -> u64 {
        match set.ready_mask_with(standing) {
            Ok(mask) => mask,
            Err(_) => panic!("no member is retired"),
        }
    }

    /// Add a member that must be accepted.
    fn add(set: &mut WaitSet, slot: u64, object: CarvedId, condition: u64) {
        assert!(set.add_member(slot, object, condition).is_ok());
    }

    fn is_slot_error(result: Result<(), CapError>, expected: &str, slot: u32) -> bool {
        match (result, expected) {
            (Err(CapError::InvalidSlot(found)), "invalid") => found == KeySlot(slot),
            (Err(CapError::SlotOccupied(found)), "occupied") => found == KeySlot(slot),
            (Err(CapError::EmptySlot(found)), "empty") => found == KeySlot(slot),
            _ => false,
        }
    }

    #[test_case]
    fn an_empty_set_has_nothing_ready() {
        let set = WaitSet::new();
        assert_eq!(mask_of(&set, |_| Condition::Met), 0);
        assert!(set.waiter().is_none());
    }

    #[test_case]
    fn members_fill_slots_and_the_mask_has_a_bit_per_ready_slot() {
        let mut set = WaitSet::new();
        add(&mut set, 0, notification(0x1000), 0b1);
        add(&mut set, 3, event_count(0x2000), 5);
        add(&mut set, 7, notification(0x3000), 0b110);
        assert_eq!(mask_of(&set, ready_at(&[0x1000, 0x3000])), 0b1000_0001);
        assert_eq!(mask_of(&set, ready_at(&[0x2000])), 0b1000);
        assert_eq!(mask_of(&set, ready_at(&[])), 0);
        assert_eq!(set.member(3).map(|member| member.condition), Some(5));
        assert!(set.member(1).is_none());
    }

    #[test_case]
    fn slots_outside_the_set_and_occupied_ones_are_refused() {
        let mut set = WaitSet::new();
        assert!(is_slot_error(
            set.add_member(MAX_MEMBERS as u64, notification(0x1000), 1),
            "invalid",
            MAX_MEMBERS as u32
        ));
        assert!(is_slot_error(
            set.add_member(u64::MAX, notification(0x1000), 1),
            "invalid",
            u32::MAX
        ));
        add(&mut set, 2, notification(0x1000), 1);
        assert!(is_slot_error(
            set.add_member(2, event_count(0x2000), 1),
            "occupied",
            2
        ));
        assert_eq!(
            set.member(2).map(|member| member.object),
            Some(notification(0x1000)),
            "a refused add changes nothing"
        );
    }

    #[test_case]
    fn only_notifications_and_event_counts_are_members_and_a_mask_is_nonzero() {
        let mut set = WaitSet::new();
        assert!(matches!(
            set.add_member(0, test_id(ObjectType::TIME, 0x1000), 1),
            Err(CapError::TypeMismatch { .. })
        ));
        assert!(matches!(
            set.add_member(0, notification(0x1000), 0),
            Err(CapError::InvalidOperation)
        ));
        // An `EventCount` target of zero is always ready.
        add(&mut set, 0, event_count(0x2000), 0);
        assert_eq!(mask_of(&set, |_| Condition::Met), 1);
    }

    #[test_case]
    fn a_condition_changes_while_the_member_stays() {
        let mut set = WaitSet::new();
        add(&mut set, 1, event_count(0x2000), 5);
        assert!(set.update_member(1, 9).is_ok());
        assert_eq!(set.member(1).map(|member| member.condition), Some(9));
        assert_eq!(
            set.member(1).map(|member| member.object),
            Some(event_count(0x2000))
        );
        add(&mut set, 2, notification(0x1000), 0b1);
        assert!(matches!(
            set.update_member(2, 0),
            Err(CapError::InvalidOperation)
        ));
        assert_eq!(
            set.member(2).map(|member| member.condition),
            Some(0b1),
            "a refused update changes nothing"
        );
        assert!(is_slot_error(set.update_member(4, 1), "empty", 4));
        assert!(is_slot_error(set.update_member(8, 1), "invalid", 8));
    }

    #[test_case]
    fn removing_empties_the_slot_for_reuse() {
        let mut set = WaitSet::new();
        add(&mut set, 5, notification(0x1000), 1);
        assert!(set.remove_member(5).is_ok());
        assert!(set.member(5).is_none());
        assert!(is_slot_error(set.remove_member(5).map(|_| ()), "empty", 5));
        assert!(is_slot_error(
            set.remove_member(9).map(|_| ()),
            "invalid",
            9
        ));
        add(&mut set, 5, event_count(0x2000), 1);
    }

    #[test_case]
    fn a_retired_member_is_an_error_naming_its_slot_ahead_of_ready_ones() {
        let mut set = WaitSet::new();
        let mut pending = PendingPool::new();
        add(&mut set, 1, notification(0x1000), 1);
        add(&mut set, 4, event_count(0x2000), 1);
        add(&mut set, 6, notification(0x3000), 1);
        // Slot 1 is ready, slots 4 and 6 are retired: the first retired slot
        // is reported.
        assert!(matches!(
            set.ready_mask_with(retired_at(&[0x2000, 0x3000])),
            Err(CapError::MemberRetired { slot: 4 })
        ));
        assert!(matches!(
            set.await_ready_with(set_id(), waiter(0), &mut pending, retired_at(&[0x3000])),
            Err(CapError::MemberRetired { slot: 6 })
        ));
        assert!(set.waiter().is_none(), "a refused wait registers nothing");
        assert_eq!(pending.len(), 0);
    }

    #[test_case]
    fn removing_a_member_returns_it() {
        let mut set = WaitSet::new();
        add(&mut set, 3, event_count(0x2000), 7);
        let removed = set.remove_member(3);
        assert!(matches!(
            removed,
            Ok(member) if member.object == event_count(0x2000) && member.condition == 7
        ));
    }

    #[test_case]
    fn a_ready_member_answers_at_once_and_registers_no_waiter() {
        let mut set = WaitSet::new();
        let mut pending = PendingPool::new();
        add(&mut set, 2, notification(0x1000), 1);
        assert!(matches!(
            set.await_ready_with(set_id(), waiter(0), &mut pending, ready_at(&[0x1000])),
            Ok(WaitSetOutcome::Ready(0b100))
        ));
        assert!(set.waiter().is_none());
        assert_eq!(pending.len(), 0);
    }

    #[test_case]
    fn nothing_ready_blocks_and_the_set_holds_one_waiter() {
        let mut set = WaitSet::new();
        let mut pending = PendingPool::new();
        add(&mut set, 0, notification(0x1000), 1);
        let Ok(WaitSetOutcome::Blocked(record)) =
            set.await_ready_with(set_id(), waiter(0), &mut pending, ready_at(&[]))
        else {
            panic!("expected the caller to block");
        };
        assert_eq!(set.waiter(), Some(record));
        // A second waiter is refused, even if something is ready.
        assert!(matches!(
            set.await_ready_with(set_id(), waiter(1), &mut pending, ready_at(&[])),
            Err(CapError::PoolExhausted)
        ));
        assert!(matches!(
            set.await_ready_with(set_id(), waiter(1), &mut pending, ready_at(&[0x1000])),
            Err(CapError::PoolExhausted)
        ));
        assert_eq!(pending.len(), 1);
    }

    #[test_case]
    fn tearing_down_the_waiter_clears_it_and_only_its_own() {
        let mut set = WaitSet::new();
        let mut pending = PendingPool::new();
        add(&mut set, 0, notification(0x1000), 1);
        assert!(
            set.await_ready_with(set_id(), waiter(0), &mut pending, ready_at(&[]))
                .is_ok()
        );
        assert_eq!(set.remove_waiter(waiter(1), &pending), 0);
        assert!(set.waiter().is_some());
        assert_eq!(set.remove_waiter(waiter(0), &pending), 1);
        assert!(set.waiter().is_none());
    }
}
