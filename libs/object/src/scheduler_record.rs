//! Scheduler-shared records: the nucleus's publication channel to a
//! userspace scheduler.
//!
//! A scheduler maps Frames of records into its `AddressSpace` and shares them
//! with `Scheduler.ShareRegion`. Each slot of such a Frame starts with a
//! 128-byte record the nucleus writes: slot 0 is the [`SchedulerRecord`],
//! every other slot a [`ThreadRecord`]. The rest of a slot belongs to the
//! scheduler.
//!
//! The scheduler can write any byte of a record, so the nucleus never reads
//! one back; the authoritative state is kernel-private. A scheduler that
//! corrupts its own records only misleads itself.
//!
//! Every field is atomic, so the nucleus and the scheduler can share a record
//! without data races. The nucleus publishes with a sequence lock: it makes
//! `sequence` odd, writes the fields, then makes it even again with release
//! ordering. [`ThreadRecord::try_snapshot`] and
//! [`SchedulerRecord::try_snapshot`] read a consistent copy.

use {
    crate::thread::ThreadState,
    core::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence},
};

/// Bytes of a record that the nucleus writes.
pub const RECORD_SIZE: usize = 128;

/// [`RECORD_SIZE`] as it is stored in the scheduler record.
const RECORD_SIZE_U32: u32 = 128;
const _: () = assert!(RECORD_SIZE_U32 as usize == RECORD_SIZE);

/// Alignment of a record, and the granularity of a table's stride.
pub const RECORD_ALIGN: usize = 64;

/// Smallest stride of a table: one record.
pub const MIN_STRIDE: u32 = 128;

/// Most regions (shared Frames) one Scheduler's table has.
pub const MAX_REGIONS: usize = 8;

/// The only record layout version so far.
pub const LAYOUT_VERSION: u32 = 1;

/// The slot of the [`SchedulerRecord`]; Thread records use the other slots.
pub const SCHEDULER_SLOT: u32 = 0;

/// `ThreadRecord::cpu` of a Thread that is not running.
pub const NO_CPU: u32 = u32::MAX;

/// `ThreadRecord::flags` bit: the Thread is runnable, but its bound Time is
/// exhausted.
pub const FLAG_AWAITING_BUDGET: u32 = 1 << 0;

/// `ThreadRecord::flags` bit: a Time is bound to the Thread. Without one,
/// `budget_remaining_ns` is `u64::MAX`.
pub const FLAG_HAS_TIME: u32 = 1 << 1;

/// `ThreadRecord::flags` bit: the Thread is the dispatcher of a Scheduler.
/// For the Thread of a parent that is a child's dispatcher it tells the
/// parent that the Thread runs a domain.
pub const FLAG_DISPATCHER: u32 = 1 << 2;

/// What a blocked Thread waits for.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlockReason {
    None = 0,
    NotificationAwait = 1,
    EventCountAwait = 2,
    PpcCall = 3,
    WaitSetAwait = 4,
}

impl BlockReason {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::NotificationAwait),
            2 => Some(Self::EventCountAwait),
            3 => Some(Self::PpcCall),
            4 => Some(Self::WaitSetAwait),
            _ => None,
        }
    }
}

/// Why the Thread last stopped running.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    None = 0,
    Blocked = 1,
    SliceEnd = 2,
    BudgetExhausted = 3,
    Yielded = 4,
    Replaced = 5,
    Exited = 6,
    Faulted = 7,
}

impl StopReason {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::Blocked),
            2 => Some(Self::SliceEnd),
            3 => Some(Self::BudgetExhausted),
            4 => Some(Self::Yielded),
            5 => Some(Self::Replaced),
            6 => Some(Self::Exited),
            7 => Some(Self::Faulted),
            _ => None,
        }
    }
}

