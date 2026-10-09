#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fault-test: fault delivery to EL0 fault handlers.
//!
//! A fault is a synchronous upcall on the faulting Thread into the
//! `Invocation` at `KeySlot::FAULT_HANDLER` of the `AddressSpace` it faults
//! in. The `faulter` component has such a handler; its Threads (see
//! `fault-protocol`) exercise:
//!
//! - **skip**, **retry** and a **Return fault** (a Return with nothing to
//!   return to), each resumed by the handler;
//! - **terminate**: the handler ends its Thread, which is parked as faulted;
//! - **nested**: the handler faults itself — unhandled;
//! - **busy**: a fault while another Thread's handler is blocked — unhandled.
//!
//! The `bare` component has no handler, so its fault is unhandled. Each
//! unhandled fault parks its Thread as `Faulted` and counts on the faulting
//! `AddressSpace`; the kernel keeps running. A witness Thread queued after a
//! stopping Thread wakes the builder, which then checks the kernel's view.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-fault`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    fault_protocol::{
        BLOCKED_BIT, FaulterInit, INIT_VA, ROLE_BLOCKED, ROLE_NESTED, ROLE_SEQUENCE,
        ROLE_TERMINATE, ROLE_WHILE_BUSY, ROLE_WITNESS, SEQUENCE_BIT, STACK_REGION, WITNESS_BIT,
    },
    kickstart::{
        bootstrap::{
            BOOT_TABLE_SIZE_BITS, PoolCapacities, bootstrap_nucleus, retained_init_memory,
        },
        kickstart_init_el2,
    },
    libexception::arch::aarch64::ExceptionOrigin,
    libimage::ComponentImage,
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::{Component, ttbr},
        keys::{SlotCursor, boot_key},
        loader::{StackRegion, verify_component, write_init},
        paging::image_table_count,
        threads,
    },
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjectsImpl, ExecutionContext, KeyTable, Notification, Nucleus, access::ObjectId,
            arch_objects::AddressSpaceObject,
        },
    },
};

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const FAULTER_GUARD: u32 = 0x0F_A701;
const BARE_GUARD: u32 = 0x0B_A401;
/// Pages per stack (each also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
/// Headroom the fault handler's Invocation requires below its SP.
const MINIMUM_HEADROOM: u64 = 0x400;
/// Fixture boot-table slots start clear of the well-known bootstrap slots.
const FIRST_FIXTURE_SLOT: u32 = 64;

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Bootstrap grant of a Notification into `component`'s table at `slot`.
/// Notification is off the `CopyDerive` allowlist, so the builder installs
/// the component's capability kernel-privately.
fn grant_notification(component: &Component, object: ObjectId, slot: u32) -> RawKey {
    // SAFETY: the component's retained initialized table, exclusively
    // borrowed for this bootstrap grant; no capability invocation overlaps it.
    unsafe { &mut *(component.table_addr as *mut KeyTable) }
        .insert(
            KeySlot(slot),
            KeyEntry::new::<Notification>(object, Rights::all(), 0),
            component.guard,
        )
        .unwrap_or_else(|failure| panic!("Notification grant failed: {:?}", failure.error.code()))
}

/// Queue an EL0 Thread of `component` in `role`, on a fresh guarded stack.
fn spawn(
    builder: &Builder<'_>,
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    stacks: &mut StackRegion,
    slots: &mut SlotCursor,
    component: &Component,
    image: &ComponentImage,
    role: u64,
) -> ObjectId {
    let stack = builder.user_stack(stacks, STACK_PAGES, slots);
    threads::spawn_el0(
        nucleus,
        component.address_space,
        image.entry.expect("component has an entry"),
        stack.top,
        role,
    )
}

/// Block until the components report exactly `expected` on `done`.
fn wait_for(done: &NotificationKey, expected: u64, what: &str) {
    let bits = done
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("{what}: wait failed: {:?}", error.code()));
    assert_eq!(bits, expected, "{what}: unexpected report");
}

