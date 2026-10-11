#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! irq-test: interrupt delivery to an EL0 receiver Thread, end to end.
//!
//! The boot Thread (the builder) issues an `IRQHandler` for the CPU's
//! virtual timer through the boot `IRQControl`, retypes a receiver Thread
//! with its own Time, binds the line to the receiver's badged Notification
//! and starts the `irq-receiver` component (see `irq-protocol`). Each round
//! it arms the virtual timer and waits for the receiver's report; the
//! receiver was woken by the interrupt, reports the delivered bits, and acks
//! the line once the builder has quieted the timer. The line is masked from
//! each delivery until that Ack, so the level-triggered timer cannot storm.
//!
//! It also checks the refusals: an issued line and the nucleus timer line
//! are `IrqLineActive`, an unknown controller and an unbadged Notification
//! `InvalidOperation`, and a retired handler's keys `ObjectRetired`.
//!
//! The timer interrupts and their controller come from the device tree.
//! Runs on raspi3b (BCM2836 local controller) and raspi4b (GIC-400);
//! in-guest assertions and the QEMU semihosting exit status are the result
//! (`just test-irq`, `just test-irq-rpi4`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{CNTFRQ_EL0, CNTV_CTL_EL0, CNTV_TVAL_EL0, Readable, Writeable},
    irq_protocol::{INIT_VA, IRQ_BADGE, ROUNDS, ReceiverInit, STACK_REGION},
    kickstart::{
        bootstrap::{
            PoolCapacities, bootstrap_nucleus, interrupt_controllers, retained_init_memory,
            spawn_idle,
        },
        kickstart_init_el2,
    },
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::{Component, ttbr},
        keys::{BOOT_TABLE_GUARD, SlotCursor, boot_key},
        loader::{verify_component, write_init},
        paging::image_table_count,
    },
    libdevicetree::{
        interrupts::{TimerInterrupt, root_interrupt_controller, timer_interrupt},
        is_enabled, is_interrupt_controller, node_by_phandle, phandle,
    },
    libkicktest::devices,
    libobject::{
        CapError, InconsistencyReason, IrqControlKey, IrqHandlerKey, KeySlot, KeyTableKey,
        NotificationKey, ObjectType, RawKey, Rights, TimeKey, UntypedKey,
        address_space::AddressSpaceKey,
        scheduler::{SchedulerKey, ThreadStart},
        time::DEFAULT_PRIORITY,
    },
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// Guard of the receiver component's `KeyTable`.
const RECEIVER_GUARD: u32 = 0x1A_0001;

/// Pages of the receiver's stack.
const STACK_PAGES: u32 = 4;

/// Budget the receiver's Time gets from the boot Time.
const RECEIVER_BUDGET_NS: u64 = 1_000_000_000;

/// How far ahead each round arms the virtual timer.
const TIMER_DELAY_US: u64 = 500;

/// Arm the virtual timer to fire once, [`TIMER_DELAY_US`] from now.
fn arm_virtual_timer() {
    let ticks = CNTFRQ_EL0.get() * TIMER_DELAY_US / 1_000_000;
    CNTV_TVAL_EL0.set(ticks.max(1));
    CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::SET + CNTV_CTL_EL0::IMASK::CLEAR);
}

/// Stop the virtual timer: its level-triggered interrupt drops.
fn quiet_virtual_timer() {
    CNTV_CTL_EL0.write(CNTV_CTL_EL0::ENABLE::CLEAR);
}

/// Install `entry` in `component`'s table at `slot`; the component-local key.
fn grant(component: &Component, entry: KeyEntry, slot: u32) -> RawKey {
    // SAFETY: the component's retained initialized table, exclusively
    // borrowed for this bootstrap grant; no capability invocation overlaps it.
    unsafe { &mut *(component.table_addr as *mut KeyTable) }
        .insert(KeySlot(slot), entry, component.guard)
        .unwrap_or_else(|failure| panic!("grant failed: {:?}", failure.error.code()))
}