/// A consistent copy of a [`ThreadRecord`]. Fields are raw; the accessors
/// decode them.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadSnapshot {
    /// The Thread's state-change count.
    pub generation: u64,
    /// The Thread's incarnation, to notice a reused slot.
    pub incarnation: u32,
    /// A [`ThreadState`].
    pub state: u32,
    /// `FLAG_*` bits.
    pub flags: u32,
    /// A [`BlockReason`].
    pub block_reason: u32,
    /// A [`StopReason`].
    pub last_stop: u32,
    /// The CPU the Thread runs on, or [`NO_CPU`].
    pub cpu: u32,
    /// Nanoseconds the Thread has executed, rounded up.
    pub run_time_ns: u64,
    /// Nanoseconds of its last run, rounded up.
    pub last_run_ns: u64,
    /// Remaining budget of the bound Time, rounded down.
    pub budget_remaining_ns: u64,
}

impl ThreadSnapshot {
    pub fn thread_state(&self) -> Option<ThreadState> {
        ThreadState::try_from(self.state).ok()
    }

    pub fn block_reason(&self) -> Option<BlockReason> {
        BlockReason::from_raw(self.block_reason)
    }

    pub fn stop_reason(&self) -> Option<StopReason> {
        StopReason::from_raw(self.last_stop)
    }

    pub const fn awaits_budget(&self) -> bool {
        self.flags & FLAG_AWAITING_BUDGET != 0
    }

    pub const fn has_time(&self) -> bool {
        self.flags & FLAG_HAS_TIME != 0
    }

    /// Whether the Thread is a Scheduler's dispatcher.
    pub const fn is_dispatcher(&self) -> bool {
        self.flags & FLAG_DISPATCHER != 0
    }
}

/// A consistent copy of the [`SchedulerRecord`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedulerSnapshot {
    /// Events published: the bound `EventCount`'s value.
    pub events: u64,
    /// Armed timers that fired.
    pub timer_expirations: u64,
    /// The dispatcher's state.
    pub dispatcher_state: u32,
    pub flags: u32,
    /// Total time spent handling interrupts, in nanoseconds, rounded up.
    pub interrupt_ns: u64,
    /// When the parent's grant to this (child) Scheduler ends, on the
    /// monotonic clock in nanoseconds, rounded up; 0 when none is in force.
    pub grant_end_ns: u64,
    /// Total time nothing was runnable and the nucleus waited for an
    /// interrupt, in nanoseconds, rounded up.
    pub idle_ns: u64,
}

/// The record of one Thread, written by the nucleus.
#[repr(C, align(64))]
pub struct ThreadRecord {
    sequence: AtomicU64,
    generation: AtomicU64,
    incarnation: AtomicU32,
    state: AtomicU32,
    flags: AtomicU32,
    block_reason: AtomicU32,
    last_stop: AtomicU32,
    cpu: AtomicU32,
    run_time_ns: AtomicU64,
    last_run_ns: AtomicU64,
    budget_remaining_ns: AtomicU64,
    reserved: [AtomicU64; 8],
}

/// The scheduler's own record (slot 0), written by the nucleus.
#[repr(C, align(64))]
pub struct SchedulerRecord {
    sequence: AtomicU64,
    layout: AtomicU32,
    record_size: AtomicU32,
    events: AtomicU64,
    timer_expirations: AtomicU64,
    dispatcher_state: AtomicU32,
    flags: AtomicU32,
    interrupt_ns: AtomicU64,
    grant_end_ns: AtomicU64,
    idle_ns: AtomicU64,
    reserved: [AtomicU64; 8],
}

