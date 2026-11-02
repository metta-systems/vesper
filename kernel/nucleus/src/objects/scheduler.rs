//! `Scheduler` kernel object: authority to make Threads run.
//!
//! Kickstart carves the one root Scheduler from the boot Untyped and installs
//! it at `KeySlot::ROOT_SCHEDULER`. Only the root configures and starts
//! Threads created by `Untyped.Retype` (Composite's root-only thread creation,
//! expressed as activation). Scheduler derivation and donation are not
//! implemented yet; `Untyped.Retype` cannot create a Scheduler.
//!
//! A Scheduler also owns its record table: the regions (shared Frames) the
//! nucleus publishes scheduling records into, and the `EventCount` it advances
//! when it publishes a change. The records themselves are in
//! [`libobject::scheduler_record`].

use {
    crate::objects::{
        NucleusObject,
        access::ObjectId,
        arch_objects::AddressSpaceId,
        carved::{CarvedId, CarvedObject},
    },
    libobject::{
        CapError, ObjectType,
        scheduler::activation_mask,
        scheduler_record::{MAX_REGIONS, RECORD_ALIGN, SCHEDULER_SLOT, valid_stride},
    },
};

/// One shared Frame of the record table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// Physical base of the Frame.
    pub frame_paddr: u64,
    /// Size of the Frame in bytes.
    pub frame_bytes: u64,
    /// The first slot the Frame covers.
    pub first_slot: u32,
    /// The number of slots the Frame covers.
    pub slots: u32,
}

impl Region {
    fn slot_range(&self) -> core::ops::Range<u64> {
        let first = u64::from(self.first_slot);
        first..first + u64::from(self.slots)
    }

    fn frame_range(&self) -> core::ops::Range<u64> {
        self.frame_paddr..self.frame_paddr + self.frame_bytes
    }
}

/// The execution a parent granted its child by dispatching the child's
/// dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The counter value at which the grant ends, if it is limited.
    pub deadline_ticks: Option<u64>,
}

/// A scheduler in the scheduler tree.
#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    root: bool,
    /// The Scheduler that adopted this one; set once, never for the root.
    parent: Option<CarvedId>,
    /// Levels below the root (the root is 0).
    depth: u8,
    /// Schedulers adopted by this one.
    children: u32,
    /// Threads this Scheduler owns: configured by it or moved to it, and not
    /// yet retired or moved away.
    threads: u32,
    /// The parent's grant in force: the parent's dispatcher dispatched this
    /// Scheduler's dispatcher, until the counter reaches the deadline (none
    /// for no limit). `None` when the domain is not running.
    grant: Option<Grant>,
    /// The Threads this Scheduler owns form a list through
    /// `Thread::owner_next`; this is its head.
    first_thread: Option<CarvedId>,
    /// The `AddressSpace` the record Frames are mapped in; fixed by the first
    /// region.
    record_space: Option<AddressSpaceId>,
    /// The table's stride in bytes; fixed by the first region.
    stride: u32,
    regions: [Option<Region>; MAX_REGIONS],
    /// The `EventCount` advanced when a record changes, once bound.
    event_count: Option<CarvedId>,
    /// Scheduling events published so far.
    events: u64,
    /// Armed timers that fired.
    timer_expirations: u64,
    /// The Thread that makes this scheduler's decisions, once bound.
    dispatcher: Option<CarvedId>,
    /// The dispatcher's parked `Dispatch` call, if it waits, and whether it
    /// dispatched nothing.
    parked: Option<(ObjectId, bool)>,
    /// The events besides the mandatory ones that activate the dispatcher.
    activation_mask: u64,
    /// Activation reasons raised while the dispatcher was not waiting; the
    /// next `Dispatch` returns them at once.
    activation_pending: u64,
}

