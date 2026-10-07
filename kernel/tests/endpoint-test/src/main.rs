#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! endpoint-test: separately linked EL0 components, and a rendezvous through
//! a third-party endpoint component.
//!
//! The boot Thread is a trusted `EL1t` builder/supervisor. It loads bundled
//! userspace components (see `image.toml`), each into its own
//! `AddressSpace` that maps only that component's image, its guarded stacks
//! and its init page:
//!
//! - **hello** — a smoke test: runs at EL0, signals the builder, parks;
//! - **client** — two client Threads sending requests;
//! - **endpoint** — a passive component holding the queue, the doorbell
//!   `Notification` and the `done` `EventCount`, reached only through PPC;
//! - **server** — a Thread serving requests.
//!
//! Clients and the server never hold each other's keys, and block *inside*
//! the endpoint during their migrated calls. The builder waits for both
//! clients to report, then checks the replies and the kernel's view.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-endpoint`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    core::sync::atomic::Ordering,
    endpoint_protocol::{
        CLIENT_REQUESTS, ClientInit, ClientReport, EndpointInit, INIT_VA, REPORT_VA, STACK_REGION,
        ServerInit, work as check_work,
    },
    kickstart::{
        bootstrap::{
            BOOT_TABLE_SIZE_BITS, PoolCapacities, bootstrap_nucleus, retained_init_memory,
        },
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libexception::arch::aarch64::ExceptionOrigin,
    libimage::ComponentImage,
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::{Component, ttbr},
        keys::{SlotCursor, boot_key},
        loader::{UserStack, verify_component},
        paging::{ADDR_MASK, PAGE, find_leaf, image_table_count, read_leaf},
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
        },
    },
};

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const CLIENT_GUARD: u32 = 0x0C_1E01;
const ENDPOINT_GUARD: u32 = 0x0E_4D01;
const SERVER_GUARD: u32 = 0x05_E401;
const HELLO_GUARD: u32 = 0x0B_E101;
/// Server-private salt: proves a reply was computed in the server.
const SALT: u64 = 0x5A17_0000_0000_00A5;
/// The bits the `hello` component signals (see `userspace/tests/hello`).
const HELLO_BITS: u64 = 0b1010;
const MINIMUM_HEADROOM: u64 = 0x400;
/// Pages per EL0 stack (each also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
/// Fixture boot-table slots start clear of the well-known bootstrap slots.
const FIRST_FIXTURE_SLOT: u32 = 64;

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Provision a component `AddressSpace` with `guard` and map `image` into it.
fn load(
    builder: &Builder<'_>,
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    image: &ComponentImage,
    guard: u32,
    slots: &mut SlotCursor,
) -> Component {
    let component = builder.component(nucleus, guard, slots);
    // The table archiving the image page capabilities gets its own guard,
    // derived from the component's.
    builder.load_component(&component, image, guard ^ 0x80_0000, slots);
    verify_component(ttbr(nucleus, component.address_space), image);
    component
}

/// A guarded EL0 stack in `component`, checked to have unmapped neighbors.
fn stack(
    builder: &Builder<'_>,
    nucleus: &Nucleus<ArchObjectsImpl>,
    component: &Component,
    region: &mut libkicktest::loader::StackRegion,
    pages: u32,
    slots: &mut SlotCursor,
) -> UserStack {
    let stack = builder.user_stack(region, pages, slots);
    let root = ttbr(nucleus, component.address_space);
    for guard in [stack.bottom - PAGE, stack.top] {
        assert!(
            find_leaf(root, guard).is_none(),
            "stack guard page at {guard:#x} must be unmapped"
        );
    }
    stack
}

/// Write `value` into the builder-owned page at `paddr` (an init page).
fn write_init<T>(paddr: u64, value: T) {
    // SAFETY: `paddr` names a freshly retyped, accounted Frame the builder
    // owns; the component does not run until after this write.
    unsafe {
        PhysAddr::new(paddr)
            .user_to_kernel()
            .as_mut_ptr::<T>()
            .write_volatile(value);
    }
}

/// Bootstrap grant of a Notification into `component`'s table at `slot`.
/// Notification is off the `CopyDerive` allowlist, so the builder installs the
/// second capability kernel-privately.
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

/// The parked context of an EL0 Thread: it must be parked in
/// `address_space`, with `depth` continuations, at EL0.
fn assert_parked_el0(
    nucleus: &Nucleus<ArchObjectsImpl>,
    thread: ObjectId,
    address_space: ObjectId,
    depth: usize,
) {
    let parked = nucleus
        .pools
        .threads
        .get_live(usize::from(thread.index))
        .expect("Thread missing");
    assert_eq!(parked.address_space, address_space);
    assert_eq!(parked.invocation_stack.len(), depth);
    let ExecutionContext::Parked { saved, .. } = parked.context else {
        panic!("Thread is not parked");
    };
    assert_eq!(saved.spsr_el1 & 0xf, 0, "the Thread must run as EL0t");
    assert_eq!(saved.origin, ExceptionOrigin::LowerAarch64);
}

