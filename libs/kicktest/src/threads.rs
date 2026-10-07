//! Fixture Threads queued runnable by the bootstrap builder.

use {
    libexception::arch::aarch64::SavedContext,
    nucleus::objects::{
        ArchObjectsImpl, ExecutionContext, InvocationStack, Nucleus, Thread, access::ObjectId,
    },
};

/// Allocate a Thread in `address_space` that starts at `entry` as `EL1t` on
/// `stack_top`, and queue it runnable. It first runs when the current Thread
/// blocks. Thread creation has no public ABI yet, so the builder does this
/// kernel-privately.
pub fn spawn(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    address_space: ObjectId,
    entry: u64,
    stack_top: u64,
) -> ObjectId {
    let (thread, _) = nucleus
        .pools
        .threads
        .allocate(Thread {
            address_space,
            context: ExecutionContext::NotStarted {
                saved: SavedContext::el1t(entry, stack_top),
            },
            invocation_stack: InvocationStack::new(),
            fault: None,
        })
        .expect("no fixture Thread slot");
    assert!(
        nucleus.scheduler.push(thread.index),
        "fixture Thread did not queue"
    );
    thread
}

/// Like [`spawn`], but the Thread starts unprivileged at `EL0t`, with
/// `argument` in `x0`. `entry` and `stack_top` must be EL0-accessible in
/// `address_space`.
pub fn spawn_el0(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    address_space: ObjectId,
    entry: u64,
    stack_top: u64,
    argument: u64,
) -> ObjectId {
    let (thread, _) = nucleus
        .pools
        .threads
        .allocate(Thread {
            address_space,
            context: ExecutionContext::NotStarted {
                saved: SavedContext::el0(entry, stack_top, argument),
            },
            invocation_stack: InvocationStack::new(),
            fault: None,
        })
        .expect("no fixture Thread slot");
    assert!(
        nucleus.scheduler.push(thread.index),
        "fixture Thread did not queue"
    );
    thread
}