impl Scheduler {
    /// The root scheduler; only boot code creates it.
    pub const fn root() -> Self {
        Self {
            root: true,
            parent: None,
            depth: 0,
            children: 0,
            threads: 0,
            grant: None,
            first_thread: None,
            record_space: None,
            stride: 0,
            regions: [None; MAX_REGIONS],
            event_count: None,
            events: 0,
            timer_expirations: 0,
            dispatcher: None,
            parked: None,
            activation_mask: 0,
            activation_pending: 0,
        }
    }

    /// A fresh Scheduler as `Untyped.Retype` makes it: not the root, no
    /// parent, nothing in it. It has no execution authority until a parent
    /// adopts it and dispatches its dispatcher.
    pub const fn inert() -> Self {
        let mut scheduler = Self::root();
        scheduler.root = false;
        scheduler
    }

    /// The levels a Scheduler may sit below the root.
    pub const MAX_DEPTH: u8 = 3;

    /// The Scheduler that adopted this one.
    pub const fn parent(&self) -> Option<CarvedId> {
        self.parent
    }

    /// Levels below the root.
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// Whether `Adopt` may take this Scheduler: no parent, not the root, and
    /// no children, Threads or dispatcher. Such a Scheduler has no
    /// descendants, so adopting it cannot close a cycle.
    pub const fn is_fresh(&self) -> bool {
        !self.root
            && self.parent.is_none()
            && self.children == 0
            && self.threads == 0
            && self.dispatcher.is_none()
    }

    /// Set the parent, once, and the depth that follows from it.
    pub fn set_parent(&mut self, parent: CarvedId, depth: u8) {
        self.parent = Some(parent);
        self.depth = depth;
    }

    /// The parent's grant in force, if its dispatcher dispatched this
    /// Scheduler's dispatcher and that dispatch has not ended.
    pub const fn grant(&self) -> Option<Grant> {
        self.grant
    }

    /// The head of the list of Threads this Scheduler owns.
    pub const fn first_thread(&self) -> Option<CarvedId> {
        self.first_thread
    }

    /// Set the head of the list of owned Threads.
    pub fn set_first_thread(&mut self, thread: Option<CarvedId>) {
        self.first_thread = thread;
    }

    /// The dispatcher is gone (it faulted or was retired); another can be
    /// bound.
    pub fn unbind_dispatcher(&mut self) {
        self.dispatcher = None;
        self.parked = None;
    }

    /// Set or end the grant.
    pub fn set_grant(&mut self, grant: Option<Grant>) {
        self.grant = grant;
    }

    /// A child was adopted.
    pub fn child_adopted(&mut self) {
        self.children = self.children.saturating_add(1);
    }

    /// A Thread joined this Scheduler.
    pub fn thread_joined(&mut self) {
        self.threads = self.threads.saturating_add(1);
    }

    /// A Thread left this Scheduler (retired, or moved to another).
    pub fn thread_left(&mut self) {
        self.threads = self.threads.saturating_sub(1);
    }

    /// Threads this Scheduler owns.
    pub const fn thread_count(&self) -> u32 {
        self.threads
    }

    /// Whether this is the root scheduler.
    pub const fn is_root(&self) -> bool {
        self.root
    }

    /// The table's stride in bytes, once a region is shared.
    pub const fn stride(&self) -> u32 {
        self.stride
    }

    /// The regions shared so far, in the order they were added.
    pub fn regions(&self) -> impl Iterator<Item = &Region> {
        self.regions.iter().flatten()
    }