pub fn run() -> ! {
    semi::println!("endpoint-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder, hello, two clients, server
        threads: 5,
        // builder, client, endpoint, server, hello
        address_spaces: 5,
        // endpoint doorbell, clients done, clients park, hello
        notifications: 4,
        // endpoint done
        event_counts: 1,
        // builder: root/L1/L2 and image L3s; each of the four components:
        // root/L1/L2, image span, stack region; client/endpoint/server: init
        page_tables: 3 + image_tables + 4 * 5 + 3,
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

    // ── The builder's own AddressSpace: the bundling image, nothing else ──
    let builder_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let builder_l2 = builder.root_chain(builder_as_key, &mut slots);
    assert_eq!(Builder::assign_asid(builder_as_key), 1);
    let image = builder.map_retained_image(
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
    assert_eq!(image.archived, image.image_pages + image.stack_pages);
    let builder_ttbr = ttbr(nucleus, builder_as);
    verify_retained_image(&retained, &[builder_ttbr]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));
    assert_eq!(TTBR0_EL1.get(), builder_ttbr);
    let notification = |table: &KeyTableKey, slot: u32| {
        untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                1,
                table,
                slot,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()))
    };

    // ── Components ────────────────────────────────────────────────────────
    let client = load(
        &builder,
        nucleus,
        &components::CLIENT,
        CLIENT_GUARD,
        &mut slots,
    );
    let endpoint = load(
        &builder,
        nucleus,
        &components::ENDPOINT,
        ENDPOINT_GUARD,
        &mut slots,
    );
    let server = load(
        &builder,
        nucleus,
        &components::SERVER,
        SERVER_GUARD,
        &mut slots,
    );
    let hello = load(
        &builder,
        nucleus,
        &components::HELLO,
        HELLO_GUARD,
        &mut slots,
    );
    assert_eq!(
        [client.asid, endpoint.asid, server.asid, hello.asid],
        [2, 3, 4, 5]
    );
    let (image_start, _) = retained.image();
    for component in [&client, &endpoint, &server, &hello] {
        assert!(
            find_leaf(ttbr(nucleus, component.address_space), image_start).is_none(),
            "a component maps only its own image"
        );
    }

    // ── EL0 smoke test: hello ─────────────────────────────────────────────
    let mut hello_stacks = builder.stack_region(&hello, STACK_REGION, &mut slots);
    let hello_stack = stack(
        &builder,
        nucleus,
        &hello,
        &mut hello_stacks,
        STACK_PAGES,
        &mut slots,
    );
    let hello_done = notification(&self_table, slots.take(1));
    let hello_key = grant_notification(&hello, builder.object_id(hello_done), 5);
    let hello_thread = threads::spawn_el0(
        nucleus,
        hello.address_space,
        components::HELLO.entry.expect("hello has an entry"),
        hello_stack.top,
        hello_key.to_wire(),
    );
    let bits = NotificationKey::from_key(hello_done)
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("hello wait failed: {:?}", error.code()));
    assert_eq!(bits, HELLO_BITS);
    assert_parked_el0(nucleus, hello_thread, hello.address_space, 0);
    semi::println!("EL0 component `hello` ran in its own AddressSpace");

    // ── The endpoint: its objects, its Invocation stacks, its exports ─────
    let endpoint_table = endpoint.table();
    let doorbell = notification(&endpoint_table, 5);
    let done = untyped
        .retype(
            ObjectType::EVENT_COUNT,
            0,
            0,
            1,
            &endpoint_table,
            6,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("done Retype failed: {:?}", error.code()));
    let endpoint_init = builder.private_pages(
        endpoint.address_space_key,
        endpoint.l2,
        INIT_VA,
        1,
        &mut slots,
    );
    write_init(
        endpoint_init,
        EndpointInit {
            guard: u64::from(ENDPOINT_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
            doorbell: doorbell.to_wire(),
            done: done.to_wire(),
        },
    );
    // One guarded endpoint stack per Thread that may be inside at once.
    let mut endpoint_stacks = builder.stack_region(&endpoint, STACK_REGION, &mut slots);
    let [client0_stack, client1_stack, server_stack] = [0; 3].map(|_| {
        stack(
            &builder,
            nucleus,
            &endpoint,
            &mut endpoint_stacks,
            2,
            &mut slots,
        )
    });
    let endpoint_space = AddressSpaceKey::from_key(endpoint.address_space_key);
    let export = |name: &str, table: &KeyTableKey, slot: u32, stack: UserStack| {
        let entry = components::ENDPOINT
            .export(name)
            .unwrap_or_else(|| panic!("endpoint does not export `{name}`"));
        endpoint_space
            .create_invocation(
                entry,
                table,
                KeySlot(slot),
                stack.bottom,
                stack.top,
                MINIMUM_HEADROOM,
            )
            .unwrap_or_else(|error| panic!("CreateInvocation `{name}` failed: {:?}", error.code()))
    };

    // ── The client AddressSpace ───────────────────────────────────────────
    let client_table = client.table();
    let send = [
        export("send_entry", &client_table, 5, client0_stack),
        export("send_entry", &client_table, 6, client1_stack),
    ];
    let clients_done = notification(&self_table, slots.take(1));
    let clients_done_key = grant_notification(&client, builder.object_id(clients_done), 7);
    let clients_park = notification(&client_table, 8);
    let client_pages =
        builder.private_pages(client.address_space_key, client.l2, INIT_VA, 2, &mut slots);
    write_init(
        client_pages,
        ClientInit {
            send: send.map(|key| key.to_wire()),
            stack_end: [client0_stack.top, client1_stack.top],
            done: clients_done_key.to_wire(),
            park: clients_park.to_wire(),
        },
    );
    let report_paddr = client_pages + (REPORT_VA - INIT_VA);
    let mut client_stacks = builder.stack_region(&client, STACK_REGION, &mut slots);

    // ── The server AddressSpace ───────────────────────────────────────────
    let server_table = server.table();
    let server_init =
        builder.private_pages(server.address_space_key, server.l2, INIT_VA, 1, &mut slots);
    write_init(
        server_init,
        ServerInit {
            receive: export("receive_entry", &server_table, 5, server_stack).to_wire(),
            reply_receive: export("reply_receive_entry", &server_table, 6, server_stack).to_wire(),
            stack_end: server_stack.top,
            salt: SALT,
        },
    );
    let mut server_stacks = builder.stack_region(&server, STACK_REGION, &mut slots);
    let server_thread_stack = stack(
        &builder,
        nucleus,
        &server,
        &mut server_stacks,
        STACK_PAGES,
        &mut slots,
    );
    // Private regions belong to one root each.
    assert_ne!(
        read_leaf(ttbr(nucleus, client.address_space), INIT_VA).1 & ADDR_MASK,
        read_leaf(ttbr(nucleus, server.address_space), INIT_VA).1 & ADDR_MASK
    );
    assert!(find_leaf(ttbr(nucleus, hello.address_space), INIT_VA).is_none());

    // ── Run: both clients first, so two requests queue before the server ──
    let client_threads = [0_u64, 1].map(|index| {
        let thread_stack = stack(
            &builder,
            nucleus,
            &client,
            &mut client_stacks,
            STACK_PAGES,
            &mut slots,
        );
        threads::spawn_el0(
            nucleus,
            client.address_space,
            components::CLIENT.entry.expect("client has an entry"),
            thread_stack.top,
            index,
        )
    });
    let server_thread = threads::spawn_el0(
        nucleus,
        server.address_space,
        components::SERVER.entry.expect("server has an entry"),
        server_thread_stack.top,
        0,
    );
    let mut reported = 0;
    while reported != 0b11 {
        reported |= NotificationKey::from_key(clients_done)
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("clients wait failed: {:?}", error.code()));
    }
    assert_eq!(TTBR0_EL1.get(), builder_ttbr);

    // ── Check: replies, tickets and where everyone is parked ─────────────
    // SAFETY: the builder owns the report Frame; both clients finished
    // writing before signalling (release), observed by the wait (acquire).
    let report = unsafe {
        &*(PhysAddr::new(report_paddr)
            .user_to_kernel()
            .as_ptr::<ClientReport>())
    };
    let mut tickets = 0_u64;
    for (client_index, requests) in CLIENT_REQUESTS.iter().enumerate() {
        for (row, &request) in requests.iter().enumerate() {
            let [ticket, reply] = &report.replies[client_index][row];
            let (ticket, reply) = (
                ticket.load(Ordering::Acquire),
                reply.load(Ordering::Acquire),
            );
            assert_eq!(
                reply,
                check_work(request, SALT),
                "reply to client {client_index} request {row}"
            );
            assert!((1..=3).contains(&ticket), "ticket {ticket} out of range");
            tickets |= 1 << ticket;
        }
    }
    assert_eq!(tickets, 0b1110, "tickets 1..=3 each issued exactly once");
    // The server is parked *inside* the endpoint, in `reply_receive`'s
    // receive half: migrated there, one continuation on its stack, at EL0.
    assert_parked_el0(nucleus, server_thread, endpoint.address_space, 1);
    for thread in client_threads {
        assert_parked_el0(nucleus, thread, client.address_space, 0);
    }
    semi::println!("Endpoint rendezvous across three EL0 components passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
