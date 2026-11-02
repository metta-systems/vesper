#![no_std]

//! What irq-test's builder and its EL0 interrupt receiver agree on.
//!
//! The builder issues an `IRQHandler` for the virtual timer's line, binds it
//! to the receiver's interrupt Notification with [`IRQ_BADGE`], and hands
//! the receiver its keys through the init page. The receiver enables the
//! line, then for each interrupt reports the delivered bits to the builder,
//! waits until the builder has quieted the timer and acks the line again.

/// The receiver's init page (written by the builder before it runs).
pub const INIT_VA: u64 = 0x2000_0000;
/// The 2 MiB span holding the receiver's guarded stack.
pub const STACK_REGION: u64 = 0x3000_0000;

/// The badge of the receiver's interrupt Notification: the bits each
/// interrupt signals.
pub const IRQ_BADGE: u64 = 0x10;

/// Interrupts the receiver handles before it parks.
pub const ROUNDS: u64 = 3;

/// What the builder hands the receiver, as wire keys in its own table.
#[repr(C)]
pub struct ReceiverInit {
    /// Signalled by the nucleus on each interrupt; the receiver waits on it.
    pub irq: u64,
    /// The `IRQHandler` the receiver acks.
    pub handler: u64,
    /// The receiver reports each delivered bitmap to the builder here.
    pub report: u64,
    /// The builder signals here once the timer is quiet.
    pub resume: u64,
}

/// The receiver's init page.
///
/// # Safety
///
/// Only in the receiver, whose builder maps and fills a [`ReceiverInit`] at
/// [`INIT_VA`] before it runs and never changes it afterwards.
pub unsafe fn receiver_init() -> &'static ReceiverInit {
    // SAFETY: forwarded caller contract.
    unsafe { &*(INIT_VA as *const ReceiverInit) }
}
