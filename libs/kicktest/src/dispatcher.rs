//! The fixture dispatcher: the root Scheduler's dispatcher for test kernels.
//!
//! Fixture Threads (the boot Thread included) are Threads of the root
//! Scheduler, so the test kernels run on the production dispatch path. The
//! dispatcher here is an `EL1t` Thread using `Scheduler.Dispatch`. Its
//! policy needs no scheduler records: it tries each registered Thread in
//! turn for one slice and skips the ones `Dispatch` refuses (blocked, not
//! started, retired), and when a whole pass ran nothing it waits with
//! nothing dispatched, which a runnable Thread wakes. A Thread that never
//! takes interrupts runs until it blocks, as it did under the kernel queue.
//!
//! [`init`] must run before the first fixture Thread is created with
//! [`crate::threads`].

use {
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    libcomponent_loader::{builder::Builder, keys::BOOT_TABLE_GUARD},
    libexception::arch::aarch64::SavedContext,
    libobject::{
        KeySlot, ObjectType, RawKey, Rights,
        scheduler::{SchedulerKey, activation_mask},
    },
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjectsImpl, ExecutionContext, KeyTable, Nucleus, Thread,
            arch_objects::AddressSpaceId, carved::CarvedId, thread::ThreadId,
        },
    },
};

/// Fixture Threads one test kernel can register.
const MAX_FIXTURES: usize = 24;

/// How long a fixture runs before the dispatcher decides again: ten
/// milliseconds, the old kernel slice.
const SLICE_NS: u64 = 10_000_000;

/// The first boot-table slot the fixture keys take; the test kernels' own
/// slots stay below it.
const FIRST_KEY_SLOT: u32 = 400;

const STACK_BYTES: usize = 8 * 1024;

#[repr(C, align(16))]
struct Stack(UnsafeCell<[u8; STACK_BYTES]>);

// SAFETY: only the dispatcher Thread uses the stack, through its SP.
unsafe impl Sync for Stack {}

static STACK: Stack = Stack(UnsafeCell::new([0; STACK_BYTES]));

/// Wire keys of the registered Threads, in registration order.
static KEYS: [AtomicU64; MAX_FIXTURES] = [const { AtomicU64::new(0) }; MAX_FIXTURES];
static COUNT: AtomicUsize = AtomicUsize::new(0);
/// The registered Threads' identities, to read their current generation: a
/// Thread with a record has one, and `Dispatch` wants it.
static ADDRESSES: [AtomicU64; MAX_FIXTURES] = [const { AtomicU64::new(0) }; MAX_FIXTURES];
static GENERATIONS: [AtomicU32; MAX_FIXTURES] = [const { AtomicU32::new(0) }; MAX_FIXTURES];
static NUCLEUS: AtomicPtr<Nucleus<ArchObjectsImpl>> = AtomicPtr::new(core::ptr::null_mut());
static NEXT_SLOT: AtomicU32 = AtomicU32::new(FIRST_KEY_SLOT);

/// The root Scheduler: its wire key (for the dispatcher) and its identity
/// (for registering Threads).
static SCHEDULER_KEY: AtomicU64 = AtomicU64::new(0);
static SCHEDULER_ADDRESS: AtomicU64 = AtomicU64::new(0);
static SCHEDULER_GENERATION: AtomicU32 = AtomicU32::new(0);

fn scheduler_id() -> CarvedId {
    let address = SCHEDULER_ADDRESS.load(Ordering::Acquire);
    assert_ne!(address, 0, "the fixture dispatcher is not initialized");
    CarvedId {
        kind: ObjectType::SCHEDULER,
        address,
        generation: SCHEDULER_GENERATION.load(Ordering::Acquire),
    }
}

/// The current generation of the registered Thread `index`: the Thread's
/// state-change count, which moves once it has a scheduler record. The
/// dispatcher runs alone on the CPU, so the read cannot race a change.
fn generation_of(index: usize) -> u64 {
    let nucleus = NUCLEUS.load(Ordering::Acquire);
    let id = ThreadId {
        kind: ObjectType::THREAD,
        address: ADDRESSES[index].load(Ordering::Acquire),
        generation: GENERATIONS[index].load(Ordering::Acquire),
    };
    // SAFETY: the Nucleus outlives every fixture Thread, and the dispatcher
    // is the only Thread running while it reads.
    unsafe { (*nucleus).thread(id) }.map_or(0, |thread| thread.generation)
}