    /// Add the Frame at `frame_paddr` (`frame_bytes` long, mapped in
    /// `space`) as the region covering slots from `first_slot` at `stride`
    /// bytes each. Every check precedes the change, so a failure changes
    /// nothing. Returns the new region.
    pub fn add_region(
        &mut self,
        space: AddressSpaceId,
        frame_paddr: u64,
        frame_bytes: u64,
        first_slot: u32,
        stride: u32,
    ) -> Result<Region, CapError> {
        if !valid_stride(stride, frame_bytes) || !frame_paddr.is_multiple_of(RECORD_ALIGN as u64) {
            return Err(CapError::InvalidOperation);
        }
        let slots = u32::try_from(frame_bytes / u64::from(stride))
            .map_err(|_too_many| CapError::InvalidOperation)?;
        first_slot
            .checked_add(slots)
            .ok_or(CapError::InvalidOperation)?;
        let frame_end = frame_paddr
            .checked_add(frame_bytes)
            .ok_or(CapError::InvalidOperation)?;
        match self.record_space {
            // The first region fixes the AddressSpace and the stride, and
            // holds the scheduler record in slot 0.
            None if first_slot != SCHEDULER_SLOT => return Err(CapError::InvalidOperation),
            Some(fixed) if fixed != space || self.stride != stride => {
                return Err(CapError::InvalidOperation);
            }
            _ => {}
        }
        let region = Region {
            frame_paddr,
            frame_bytes,
            first_slot,
            slots,
        };
        let mut free = None;
        for (index, existing) in self.regions.iter().enumerate() {
            let Some(existing) = existing else {
                free.get_or_insert(index);
                continue;
            };
            // Sharing a Frame again, or sharing slots that are taken, is a
            // conflicting re-share.
            let slots_overlap = region.slot_range().start < existing.slot_range().end
                && existing.slot_range().start < region.slot_range().end;
            let frames_overlap = frame_paddr < existing.frame_range().end
                && existing.frame_range().start < frame_end;
            if slots_overlap || frames_overlap {
                return Err(CapError::InvalidOperation);
            }
        }
        let index = free.ok_or(CapError::InvalidOperation)?;
        self.record_space = Some(space);
        self.stride = stride;
        self.regions[index] = Some(region);
        Ok(region)
    }

    /// The physical address of the record in `slot`, if a region covers it.
    pub fn record_paddr(&self, slot: u32) -> Option<u64> {
        let region = self
            .regions()
            .find(|region| region.slot_range().contains(&u64::from(slot)))?;
        let index = u64::from(slot - region.first_slot);
        Some(region.frame_paddr + index * u64::from(self.stride))
    }

    /// Whether a Thread record can use `slot`: a Thread slot some region
    /// covers.
    pub fn covers_thread_slot(&self, slot: u32) -> bool {
        slot != SCHEDULER_SLOT && self.record_paddr(slot).is_some()
    }

    /// The `EventCount` bound for notifications, if any.
    pub const fn event_count(&self) -> Option<CarvedId> {
        self.event_count
    }

    /// Bind `event_count`. Binding is once-only.
    pub fn bind_event_count(&mut self, event_count: CarvedId) -> Result<(), CapError> {
        if self.event_count.is_some() {
            return Err(CapError::InvalidOperation);
        }
        self.event_count = Some(event_count);
        Ok(())
    }

    /// Forget a retired `EventCount`.
    pub fn unbind_event_count(&mut self) {
        self.event_count = None;
    }

    /// Count a published scheduling event; the new total.
    pub fn count_event(&mut self) -> u64 {
        self.events = self.events.saturating_add(1);
        self.events
    }

    /// Scheduling events published so far.
    pub const fn events(&self) -> u64 {
        self.events
    }

    /// The bound dispatcher, if any.
    pub const fn dispatcher(&self) -> Option<CarvedId> {
        self.dispatcher
    }

    /// Bind `thread` as the dispatcher. Binding is once-only.
    pub fn bind_dispatcher(&mut self, thread: CarvedId) -> Result<(), CapError> {
        if self.dispatcher.is_some() {
            return Err(CapError::InvalidOperation);
        }
        self.dispatcher = Some(thread);
        Ok(())
    }

    /// The events that activate the dispatcher besides the mandatory ones.
    pub const fn activation_mask(&self) -> u64 {
        self.activation_mask
    }