/// Install `entry` in the boot table at `slot`; the boot-table key.
fn install_boot(keytable_addr: u64, entry: KeyEntry, slot: u32) -> RawKey {
    // SAFETY: the live boot table, borrowed for this bootstrap grant only.
    unsafe { &mut *(keytable_addr as *mut KeyTable) }
        .insert(KeySlot(slot), entry, BOOT_TABLE_GUARD)
        .unwrap_or_else(|failure| panic!("boot install failed: {:?}", failure.error.code()))
}

fn is_retired(error: &CapError) -> bool {
    matches!(
        error,
        CapError::InconsistentKey {
            reason: InconsistencyReason::ObjectRetired,
            ..
        }
    )
}

pub fn run() -> ! {
    semi::println!("irq-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder, receiver
        address_spaces: 2,
        // builder: root/L1/L2 and image L3s; receiver: root/L1/L2, image
        // span, stack region, init page
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

    // Kickstart recorded the controllers Issue specifiers are relative to:
    // the root, the timer's interrupt-parent, first; each one an enabled
    // interrupt controller of the device tree.
    let device_tree = devices::device_tree();
    let tree = device_tree.index();
    let controllers = interrupt_controllers();
    semi::println!(
        "irq-test: interrupt controllers by phandle {:?}",
        controllers.phandles()
    );
    let root = root_interrupt_controller(tree).expect("the device tree has a root controller");
    assert_eq!(controllers.phandles().first().copied(), phandle(&root));
    for &recorded in controllers.phandles() {
        let node = node_by_phandle(tree, recorded).expect("a recorded controller is in the tree");
        assert!(is_interrupt_controller(&node) && is_enabled(&node));
    }
    // The timer interrupts, as the device tree routes them.
    let virtual_timer =
        timer_interrupt(tree, TimerInterrupt::Virtual).expect("the timer has a virtual interrupt");
    let physical_timer = timer_interrupt(tree, TimerInterrupt::NonSecurePhysical)
        .expect("the timer has a physical interrupt");
    let timer_controller = controllers
        .index_of(virtual_timer.controller)
        .expect("the timer's controller is driven");
    assert_eq!(timer_controller, 0, "the timer is wired to the root");
    semi::println!(
        "irq-test: virtual timer {:?}, physical timer {:?} on controller {timer_controller}",
        virtual_timer.cells(),
        physical_timer.cells()
    );

    // ── The builder's own AddressSpace ────────────────────────────────────
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

    // ── Issue the virtual timer's line ────────────────────────────────────
    let irq_control = IrqControlKey::from_key(boot.irq_control_key);
    let issued = irq_control
        .issue(
            &self_table,
            slots.take(1),
            timer_controller,
            virtual_timer.cells(),
        )
        .unwrap_or_else(|error| panic!("IRQControl.Issue failed: {:?}", error.code()));
    semi::println!("irq-test: the virtual timer is line {}", issued.line);
    let handler = IrqHandlerKey::from_key(issued.handler);
    // One live handler per line; the nucleus keeps its timer line.
    assert!(matches!(
        irq_control.issue(&self_table, slots.take(1), timer_controller, virtual_timer.cells()),
        Err(CapError::IrqLineActive { line }) if line == issued.line
    ));
    assert!(matches!(
        irq_control.issue(&self_table, slots.take(1), timer_controller, physical_timer.cells()),
        Err(CapError::IrqLineActive { line }) if line != issued.line
    ));
    // A controller index past the recorded ones.
    let unknown_controller = u8::try_from(controllers.count).expect("few controllers");
    assert!(matches!(
        irq_control.issue(
            &self_table,
            slots.take(1),
            unknown_controller,
            virtual_timer.cells()
        ),
        Err(CapError::InvalidOperation)
    ));

    // ── The receiver: component, Thread and its own Time ─────────────────
    let receiver = builder.component(nucleus, RECEIVER_GUARD, &mut slots);
    builder.load_component(
        &receiver,
        &components::RECEIVER,
        RECEIVER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, receiver.address_space), &components::RECEIVER);
    let mut stacks = builder.stack_region(&receiver, STACK_REGION, &mut slots);
    let stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);

    let mut retype = |kind: ObjectType| {
        untyped
            .retype(kind, 0, 0, 1, &self_table, slots.take(1), Rights::all())
            .unwrap_or_else(|error| panic!("{kind:?} Retype failed: {:?}", error.code()))
    };
    let irq = retype(ObjectType::NOTIFICATION);
    let report = retype(ObjectType::NOTIFICATION);
    let resume = retype(ObjectType::NOTIFICATION);
    let receiver_thread = retype(ObjectType::THREAD);
    let receiver_time = retype(ObjectType::TIME);
    TimeKey::from_key(boot.boot_time_key)
        .transfer(
            &TimeKey::from_key(receiver_time),
            RECEIVER_BUDGET_NS,
            DEFAULT_PRIORITY,
        )
        .unwrap_or_else(|error| panic!("Time.Transfer failed: {:?}", error.code()));
    let root_scheduler = SchedulerKey::from_key(boot.root_scheduler_key);
    root_scheduler
        .configure_thread(
            receiver_thread,
            receiver.address_space_key,
            ThreadStart {
                entry: components::RECEIVER
                    .entry
                    .expect("the receiver has an entry"),
                stack_pointer: stack.top,
                argument: 0,
            },
            receiver_time,
        )
        .unwrap_or_else(|error| panic!("ConfigureThread failed: {:?}", error.code()));

    // ── Bind the line to the receiver's badged Notification ──────────────
    // An unbadged Notification has no bits to signal.
    assert!(matches!(
        handler.set_notification(irq, receiver_thread),
        Err(CapError::InvalidOperation)
    ));
    let irq_id = builder
        .entry(irq)
        .carved_id()
        .unwrap_or_else(|error| panic!("Notification identity: {:?}", error.code()));
    let badged_irq = install_boot(
        keytable_addr,
        KeyEntry::new_carved(irq_id, Rights::all(), IRQ_BADGE),
        slots.take(1),
    );
    handler
        .set_notification(badged_irq, receiver_thread)
        .unwrap_or_else(|error| panic!("SetNotification failed: {:?}", error.code()));

    // ── Hand the receiver its keys and start it ───────────────────────────
    let init_page = builder.private_pages(
        receiver.address_space_key,
        receiver.l2,
        INIT_VA,
        1,
        &mut slots,
    );
    write_init(
        init_page,
        ReceiverInit {
            irq: grant(&receiver, builder.entry(irq), 5).to_wire(),
            handler: grant(&receiver, builder.entry(issued.handler), 6).to_wire(),
            report: grant(&receiver, builder.entry(report), 7).to_wire(),
            resume: grant(&receiver, builder.entry(resume), 8).to_wire(),
        },
    );
    let _idle = spawn_idle(
        nucleus,
        keytable_addr,
        RawKey::from_wire(untyped.to_wire()),
        boot_as_id,
    );
    root_scheduler
        .start_thread(receiver_thread)
        .unwrap_or_else(|error| panic!("StartThread failed: {:?}", error.code()));

    // ── Rounds: arm, get the report, quiet the timer, let it ack ─────────
    let (report, resume) = (
        NotificationKey::from_key(report),
        NotificationKey::from_key(resume),
    );
    for round in 1..=ROUNDS {
        arm_virtual_timer();
        let bits = report
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("report Wait failed: {:?}", error.code()));
        assert_eq!(
            bits,
            u64::from(IRQ_BADGE),
            "round {round} delivered {bits:#x}"
        );
        quiet_virtual_timer();
        semi::println!("irq-test: round {round}: the receiver got {bits:#x}");
        resume
            .signal(1)
            .unwrap_or_else(|error| panic!("resume Signal failed: {:?}", error.code()));
    }

    // ── Retire: every copy fails, the line is free again ──────────────────
    handler
        .retire()
        .unwrap_or_else(|error| panic!("IRQHandler.Retire failed: {:?}", error.code()));
    let error = handler.ack().map(|()| 0).unwrap_err();
    assert!(is_retired(&error), "a retired handler key still works");
    let reissued = irq_control
        .issue(
            &self_table,
            slots.take(1),
            timer_controller,
            virtual_timer.cells(),
        )
        .unwrap_or_else(|error| panic!("reissue failed: {:?}", error.code()));
    assert_eq!(reissued.line, issued.line);

    semi::println!("irq-test: ✅ issue, binding, repeated delivery and retirement checks passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
