#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! preempt-test: interrupts and the kernel-owned tick end to end.
//!
//! Kickstart selects and loads the privileged interrupt-controller component
//! from the device tree; the nucleus installs it and arms the physical timer.
//! The boot Thread (non-interruptible `EL1t`) then starts:
//!
//! - two **`EL1t` spinners**: interruptible Threads that count forever and
//!   never block or yield;
//! - an **EL0 spinner**: the `preempt-spinner` component, in its own
//!   `AddressSpace`, counting forever in a page mapped for it only;
//! - a **checker**: an interruptible Thread that waits (busy, never blocking)
//!   until every spinner has counted across enough ticks, then signals the
//!   boot Thread's Notification;
//! - the **idle** Thread, selected only when nothing else is runnable.
//!
//! On one core, Threads that never yield can all make progress only through
//! preemption. The boot Thread parks on the Notification and checks the
//! counters, the tick count, the clock and the EL0 spinner's preempted
//! context once it is woken.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-preempt`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicPtr, AtomicU64, Ordering},
    },
    kickstart::{
        bootstrap::{PoolCapacities, bootstrap_nucleus, retained_init_memory, spawn_idle},
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libexception::arch::aarch64::ExceptionOrigin,
    // `libkicktest` supplies the panic handler.
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key, boot_slot},
        loader::verify_component,
        paging::image_table_count,
        threads,
    },
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::objects::{ArchObjectsImpl, ExecutionContext, Nucleus},
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Ticks both spinners must run across before the checker wakes the boot Thread.
const REQUIRED_TICKS: u64 = 8;

/// Boot `KeyTable` slot of the boot Thread's wake-up Notification.
const WAKE_SLOT: u32 = 16;

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// Guard of the EL0 spinner component's `KeyTable`.
const SPINNER_GUARD: u32 = 0x05_7101;

/// Where the EL0 spinner's counter page is mapped, in its `AddressSpace` only.
const COUNTER_VA: u64 = 0x2000_0000;

/// Where the EL0 spinner's guarded stacks are carved.
const STACK_REGION: u64 = 0x3000_0000;

/// Pages of the EL0 spinner's stack.
const STACK_PAGES: u32 = 4;

/// Bits the checker signals.
const WAKE_BITS: u64 = 0b1;

/// Per-spinner progress counters.
static SPINNER_COUNTS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// The live Nucleus, for the checker to read the tick count.
static NUCLEUS: AtomicPtr<Nucleus<ArchObjectsImpl>> = AtomicPtr::new(core::ptr::null_mut());

/// The wake-up Notification key (wire form), for the checker.
static WAKE_KEY: AtomicU64 = AtomicU64::new(0);

/// The EL0 spinner's counter, through the kernel's physical window.
static EL0_COUNT: AtomicPtr<u64> = AtomicPtr::new(core::ptr::null_mut());

/// The tick count when the Threads were started.
static START_TICKS: AtomicU64 = AtomicU64::new(0);

const STACK_BYTES: usize = 16 * 1024;

/// One execution stack per fixture Thread.
#[repr(C, align(16))]
struct Stack(UnsafeCell<[u8; STACK_BYTES]>);

// SAFETY: each stack is used only by its own Thread, through its SP.
unsafe impl Sync for Stack {}

impl Stack {
    const fn new() -> Self {
        Self(UnsafeCell::new([0; STACK_BYTES]))
    }

    fn top(&self) -> u64 {
        self.0.get() as u64 + STACK_BYTES as u64
    }
}

static STACKS: [Stack; 3] = [Stack::new(), Stack::new(), Stack::new()];

/// The kernel's tick count. The nucleus updates it under its lock on another
/// path; a volatile read of the word is all the checker needs.
fn ticks() -> u64 {
    let nucleus = NUCLEUS.load(Ordering::Acquire);
    // SAFETY: the Nucleus outlives every fixture Thread and is mapped for EL1.
    unsafe { core::ptr::read_volatile(&raw const (*nucleus).ticks) }
}

/// How far the EL0 spinner counted.
fn el0_count() -> u64 {
    // SAFETY: the counter page is mapped in the kernel's physical window and
    // outlives the test; only the EL0 spinner writes it.
    unsafe { core::ptr::read_volatile(EL0_COUNT.load(Ordering::Acquire)) }
}