const _: () = {
    assert!(size_of::<ThreadRecord>() == RECORD_SIZE);
    assert!(align_of::<ThreadRecord>() == RECORD_ALIGN);
    assert!(core::mem::offset_of!(ThreadRecord, sequence) == 0);
    assert!(core::mem::offset_of!(ThreadRecord, generation) == 8);
    assert!(core::mem::offset_of!(ThreadRecord, incarnation) == 16);
    assert!(core::mem::offset_of!(ThreadRecord, state) == 20);
    assert!(core::mem::offset_of!(ThreadRecord, flags) == 24);
    assert!(core::mem::offset_of!(ThreadRecord, block_reason) == 28);
    assert!(core::mem::offset_of!(ThreadRecord, last_stop) == 32);
    assert!(core::mem::offset_of!(ThreadRecord, cpu) == 36);
    assert!(core::mem::offset_of!(ThreadRecord, run_time_ns) == 40);
    assert!(core::mem::offset_of!(ThreadRecord, last_run_ns) == 48);
    assert!(core::mem::offset_of!(ThreadRecord, budget_remaining_ns) == 56);
    assert!(size_of::<SchedulerRecord>() == RECORD_SIZE);
    assert!(align_of::<SchedulerRecord>() == RECORD_ALIGN);
    assert!(core::mem::offset_of!(SchedulerRecord, sequence) == 0);
    assert!(core::mem::offset_of!(SchedulerRecord, layout) == 8);
    assert!(core::mem::offset_of!(SchedulerRecord, record_size) == 12);
    assert!(core::mem::offset_of!(SchedulerRecord, events) == 16);
    assert!(core::mem::offset_of!(SchedulerRecord, timer_expirations) == 24);
    assert!(core::mem::offset_of!(SchedulerRecord, dispatcher_state) == 32);
    assert!(core::mem::offset_of!(SchedulerRecord, flags) == 36);
    assert!(core::mem::offset_of!(SchedulerRecord, interrupt_ns) == 40);
    assert!(core::mem::offset_of!(SchedulerRecord, grant_end_ns) == 48);
    assert!(core::mem::offset_of!(SchedulerRecord, idle_ns) == 56);
};

/// The sequence value that starts a publication after `previous`, and the
/// one that ends it. A corrupted (odd) `previous` is normalized, so the
/// parity the readers rely on stays right whatever the scheduler wrote.
const fn publication_sequences(previous: u64) -> (u64, u64) {
    let base = previous & !1;
    (base.wrapping_add(1), base.wrapping_add(2))
}

impl ThreadRecord {
    /// A record with every field zero.
    pub const fn zeroed() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            incarnation: AtomicU32::new(0),
            state: AtomicU32::new(0),
            flags: AtomicU32::new(0),
            block_reason: AtomicU32::new(0),
            last_stop: AtomicU32::new(0),
            cpu: AtomicU32::new(0),
            run_time_ns: AtomicU64::new(0),
            last_run_ns: AtomicU64::new(0),
            budget_remaining_ns: AtomicU64::new(0),
            reserved: [const { AtomicU64::new(0) }; 8],
        }
    }

    /// View the memory at `address` as a record.
    ///
    /// # Safety
    /// `address` must be 64-byte aligned and point to 128 bytes that stay
    /// mapped for `'a` and are accessed only through atomic operations.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "the caller guarantees the 64-byte alignment"
    )]
    pub unsafe fn at<'a>(address: *const u8) -> &'a Self {
        // SAFETY: the caller's contract; every field is atomic.
        unsafe { &*address.cast::<Self>() }
    }

    /// Reset the record to all zero, as when a Thread is linked to its
    /// slot. Only the nucleus calls this.
    pub fn clear(&self) {
        for word in &self.reserved {
            word.store(0, Ordering::Relaxed);
        }
        self.publish(&ThreadSnapshot::default());
    }

    /// The current sequence number: even when no publication is in progress.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    /// Publish `snapshot`. Only the nucleus calls this, as the single
    /// writer under the kernel lock.
    pub fn publish(&self, snapshot: &ThreadSnapshot) {
        let (begin, end) = publication_sequences(self.sequence.load(Ordering::Relaxed));
        self.sequence.store(begin, Ordering::Relaxed);
        fence(Ordering::Release);
        self.generation
            .store(snapshot.generation, Ordering::Relaxed);
        self.incarnation
            .store(snapshot.incarnation, Ordering::Relaxed);
        self.state.store(snapshot.state, Ordering::Relaxed);
        self.flags.store(snapshot.flags, Ordering::Relaxed);
        self.block_reason
            .store(snapshot.block_reason, Ordering::Relaxed);
        self.last_stop.store(snapshot.last_stop, Ordering::Relaxed);
        self.cpu.store(snapshot.cpu, Ordering::Relaxed);
        self.run_time_ns
            .store(snapshot.run_time_ns, Ordering::Relaxed);
        self.last_run_ns
            .store(snapshot.last_run_ns, Ordering::Relaxed);
        self.budget_remaining_ns
            .store(snapshot.budget_remaining_ns, Ordering::Relaxed);
        self.sequence.store(end, Ordering::Release);
    }

    /// Read a consistent copy, retrying while a publication is in progress
    /// or lands during the read. `None` after `attempts` tries, which a
    /// scheduler that corrupted `sequence` can cause.
    pub fn try_snapshot(&self, attempts: u32) -> Option<ThreadSnapshot> {
        for _ in 0..attempts {
            let before = self.sequence.load(Ordering::Acquire);
            if before & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            let snapshot = ThreadSnapshot {
                generation: self.generation.load(Ordering::Relaxed),
                incarnation: self.incarnation.load(Ordering::Relaxed),
                state: self.state.load(Ordering::Relaxed),
                flags: self.flags.load(Ordering::Relaxed),
                block_reason: self.block_reason.load(Ordering::Relaxed),
                last_stop: self.last_stop.load(Ordering::Relaxed),
                cpu: self.cpu.load(Ordering::Relaxed),
                run_time_ns: self.run_time_ns.load(Ordering::Relaxed),
                last_run_ns: self.last_run_ns.load(Ordering::Relaxed),
                budget_remaining_ns: self.budget_remaining_ns.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.sequence.load(Ordering::Relaxed) == before {
                return Some(snapshot);
            }
        }
        None
    }
}

