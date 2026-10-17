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
        arch_objects::AddressSpaceId,
        carved::{CarvedId, CarvedObject},
    },
    libobject::{
        CapError, ObjectType,
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

/// A scheduler in the scheduler tree.
#[derive(Clone, Copy, Debug)]
pub struct Scheduler {
    root: bool,
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
}

impl Scheduler {
    /// The root scheduler; only boot code creates it.
    pub const fn root() -> Self {
        Self {
            root: true,
            record_space: None,
            stride: 0,
            regions: [None; MAX_REGIONS],
            event_count: None,
            events: 0,
            timer_expirations: 0,
        }
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
