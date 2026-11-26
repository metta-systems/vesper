//! The per-core record (SMP design, "Per-core record", C3(a)).
//!
//! Kickstart carves one record per core from accounted memory, sized from the device
//! tree's `/cpus`, and points each core's `TPIDR_EL1` at its own record (an EL1-only
//! register: no component sees it). The nucleus names its core by the record's logical
//! index, never by `MPIDR_EL1`: the Raspberry Pi 5 numbers its cores in Aff1.
//!
//! The record holds the core's identity, its trap stack and its [`CoreState`]: the
//! scheduling state of what runs on it and its totals. The [`Nucleus`](super::Nucleus)
//! reaches the running core's state through [`ThisCore`].

use {
    super::{nucleus::Dispatched, thread::ThreadId},
    aarch64_cpu::registers::{Read, TPIDR_EL1},
    core::{
        cell::UnsafeCell,
        ops::{Deref, DerefMut},
        sync::atomic::{AtomicBool, AtomicU64},
    },
    libobject::scheduler_record::FpTotals,
};

/// One core's record. Kickstart writes it before the core first runs at EL1; afterwards
/// its identity and stack stay as written, its [`state`](Self::state) changes by its own
/// core under the kernel lock, and its atomics by its own core without the lock.
#[repr(C, align(64))]
#[derive(Debug)]
pub struct CoreRecord {
    /// The core's logical index: its position among the CPUs of `/cpus` (SMP C9(a))
    pub index: u32,
    /// Reserved, zero
    pub reserved: u32,
    /// The core's `MPIDR_EL1` affinity bits (`libcpu::smp::CoreAffinity`)
    pub affinity: u64,
    /// Kernel virtual address of the top of the core's `SP_EL1` trap stack
    pub trap_stack_top: u64,
    /// What runs on the core, and its totals
    pub state: UnsafeCell<CoreState>,
    /// Whether the core waits for an interrupt in the kernel's idle loop, the one place
    /// kernel code is interruptible. Only its own core touches it.
    pub idling: AtomicBool,
    /// The timer deadline last written to the core's `CNTP_*_EL0`, or
    /// [`NO_DEADLINE`]: an unchanged deadline needs no register writes. Only its own
    /// core touches it.
    pub programmed_deadline: AtomicU64,
    /// Whether the frame the current kernel entry returns to is an FP frame (the
    /// `fp_entry_trap` guard). Only its own core touches it.
    pub fp_exit_open: AtomicBool,
    /// `FPEN` writes the kernel-entry FP guard made on this core, at entries and exits.
    pub fp_entry_toggles: AtomicU64,
}

/// [`CoreRecord::programmed_deadline`] with no deadline programmed.
pub const NO_DEADLINE: u64 = u64::MAX;

impl CoreRecord {
    /// The record of a core that has run nothing yet.
    pub const fn new(index: u32, affinity: u64, trap_stack_top: u64) -> Self {
        Self {
            index,
            reserved: 0,
            affinity,
            trap_stack_top,
            state: UnsafeCell::new(CoreState::new()),
            idling: AtomicBool::new(false),
            programmed_deadline: AtomicU64::new(NO_DEADLINE),
            fp_exit_open: AtomicBool::new(false),
            fp_entry_toggles: AtomicU64::new(0),
        }
    }
}

/// The record of the core this runs on.
///
/// # Panics
///
/// If `TPIDR_EL1` names no record: Kickstart sets it before the core runs at EL1.
pub fn current() -> &'static CoreRecord {
    let address = TPIDR_EL1.get();
    #[cfg(test)]
    if address == 0 {
        return test_record();
    }
    assert_ne!(address, 0, "TPIDR_EL1 names no per-core record");
    // SAFETY: Kickstart points `TPIDR_EL1` at this core's record, in memory it carved
    // for the nucleus and mapped in the kernel half; the record's identity fields do not
    // change after the core starts, and its state is behind an `UnsafeCell`.
    unsafe { &*(address as *const CoreRecord) }
}