extern "C" fn dispatcher() -> ! {
    let scheduler =
        SchedulerKey::from_key(RawKey::from_wire(SCHEDULER_KEY.load(Ordering::Acquire)));
    // One pass: try every registered Thread for a slice; whether any ran.
    let pass = || {
        let mut ran = false;
        for (index, key) in KEYS.iter().enumerate().take(COUNT.load(Ordering::Acquire)) {
            let key = RawKey::from_wire(key.load(Ordering::Acquire));
            if scheduler
                .dispatch(Some(key), SLICE_NS, generation_of(index))
                .is_ok()
            {
                ran = true;
            }
        }
        ran
    };
    loop {
        // A running Thread is not preempted by a wakeup: the mask is empty.
        scheduler.configure_activation(0).ok();
        if pass() {
            continue;
        }
        // Nothing ran. Before waiting, record wakeups (the mask asks for
        // them) and look once more, so a Thread that became runnable since
        // the pass is not missed: the wait would otherwise never end.
        scheduler.configure_activation(activation_mask::ALL).ok();
        if pass() {
            continue;
        }
        scheduler.dispatch(None, u64::MAX, 0).ok();
    }
}

/// Create the root Scheduler's dispatcher in `address_space` (which must map
/// the boot image), bind it, and register the current (boot) Thread. The
/// dispatcher first runs when the boot Thread blocks or is preempted.
pub fn init(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    address_space: AddressSpaceId,
    root_scheduler: RawKey,
) {
    let scheduler = builder
        .entry(root_scheduler)
        .carved_id()
        .unwrap_or_else(|error| panic!("root Scheduler identity: {:?}", error.code()));
    NUCLEUS.store(core::ptr::from_mut(nucleus), Ordering::Release);
    SCHEDULER_KEY.store(root_scheduler.to_wire(), Ordering::Release);
    SCHEDULER_ADDRESS.store(scheduler.address, Ordering::Release);
    SCHEDULER_GENERATION.store(scheduler.generation, Ordering::Release);

    let stack_top = STACK.0.get() as u64 + STACK_BYTES as u64;
    let thread = builder.carve_object(Thread::new(
        address_space,
        ExecutionContext::Configured {
            saved: SavedContext::el1t(dispatcher as *const () as u64, stack_top),
        },
    ));
    nucleus
        .thread_mut(thread)
        .expect("the dispatcher Thread exists")
        .scheduler = Some(scheduler);
    nucleus.join_scheduler(thread, scheduler);
    nucleus
        .bind_dispatcher(scheduler, thread)
        .unwrap_or_else(|error| panic!("fixture BindDispatcher failed: {:?}", error.code()));

    let boot = nucleus.current_thread.expect("a boot Thread is running");
    register(nucleus, builder, boot);
}

/// Make `thread` a Thread of the root Scheduler and add it to the
/// dispatcher's round. A Thread that can run is announced as for any wakeup.
pub fn register(nucleus: &mut Nucleus<ArchObjectsImpl>, builder: &Builder<'_>, thread: ThreadId) {
    let scheduler = scheduler_id();
    // A Thread made through the public path already belongs to the root.
    if nucleus
        .thread(thread)
        .expect("the fixture Thread exists")
        .scheduler
        != Some(scheduler)
    {
        nucleus
            .thread_mut(thread)
            .expect("the fixture Thread exists")
            .scheduler = Some(scheduler);
        nucleus.join_scheduler(thread, scheduler);
    }

    let slot = NEXT_SLOT.fetch_add(1, Ordering::Relaxed);
    assert!(slot < 1 << 9, "boot-table slots exhausted");
    // SAFETY: the live boot table, borrowed for this bootstrap grant only.
    let key = unsafe { &mut *KeyTable::at_carve(builder.boot_table_addr) }
        .insert(
            KeySlot(slot),
            KeyEntry::new_thread(thread, Rights::all(), 0),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| panic!("fixture key grant failed: {:?}", failure.error.code()));
    let index = COUNT.load(Ordering::Acquire);
    assert!(index < MAX_FIXTURES, "too many fixture Threads");
    KEYS[index].store(key.to_wire(), Ordering::Release);
    ADDRESSES[index].store(thread.address, Ordering::Release);
    GENERATIONS[index].store(thread.generation, Ordering::Release);
    COUNT.store(index + 1, Ordering::Release);

    // A Thread that is not the one running is runnable now (the boot Thread
    // runs, and is dispatched when it next blocks).
    if nucleus.current_thread != Some(thread) {
        nucleus.make_runnable(thread);
    }
}

/// Add a Thread made through the public path (`ConfigureThread` and
/// `StartThread` on the root Scheduler) to the dispatcher's round. `key` is
/// the builder's boot-table key to the Thread.
pub fn register_key(nucleus: &mut Nucleus<ArchObjectsImpl>, builder: &Builder<'_>, key: RawKey) {
    let thread = builder
        .entry(key)
        .thread_id()
        .unwrap_or_else(|error| panic!("fixture Thread identity: {:?}", error.code()));
    register(nucleus, builder, thread);
}
