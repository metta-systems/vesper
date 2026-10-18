//! Fixture Threads queued runnable by the bootstrap builder.
//!
//! Each Thread is carved from the boot Untyped kernel-privately (see
//! `libcomponent_loader::carve`); the fixture execution
//! contexts (`EL1t`, a chosen entry and stack) have no public creation ABI.

use {
    libcomponent_loader::builder::Builder,
    libexception::arch::aarch64::SavedContext,
    nucleus::objects::{
        ArchObjectsImpl, ExecutionContext, Nucleus, Thread, arch_objects::AddressSpaceId,
        thread::ThreadId,
    },
};

/// Carve a Thread in `address_space` that first runs `saved`, and queue it
/// runnable.
fn spawn_with(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    address_space: AddressSpaceId,
    saved: &SavedContext,
) -> ThreadId {
    let thread = builder.carve_object(Thread::new(
        address_space,
        ExecutionContext::NotStarted { saved: *saved },
    ));
    // A Thread of the root Scheduler, run by the fixture dispatcher.
    crate::dispatcher::register(nucleus, builder, thread);
    thread
}

/// Carve a Thread in `address_space` that starts at `entry` as `EL1t` on
/// `stack_top`, and queue it runnable. It first runs when the current Thread
/// blocks.
pub fn spawn(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    address_space: AddressSpaceId,
    entry: u64,
    stack_top: u64,
) -> ThreadId {
    spawn_with(
        nucleus,
        builder,
        address_space,
        &SavedContext::el1t(entry, stack_top),
    )
}

/// Like [`spawn`], but the Thread takes interrupts, so the nucleus preempts
/// it on every tick instead of letting it run until it blocks.
pub fn spawn_interruptible(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    address_space: AddressSpaceId,
    entry: u64,
    stack_top: u64,
) -> ThreadId {
    spawn_with(
        nucleus,
        builder,
        address_space,
        &SavedContext::el1t_interruptible(entry, stack_top),
    )
}

/// Like [`spawn`], but the Thread starts unprivileged at `EL0t`, with
/// `argument` in `x0`. `entry` and `stack_top` must be EL0-accessible in
/// `address_space`.
pub fn spawn_el0(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    address_space: AddressSpaceId,
    entry: u64,
    stack_top: u64,
    argument: u64,
) -> ThreadId {
    spawn_with(
        nucleus,
        builder,
        address_space,
        &SavedContext::el0(entry, stack_top, argument),
    )
}