/// The record of the nucleus test kernels, which run on one core with no Kickstart and
/// so no `TPIDR_EL1` record.
#[cfg(test)]
fn test_record() -> &'static CoreRecord {
    /// The test kernels' one record.
    struct TestRecord(CoreRecord);
    // SAFETY: the nucleus tests run on one core, one test at a time.
    unsafe impl Sync for TestRecord {}
    static RECORD: TestRecord = TestRecord(CoreRecord::new(0, 0, 0));
    &RECORD.0
}

/// One core's scheduling state and totals, in its [`CoreRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreState {
    /// Currently running thread, as a checked identity: every read validates
    /// its generation against the Thread's header, so a retired Thread is
    /// never mistaken for the current Thread.
    pub current_thread: Option<ThreadId>, // FIXME: not option, always something (Idle or other)
    /// When the current Thread was last made current, in counter ticks on the
    /// [`Nucleus::counter_ticks`](super::Nucleus::counter_ticks) timeline. The next
    /// switch away or tick charges the elapsed ticks to that Thread and its bound Time.
    /// Starts at zero, so the boot Thread is charged from counter start.
    pub current_since_ticks: u64,
    /// The Thread running under a dispatcher's `Dispatch`, and the
    /// dispatcher's limit for it.
    pub dispatched: Option<Dispatched>,
    /// The dispatch the dispatcher just asked for, taken when it parks.
    pub dispatch_next: Option<Dispatched>,
    /// An activation made the dispatcher runnable ahead of the Thread that
    /// is running: the timer fires at once so the nucleus switches.
    pub preempt_requested: bool,
    /// Counter ticks this core spent handling interrupts since boot, published to its
    /// root Scheduler's record as `interrupt_ns` (SMP C11(a)).
    pub interrupt_ticks: u64,
    /// Counter ticks this core spent with nothing to run, waiting in the kernel for an
    /// interrupt, since boot; published to its root Scheduler's record as `idle_ns`.
    /// Time spent handling interrupts while idle is counted as interrupt time, not idle
    /// time.
    pub idle_ticks: u64,
    /// This core's FP/SIMD totals, published in its root Scheduler's record.
    pub fp_totals: FpTotals,
}

impl CoreState {
    /// The state of a core that has run nothing yet.
    pub const fn new() -> Self {
        Self {
            current_thread: None,
            current_since_ticks: 0,
            dispatched: None,
            dispatch_next: None,
            preempt_requested: false,
            interrupt_ticks: 0,
            idle_ticks: 0,
            fp_totals: FpTotals::ZERO,
        }
    }
}

impl Default for CoreState {
    fn default() -> Self {
        Self::new()
    }
}

/// The [`Nucleus`](super::Nucleus)'s handle to the [`CoreState`] of the core running
/// it: the record `TPIDR_EL1` names, or a fixture's state in a test.
///
/// A field of its own, so that a borrow of the core state is disjoint from the borrows
/// of the Nucleus's other fields.
#[derive(Debug)]
pub struct ThisCore(*mut CoreState);

impl ThisCore {
    /// The state in the running core's record, through `TPIDR_EL1`.
    pub const fn running() -> Self {
        Self(core::ptr::null_mut())
    }

    /// A fixture's own state, for a test kernel that runs no Kickstart.
    ///
    /// # Safety
    ///
    /// `state` stays valid, and is reached only through this handle, for as long as the
    /// Nucleus that holds it is used.
    pub const unsafe fn fixture(state: *mut CoreState) -> Self {
        Self(state)
    }

    fn pointer(&self) -> *mut CoreState {
        if self.0.is_null() {
            current().state.get()
        } else {
            self.0
        }
    }
}

impl Deref for ThisCore {
    type Target = CoreState;

    fn deref(&self) -> &CoreState {
        // SAFETY: the running core's state is touched only by that core, through the
        // one Nucleus it holds under the kernel lock (or a fixture's state, by the
        // `fixture` contract); this borrow of the handle bounds the reference.
        unsafe { &*self.pointer() }
    }
}

impl DerefMut for ThisCore {
    fn deref_mut(&mut self) -> &mut CoreState {
        // SAFETY: as for `deref`; the exclusive borrow of the handle makes the
        // reference unique.
        unsafe { &mut *self.pointer() }
    }
}