fn spin(counter: &AtomicU64) -> ! {
    loop {
        counter.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

extern "C" fn spinner_a() -> ! {
    spin(&SPINNER_COUNTS[0])
}

extern "C" fn spinner_b() -> ! {
    spin(&SPINNER_COUNTS[1])
}

/// Wait (without blocking) until every spinner counted across
/// [`REQUIRED_TICKS`] ticks, then wake the boot Thread and idle.
extern "C" fn checker() -> ! {
    let start = START_TICKS.load(Ordering::Relaxed);
    while ticks() < start + REQUIRED_TICKS
        || el0_count() == 0
        || SPINNER_COUNTS
            .iter()
            .any(|count| count.load(Ordering::Relaxed) == 0)
    {
        core::hint::spin_loop();
    }
    let wake = NotificationKey::from_key(RawKey::from_wire(WAKE_KEY.load(Ordering::Acquire)));
    wake.signal(WAKE_BITS)
        .unwrap_or_else(|error| panic!("checker signal failed: {:?}", error.code()));
    loop {
        aarch64_cpu::asm::wfi();
    }
}

pub fn run() -> ! {
    semi::println!("preempt-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // boot, idle, two EL1t spinners and the checker in the builder's
        // AddressSpace; the EL0 spinner in its own
        threads: 6,
        // builder, EL0 spinner
        address_spaces: 2,
        // the boot Thread's wake-up
        notifications: 1,
        event_counts: 0,
        // builder: root/L1/L2 and image L3s; EL0 spinner: root/L1/L2, image
        // span, stack region, counter page
        page_tables: 3 + image_tables + 6,
        asid_pools: 1,
    });
    let (nucleus, keytable_addr, boot_as_id) = (boot.nucleus, boot.keytable_addr, boot.boot_as_id);
    let untyped = UntypedKey::from_key(boot.boot_untyped_key);
    let self_table = KeyTableKey::from_key(boot.self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained,
    };
    let mut slots = SlotCursor::starting_at(FIRST_FIXTURE_SLOT);

    // The builder's own AddressSpace maps the boot image (code, statics and
    // fixture stacks): every Thread here runs in it, so the scheduler can
    // select them with a real root and ASID.
    let builder_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let builder_l2 = builder.root_chain(builder_as_key, &mut slots);
    assert_eq!(Builder::assign_asid(builder_as_key), 1);
    builder.map_retained_image(
        &[ImageTarget {
            address_space: builder_as_key,
            l2: builder_l2,
            first_table_slot: slots.take(u32::try_from(image_tables).unwrap_or(u32::MAX)),
        }],
        &ImageArchive {
            slot: slots.take(1),
            guard: BUILDER_ARCHIVE_GUARD,
            grant_scratch: slots.take(1),
            copy_scratch: slots.take(1),
        },
    );
    verify_retained_image(&retained, &[ttbr(nucleus, boot_as_id)]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));

    let wake_key = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            KeySlot(WAKE_SLOT).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()));
    assert_eq!(wake_key.slot(), boot_slot(WAKE_SLOT));
    WAKE_KEY.store(wake_key.to_wire(), Ordering::Release);
    let wake = NotificationKey::from_key(wake_key);

    // ── The EL0 spinner: its own AddressSpace, image, stack and counter ───
    let spinner = builder.component(nucleus, SPINNER_GUARD, &mut slots);
    builder.load_component(
        &spinner,
        &components::SPINNER,
        SPINNER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, spinner.address_space), &components::SPINNER);
    assert_eq!(spinner.asid, 2);
    let mut stacks = builder.stack_region(&spinner, STACK_REGION, &mut slots);
    let stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);
    let counter_page = builder.private_pages(
        spinner.address_space_key,
        spinner.l2,
        COUNTER_VA,
        1,
        &mut slots,
    );
    EL0_COUNT.store(
        PhysAddr::new(counter_page)
            .user_to_kernel()
            .as_mut_ptr::<u64>(),
        Ordering::Release,
    );

    NUCLEUS.store(&raw mut *nucleus, Ordering::Release);
    START_TICKS.store(ticks(), Ordering::Relaxed);
    let time_before = nucleus.current_time_ns();

    let idle = spawn_idle(nucleus, boot_as_id);
    let entries = [
        spinner_a as *const () as u64,
        spinner_b as *const () as u64,
        checker as *const () as u64,
    ];
    for (entry, stack) in entries.into_iter().zip(&STACKS) {
        threads::spawn_interruptible(nucleus, boot_as_id, entry, stack.top());
    }
    let el0_spinner = threads::spawn_el0(
        nucleus,
        spinner.address_space,
        components::SPINNER.entry.expect("the spinner has an entry"),
        stack.top,
        COUNTER_VA,
    );
    semi::println!("preempt-test: spinners, checker and idle started; parking");

    // Park until the checker wakes us: only preemption lets it run.
    let bits = wake
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("Notification.Wait failed: {:?}", error.code()));
    assert_eq!(bits, WAKE_BITS);

    let elapsed_ticks = ticks() - START_TICKS.load(Ordering::Relaxed);
    let counts = SPINNER_COUNTS
        .each_ref()
        .map(|count| count.load(Ordering::Relaxed));
    let el0_counted = el0_count();
    let time_after = nucleus.current_time_ns();
    semi::println!(
        "preempt-test: woken after {elapsed_ticks} ticks, {} ms; EL1t spinner counts {counts:?}, EL0 spinner count {el0_counted}",
        (time_after - time_before) / 1_000_000
    );
    assert!(
        elapsed_ticks >= REQUIRED_TICKS,
        "too few ticks: {elapsed_ticks}"
    );
    assert!(
        counts.iter().all(|&count| count > 0),
        "a spinner never ran: {counts:?}"
    );
    assert!(time_after > time_before, "the clock did not advance");
    // At least the required ticks of real time passed (10 ms each).
    assert!(
        time_after - time_before >= REQUIRED_TICKS * 10_000_000,
        "the clock ran slower than the ticks"
    );
    assert!(el0_counted > 0, "the EL0 spinner never ran");
    assert_eq!(nucleus.idle_thread, Some(idle));

    // The EL0 spinner never blocks: it can only have stopped through a tick
    // taken at EL0, its complete user state saved for the next slice.
    let el0_thread = nucleus
        .pools
        .threads
        .get_live(usize::from(el0_spinner.index))
        .expect("EL0 spinner Thread missing");
    let ExecutionContext::Preempted { saved } = el0_thread.context else {
        panic!("the EL0 spinner is not preempted: {:?}", el0_thread.context);
    };
    assert_eq!(saved.origin, ExceptionOrigin::LowerAarch64);
    assert_eq!(saved.spsr_el1 & 0xf, 0, "the spinner must run as EL0t");
    assert_eq!(
        saved.gpr[0], COUNTER_VA,
        "the spinner's counter pointer survives"
    );
    assert_eq!(el0_thread.address_space, spinner.address_space);

    semi::println!("preempt-test: ✅ preemption, tick and clock checks passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