    /// Choose the events that activate the dispatcher; bits outside
    /// [`activation_mask::ALL`](libobject::scheduler::activation_mask::ALL)
    /// are refused.
    pub fn set_activation_mask(&mut self, mask: u64) -> Result<(), CapError> {
        if mask & !activation_mask::ALL != 0 {
            return Err(CapError::InvalidOperation);
        }
        self.activation_mask = mask;
        Ok(())
    }

    /// Record the dispatcher's `Dispatch` call `record` as waiting;
    /// `idle` when it dispatched nothing.
    pub fn park_dispatcher(&mut self, record: ObjectId, idle: bool) {
        self.parked = Some((record, idle));
    }

    /// The waiting `Dispatch` call, if any, and whether it dispatched
    /// nothing.
    pub const fn parked(&self) -> Option<(ObjectId, bool)> {
        self.parked
    }

    /// Take the waiting `Dispatch` call: the dispatcher is being activated.
    pub fn take_parked(&mut self) -> Option<ObjectId> {
        self.parked.take().map(|(record, _idle)| record)
    }

    /// Raise activation `reasons` that nobody was waiting for.
    pub fn add_pending(&mut self, reasons: u64) {
        self.activation_pending |= reasons;
    }

    /// Take and clear the raised activation reasons.
    pub fn take_pending(&mut self) -> u64 {
        core::mem::take(&mut self.activation_pending)
    }

    /// The raised activation reasons, without clearing them.
    pub const fn pending(&self) -> u64 {
        self.activation_pending
    }

    /// Count an armed timer that fired.
    pub fn count_timer_expiration(&mut self) {
        self.timer_expirations = self.timer_expirations.saturating_add(1);
    }

    /// Armed timers that fired.
    pub const fn timer_expirations(&self) -> u64 {
        self.timer_expirations
    }
}

impl NucleusObject for Scheduler {
    const TYPE: ObjectType = ObjectType::SCHEDULER;
}

impl CarvedObject for Scheduler {}

#[cfg(test)]
mod tests {
    use {
        super::Scheduler,
        crate::objects::carved::{CarvedId, test_id},
        libobject::{CapError, ObjectType},
    };

    fn space(index: u64) -> CarvedId {
        test_id(ObjectType::ADDRESS_SPACE, 0x10_000 + index * 0x100)
    }

    fn invalid<T>(result: Result<T, CapError>) -> bool {
        matches!(result, Err(CapError::InvalidOperation))
    }

    #[test_case]
    fn the_first_region_starts_at_slot_zero_and_fixes_space_and_stride() {
        let mut scheduler = Scheduler::root();
        assert!(invalid(scheduler.add_region(
            space(0),
            0x1000,
            4096,
            1,
            128
        )));
        assert_eq!(scheduler.regions().count(), 0, "a refusal changes nothing");
        let region = scheduler
            .add_region(space(0), 0x1000, 4096, 0, 128)
            .ok()
            .unwrap();
        assert_eq!((region.first_slot, region.slots), (0, 32));
        assert_eq!(scheduler.stride(), 128);
        // Another AddressSpace or stride is refused.
        assert!(invalid(scheduler.add_region(
            space(1),
            0x2000,
            4096,
            32,
            128
        )));
        assert!(invalid(scheduler.add_region(
            space(0),
            0x2000,
            4096,
            32,
            192
        )));
        assert_eq!(scheduler.regions().count(), 1);
    }

    #[test_case]
    fn a_stride_must_suit_the_records_and_the_frame() {
        let mut scheduler = Scheduler::root();
        for stride in [0, 64, 100, 130, 4160, u32::MAX] {
            assert!(invalid(scheduler.add_region(
                space(0),
                0x1000,
                4096,
                0,
                stride
            )));
        }
        // A Frame not 64-byte aligned cannot hold aligned records.
        assert!(invalid(scheduler.add_region(
            space(0),
            0x1008,
            4096,
            0,
            128
        )));
        assert_eq!(scheduler.regions().count(), 0);
    }