/// The Thread must be stopped for good by a fault: `Faulted` at EL0, holding
/// no fault.
fn assert_faulted(nucleus: &Nucleus<ArchObjectsImpl>, thread: ObjectId, what: &str) {
    let thread = nucleus
        .pools
        .threads
        .get_live(usize::from(thread.index))
        .expect("Thread missing");
    let ExecutionContext::Faulted { saved } = thread.context else {
        panic!("{what}: the Thread must be parked as faulted");
    };
    assert_eq!(
        saved.origin,
        ExceptionOrigin::LowerAarch64,
        "{what}: faulted at EL0"
    );
    assert_eq!(
        thread.fault, None,
        "{what}: a faulted Thread holds no fault"
    );
    semi::println!(
        "{what}: thread parked as faulted at PC {:#x}",
        saved.elr_el1
    );
}

/// The fault-handler state of an `AddressSpace`: `(busy, unhandled faults)`.
fn fault_state(nucleus: &Nucleus<ArchObjectsImpl>, address_space: ObjectId) -> (bool, u64) {
    let space = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(address_space.index))
        .expect("AddressSpace missing");
    (space.fault_handler_busy(), space.unhandled_faults())
}

pub fn run() -> ! {
    semi::println!("fault-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder; faulter: sequence, terminate, nested, blocked, while-busy
        // and four witnesses; bare
        threads: 11,
        // builder, faulter, bare
        address_spaces: 3,
        // done, block, park
        notifications: 3,
        event_counts: 0,
        // builder: root/L1/L2 and image L3s; faulter: root/L1/L2, image span,
        // stack region, init page; bare: root/L1/L2, image span, stack region
        page_tables: 3 + image_tables + 6 + 5,
        asid_pools: 1,
    });
    let (nucleus, keytable_addr, builder_as) = (boot.nucleus, boot.keytable_addr, boot.boot_as_id);
    let untyped = UntypedKey::from_key(boot.boot_untyped_key);
    let self_table = KeyTableKey::from_key(boot.self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained,
    };
    let mut slots = SlotCursor::starting_at(FIRST_FIXTURE_SLOT);

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
    let builder_ttbr = ttbr(nucleus, builder_as);
    verify_retained_image(&retained, &[builder_ttbr]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));
    assert_eq!(TTBR0_EL1.get(), builder_ttbr);

    // ── Components ────────────────────────────────────────────────────────
    let faulter = builder.component(nucleus, FAULTER_GUARD, &mut slots);
    builder.load_component(
        &faulter,
        &components::FAULTER,
        FAULTER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, faulter.address_space), &components::FAULTER);
    let bare = builder.component(nucleus, BARE_GUARD, &mut slots);
    builder.load_component(&bare, &components::BARE, BARE_GUARD ^ 0x80_0000, &mut slots);
    verify_component(ttbr(nucleus, bare.address_space), &components::BARE);
    let mut faulter_stacks = builder.stack_region(&faulter, STACK_REGION, &mut slots);
    let mut bare_stacks = builder.stack_region(&bare, STACK_REGION, &mut slots);

    let mut retype_notification = || {
        untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                1,
                &self_table,
                slots.take(1),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()))
    };
    let done = retype_notification();
    let block = retype_notification();
    let park = retype_notification();
    let init_page = builder.private_pages(
        faulter.address_space_key,
        faulter.l2,
        INIT_VA,
        1,
        &mut slots,
    );
    write_init(
        init_page,
        FaulterInit {
            done: grant_notification(&faulter, builder.object_id(done), 5).to_wire(),
            block: grant_notification(&faulter, builder.object_id(block), 6).to_wire(),
            park: grant_notification(&faulter, builder.object_id(park), 7).to_wire(),
            guard: u64::from(FAULTER_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
        },
    );

    // The faulter's handler, at the well-known slot of its own table. The
    // bare component's slot stays empty.
    let handler_stack = builder.user_stack(&mut faulter_stacks, STACK_PAGES, &mut slots);
    AddressSpaceKey::from_key(faulter.address_space_key)
        .create_invocation(
            components::FAULTER
                .export("fault_entry")
                .expect("the faulter exports its fault handler"),
            &faulter.table(),
            KeySlot::FAULT_HANDLER,
            handler_stack.bottom,
            handler_stack.top,
            MINIMUM_HEADROOM,
        )
        .unwrap_or_else(|error| {
            panic!("fault handler CreateInvocation failed: {:?}", error.code())
        });
    let done = NotificationKey::from_key(done);
    let block = NotificationKey::from_key(block);
    let faulter_image = &components::FAULTER;

    // ── Skip, retry and a Return fault, all handled and resumed ───────────
    let sequence = spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_SEQUENCE,
    );
    wait_for(&done, SEQUENCE_BIT, "sequence");
    let thread = nucleus
        .pools
        .threads
        .get_live(usize::from(sequence.index))
        .expect("sequence Thread missing");
    assert_eq!(thread.fault, None);
    assert_eq!(thread.invocation_stack.len(), 0);
    assert_eq!(fault_state(nucleus, faulter.address_space), (false, 0));
    semi::println!("handled faults: skip, retry and Return fault resumed at EL0");

    // ── Terminate: the handler ends its Thread (not an unhandled fault) ───
    let terminated = spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_TERMINATE,
    );
    spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_WITNESS,
    );
    wait_for(&done, WITNESS_BIT, "terminate");
    assert_faulted(nucleus, terminated, "terminate");
    assert_eq!(fault_state(nucleus, faulter.address_space), (false, 0));

    // ── Nested: a fault inside the handler is unhandled ───────────────────
    let nested = spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_NESTED,
    );
    spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_WITNESS,
    );
    wait_for(&done, WITNESS_BIT, "nested");
    assert_faulted(nucleus, nested, "nested");
    // Counted once, and the handler the Thread held is free again.
    assert_eq!(fault_state(nucleus, faulter.address_space), (false, 1));

    // ── Busy: a fault while another Thread's handler is blocked ───────────
    let blocked = spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_BLOCKED,
    );
    let while_busy = spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_WHILE_BUSY,
    );
    spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_WITNESS,
    );
    wait_for(&done, WITNESS_BIT, "busy");
    assert_faulted(nucleus, while_busy, "busy");
    // The blocked handler still holds the AddressSpace's handler.
    assert_eq!(fault_state(nucleus, faulter.address_space), (true, 2));
    let holder = nucleus
        .pools
        .threads
        .get_live(usize::from(blocked.index))
        .expect("blocked Thread missing");
    assert!(
        holder.fault.is_some(),
        "the blocked Thread is inside its handler"
    );
    // Release it: the handler skips the fault and the Thread reports.
    block
        .signal(1)
        .unwrap_or_else(|error| panic!("block signal failed: {:?}", error.code()));
    wait_for(&done, BLOCKED_BIT, "blocked");
    assert_eq!(fault_state(nucleus, faulter.address_space), (false, 2));

    // ── No handler at all ─────────────────────────────────────────────────
    let unhandled = spawn(
        &builder,
        nucleus,
        &mut bare_stacks,
        &mut slots,
        &bare,
        &components::BARE,
        0,
    );
    spawn(
        &builder,
        nucleus,
        &mut faulter_stacks,
        &mut slots,
        &faulter,
        faulter_image,
        ROLE_WITNESS,
    );
    wait_for(&done, WITNESS_BIT, "no handler");
    assert_faulted(nucleus, unhandled, "no handler");
    assert_eq!(fault_state(nucleus, bare.address_space), (false, 1));
    assert_eq!(fault_state(nucleus, faulter.address_space), (false, 2));

    semi::println!("Fault delivery to EL0 handlers passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