impl SchedulerRecord {
    /// A record with every field zero except the layout header.
    pub const fn initial() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            layout: AtomicU32::new(LAYOUT_VERSION),
            record_size: AtomicU32::new(RECORD_SIZE_U32),
            events: AtomicU64::new(0),
            timer_expirations: AtomicU64::new(0),
            dispatcher_state: AtomicU32::new(0),
            flags: AtomicU32::new(0),
            interrupt_ns: AtomicU64::new(0),
            grant_end_ns: AtomicU64::new(0),
            idle_ns: AtomicU64::new(0),
            reserved: [const { AtomicU64::new(0) }; 8],
        }
    }

    /// View the memory at `address` as a record.
    ///
    /// # Safety
    /// As [`ThreadRecord::at`].
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "the caller guarantees the 64-byte alignment"
    )]
    pub unsafe fn at<'a>(address: *const u8) -> &'a Self {
        // SAFETY: the caller's contract; every field is atomic.
        unsafe { &*address.cast::<Self>() }
    }

    /// Reset the record to its initial state: the layout header and zero
    /// counters. Only the nucleus calls this, when the first region is
    /// shared.
    pub fn initialize(&self) {
        for word in &self.reserved {
            word.store(0, Ordering::Relaxed);
        }
        self.layout.store(LAYOUT_VERSION, Ordering::Relaxed);
        self.record_size.store(RECORD_SIZE_U32, Ordering::Relaxed);
        self.publish(&SchedulerSnapshot::default());
    }

    /// The layout version and record size the nucleus wrote.
    pub fn layout(&self) -> (u32, u32) {
        (
            self.layout.load(Ordering::Relaxed),
            self.record_size.load(Ordering::Relaxed),
        )
    }

    /// The current sequence number: even when no publication is in progress.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    /// Publish `snapshot`. Only the nucleus calls this.
    pub fn publish(&self, snapshot: &SchedulerSnapshot) {
        let (begin, end) = publication_sequences(self.sequence.load(Ordering::Relaxed));
        self.sequence.store(begin, Ordering::Relaxed);
        fence(Ordering::Release);
        self.events.store(snapshot.events, Ordering::Relaxed);
        self.timer_expirations
            .store(snapshot.timer_expirations, Ordering::Relaxed);
        self.dispatcher_state
            .store(snapshot.dispatcher_state, Ordering::Relaxed);
        self.flags.store(snapshot.flags, Ordering::Relaxed);
        self.interrupt_ns
            .store(snapshot.interrupt_ns, Ordering::Relaxed);
        self.grant_end_ns
            .store(snapshot.grant_end_ns, Ordering::Relaxed);
        self.idle_ns.store(snapshot.idle_ns, Ordering::Relaxed);
        self.sequence.store(end, Ordering::Release);
    }

    /// Read a consistent copy; see [`ThreadRecord::try_snapshot`].
    pub fn try_snapshot(&self, attempts: u32) -> Option<SchedulerSnapshot> {
        for _ in 0..attempts {
            let before = self.sequence.load(Ordering::Acquire);
            if before & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            let snapshot = SchedulerSnapshot {
                events: self.events.load(Ordering::Relaxed),
                timer_expirations: self.timer_expirations.load(Ordering::Relaxed),
                dispatcher_state: self.dispatcher_state.load(Ordering::Relaxed),
                flags: self.flags.load(Ordering::Relaxed),
                interrupt_ns: self.interrupt_ns.load(Ordering::Relaxed),
                grant_end_ns: self.grant_end_ns.load(Ordering::Relaxed),
                idle_ns: self.idle_ns.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.sequence.load(Ordering::Relaxed) == before {
                return Some(snapshot);
            }
        }
        None
    }
}