    #[test_case]
    fn regions_must_not_overlap_in_slots_or_in_memory() {
        let mut scheduler = Scheduler::root();
        scheduler
            .add_region(space(0), 0x1000, 4096, 0, 128)
            .ok()
            .unwrap();
        // The same Frame again, a Frame that overlaps it, and taken slots.
        assert!(invalid(scheduler.add_region(
            space(0),
            0x1000,
            4096,
            32,
            128
        )));
        assert!(invalid(scheduler.add_region(
            space(0),
            0x1800,
            4096,
            32,
            128
        )));
        assert!(invalid(scheduler.add_region(
            space(0),
            0x2000,
            4096,
            31,
            128
        )));
        assert!(invalid(scheduler.add_region(
            space(0),
            0x2000,
            4096,
            0,
            128
        )));
        // The next Frame, with the next slots, fits; so does a gap.
        scheduler
            .add_region(space(0), 0x2000, 4096, 32, 128)
            .ok()
            .unwrap();
        scheduler
            .add_region(space(0), 0x3000, 4096, 100, 128)
            .ok()
            .unwrap();
        assert_eq!(scheduler.regions().count(), 3);
    }

    #[test_case]
    fn at_most_eight_regions() {
        let mut scheduler = Scheduler::root();
        for index in 0..8_u32 {
            scheduler
                .add_region(
                    space(0),
                    0x1000 + u64::from(index) * 0x1000,
                    4096,
                    index * 32,
                    128,
                )
                .ok()
                .unwrap();
        }
        assert!(invalid(scheduler.add_region(
            space(0),
            0x20_000,
            4096,
            256,
            128
        )));
        assert_eq!(scheduler.regions().count(), 8);
    }

    #[test_case]
    fn slots_resolve_to_record_addresses() {
        let mut scheduler = Scheduler::root();
        scheduler
            .add_region(space(0), 0x1000, 4096, 0, 192)
            .ok()
            .unwrap();
        scheduler
            .add_region(space(0), 0x8000, 4096, 100, 192)
            .ok()
            .unwrap();
        assert_eq!(scheduler.record_paddr(0), Some(0x1000));
        assert_eq!(scheduler.record_paddr(1), Some(0x1000 + 192));
        assert_eq!(scheduler.record_paddr(20), Some(0x1000 + 20 * 192));
        // 4096 / 192 = 21 slots: slot 21 is in the gap.
        assert_eq!(scheduler.record_paddr(21), None);
        assert_eq!(scheduler.record_paddr(100), Some(0x8000));
        assert_eq!(scheduler.record_paddr(120), Some(0x8000 + 20 * 192));
        assert_eq!(scheduler.record_paddr(121), None);
        // Slot 0 is the scheduler's record, not a Thread slot.
        assert!(!scheduler.covers_thread_slot(0));
        assert!(scheduler.covers_thread_slot(1));
        assert!(!scheduler.covers_thread_slot(21));
    }

    #[test_case]
    fn the_event_count_binds_once() {
        let mut scheduler = Scheduler::root();
        let event_count = test_id(ObjectType::EVENT_COUNT, 0x4000);
        assert_eq!(scheduler.event_count(), None);
        scheduler.bind_event_count(event_count).ok().unwrap();
        assert_eq!(scheduler.event_count(), Some(event_count));
        assert!(invalid(
            scheduler.bind_event_count(test_id(ObjectType::EVENT_COUNT, 0x5000))
        ));
        assert_eq!(scheduler.event_count(), Some(event_count));
        scheduler.unbind_event_count();
        assert_eq!(scheduler.event_count(), None);
        assert_eq!(scheduler.count_event(), 1);
        assert_eq!(scheduler.count_event(), 2);
        assert_eq!(scheduler.events(), 2);
    }
}