/// Whether `stride` is a valid table stride: a multiple of 64, at least one
/// record, and at most `frame_bytes`.
pub const fn valid_stride(stride: u32, frame_bytes: u64) -> bool {
    stride >= MIN_STRIDE
        && (stride as usize).is_multiple_of(RECORD_ALIGN)
        && stride as u64 <= frame_bytes
}

/// A scheduler-side view of one shared region: the mapped Frame, the
/// table's stride and the slots the region covers.
#[derive(Copy, Clone, Debug)]
pub struct RecordRegion {
    base: *const u8,
    stride: u32,
    first_slot: u32,
    slots: u32,
}

impl RecordRegion {
    /// A region of `frame_bytes` at `base` covering slots from `first_slot`
    /// at `stride`. `None` if the stride is not valid or `base` is not
    /// 64-byte aligned.
    pub fn new(base: *const u8, frame_bytes: u64, stride: u32, first_slot: u32) -> Option<Self> {
        if !valid_stride(stride, frame_bytes) || !(base as usize).is_multiple_of(RECORD_ALIGN) {
            return None;
        }
        let slots = u32::try_from(frame_bytes / u64::from(stride)).ok()?;
        first_slot.checked_add(slots)?;
        Some(Self {
            base,
            stride,
            first_slot,
            slots,
        })
    }

    /// The slots this region covers, as a half-open range.
    pub fn slots(&self) -> core::ops::Range<u32> {
        self.first_slot..self.first_slot + self.slots
    }

    fn address(&self, slot: u32) -> Option<*const u8> {
        let index = slot
            .checked_sub(self.first_slot)
            .filter(|&i| i < self.slots)?;
        // SAFETY: `index < slots`, so the offset stays inside the Frame.
        Some(unsafe { self.base.add(index as usize * self.stride as usize) })
    }

    /// The scheduler record, if this region holds slot 0.
    ///
    /// # Safety
    /// The region's memory must be mapped and shared with the nucleus.
    pub unsafe fn scheduler_record<'a>(&self) -> Option<&'a SchedulerRecord> {
        let address = self.address(SCHEDULER_SLOT)?;
        // SAFETY: the caller's contract; the address is in the region and
        // aligned by construction.
        Some(unsafe { SchedulerRecord::at(address) })
    }

    /// The record of `slot`, if this region covers it and it is a Thread
    /// slot.
    ///
    /// # Safety
    /// As [`Self::scheduler_record`].
    pub unsafe fn thread_record<'a>(&self, slot: u32) -> Option<&'a ThreadRecord> {
        if slot == SCHEDULER_SLOT {
            return None;
        }
        let address = self.address(slot)?;
        // SAFETY: as above.
        Some(unsafe { ThreadRecord::at(address) })
    }
}

#[cfg(test)]
#[path = "../tests/support/scheduler_record.rs"]
mod tests;
