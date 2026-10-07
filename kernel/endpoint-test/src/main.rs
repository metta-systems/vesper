#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! endpoint-test: a rendezvous through a third-party endpoint component.
//!
//! Three parties, each in its own `AddressSpace` with its own table, root and
//! ASID:
//!
//! - **client** — the boot Thread plus a second client Thread;
//! - **endpoint** — a component holding the request queue, the doorbell
//!   `Notification` and the `done` `EventCount` (see [`endpoint`]);
//! - **server** — a Thread that serves requests (see [`server`]).
//!
//! Clients and the server never hold each other's keys. They reach the
//! endpoint only through PPC `Invocation`s, and block *inside* it: a client
//! in `send` until its reply is published, the server in `receive` /
//! `reply_receive` until a request is queued. The endpoint's queue page and
//! the server's private page are mapped only in their own roots.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-endpoint`).

mod endpoint;
mod server;

use {
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    core::sync::atomic::{AtomicU64, Ordering},
    kickstart::{
        bootstrap::{
            BOOT_TABLE_SIZE_BITS, PoolCapacities, bootstrap_nucleus, retained_init_memory,
        },
        kickstart_init_el2,
    },
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        paging::{ADDR_MASK, PAGE, find_leaf, image_table_count, read_leaf},
        threads,
    },
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey, export, invocation::InvocationKey, thread::ThreadReturnKey,
    },
    libqemu::semihosting as semi,
    nucleus::objects::ExecutionContext,
};

/// Each party's private region: one 2 MiB span mapped only in its own root.
pub const PRIVATE_VA: u64 = 0x1800_0000;
const ENDPOINT_GUARD: u32 = 0x0E_4D01;
const SERVER_GUARD: u32 = 0x05_E401;
const ARCHIVE_GUARD: u32 = 0x0A_4C01;
/// Server-private salt: proves the reply was computed in the server.
const SALT: u64 = 0x5A17_0000_0000_00A5;
/// Fixture boot-table slots start clear of the well-known bootstrap slots.
const FIRST_FIXTURE_SLOT: u32 = 64;

const CLIENT_STACK_SLOT: u64 = 0;
const CLIENT2_STACK_SLOT: u64 = 1;

// Keys and results of the second client Thread (client `AddressSpace`).
static CLIENT2_SEND: AtomicU64 = AtomicU64::new(0);
static CLIENT2_DONE: AtomicU64 = AtomicU64::new(0);
static CLIENT2_PARK: AtomicU64 = AtomicU64::new(0);
static CLIENT2_TICKET: AtomicU64 = AtomicU64::new(0);
static CLIENT2_REPLY: AtomicU64 = AtomicU64::new(0);
const CLIENT2_REQUEST: u64 = 0xB2;

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// One PPC call into the endpoint on the given Invocation stack.
pub fn call(key: RawKey, args: [u64; 6], stack_end: u64) -> (u64, u64) {
    // SAFETY: `stack_end` tops an endpoint stack slot reserved for exactly
    // the Threads that use this Invocation; no other Thread uses it then.
    unsafe { InvocationKey::from_key(key).call(args, stack_end) }
        .unwrap_or_else(|error| panic!("endpoint Call failed: {:?}", error.code()))
}

/// `send(request)` on a client's endpoint stack slot.
fn send(key: RawKey, request: u64, stack_slot: u64) -> (u64, u64) {
    let (_, stack_end) = endpoint::stack_slot(stack_slot);
    call(key, [request, 0, 0, 0, 0, 0], stack_end)
}

/// The image-supplied handler for a rejected export Return.
#[unsafe(no_mangle)]
pub extern "C" fn vesper_thread_return_fault(
    status: u64,
    detail1: u64,
    detail2: u64,
    original_r0: u64,
    original_r1: u64,
) -> ! {
    panic!(
        "endpoint export Return rejected: ({status}, {detail1:#x}, {detail2:#x}), results ({original_r0:#x}, {original_r1:#x})"
    );
}

/// The second client Thread: one `send`, report back, then park.
extern "C" fn client2_entry() -> ! {
    let (ticket, reply) = send(
        RawKey::from_wire(CLIENT2_SEND.load(Ordering::Acquire)),
        CLIENT2_REQUEST,
        CLIENT2_STACK_SLOT,
    );
    CLIENT2_TICKET.store(ticket, Ordering::Release);
    CLIENT2_REPLY.store(reply, Ordering::Release);
    NotificationKey::from_key(RawKey::from_wire(CLIENT2_DONE.load(Ordering::Acquire)))
        .signal(1)
        .unwrap_or_else(|error| panic!("client2: done signal failed: {:?}", error.code()));
    match NotificationKey::from_key(RawKey::from_wire(CLIENT2_PARK.load(Ordering::Acquire)))
        .wait(NotificationKey::WAIT_INFINITE)
    {
        Ok(bits) => panic!("client2: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("client2: park wait failed: {:?}", error.code()),
    }
}

pub fn run() -> ! {
    semi::println!("endpoint-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let image_table_count_u32 = u32::try_from(image_tables).unwrap_or(u32::MAX);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // boot client, second client, server
        threads: 3,
        // client, endpoint, server
        address_spaces: 3,
        // endpoint doorbell, client2 done, client2 park
        notifications: 3,
        // endpoint done
        event_counts: 1,
        // three root/L1/L2 chains, their image L3s, two private-region L3s
        page_tables: 3 * (3 + image_tables) + 2,
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

    // ── Three AddressSpaces ─────────────────────────────────────────────
    let client_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let client_l2 = builder.root_chain(client_as_key, &mut slots);
    assert_eq!(Builder::assign_asid(client_as_key), 1);
    let endpoint = builder.component(nucleus, ENDPOINT_GUARD, &mut slots);
    let server = builder.component(nucleus, SERVER_GUARD, &mut slots);
    assert_eq!((endpoint.asid, server.asid), (2, 3));

    let targets = [
        (client_as_key, client_l2),
        (endpoint.address_space_key, endpoint.l2),
        (server.address_space_key, server.l2),
    ]
    .map(|(address_space, l2)| ImageTarget {
        address_space,
        l2,
        first_table_slot: slots.take(image_table_count_u32),
    });
    let archive = ImageArchive {
        slot: slots.take(1),
        guard: ARCHIVE_GUARD,
        grant_scratch: slots.take(1),
        copy_scratch: slots.take(1),
    };
    let image = builder.map_retained_image(&targets, &archive);
    // One capability per mapped page: the image in all three roots, the low
    // execution stack in the client's.
    assert_eq!(image.archived, 3 * image.image_pages + image.stack_pages);
    semi::println!(
        "endpoint-test: image {} pages in 3 roots, stack {} pages, archive 2^{} ({} caps)",
        image.image_pages,
        image.stack_pages,
        image.archive_bits,
        image.archived
    );

    // Private pages: the endpoint's queue state and Invocation stacks, and
    // the server's keys and salt. Each is mapped in its own root only.
    let endpoint_state = builder.private_pages(
        endpoint.address_space_key,
        endpoint.l2,
        PRIVATE_VA,
        u32::try_from(endpoint::PRIVATE_PAGES).unwrap_or(u32::MAX),
        &mut slots,
    );
    let server_state = builder.private_pages(
        server.address_space_key,
        server.l2,
        PRIVATE_VA,
        1,
        &mut slots,
    );

    let client_ttbr = ttbr(nucleus, boot_as_id);
    let endpoint_ttbr = ttbr(nucleus, endpoint.address_space);
    let server_ttbr = ttbr(nucleus, server.address_space);
    verify_retained_image(&retained, &[client_ttbr, endpoint_ttbr, server_ttbr]);
    assert!(
        find_leaf(client_ttbr, PRIVATE_VA).is_none(),
        "the client must not map any party's private region"
    );
    assert_eq!(
        read_leaf(endpoint_ttbr, PRIVATE_VA).1 & ADDR_MASK,
        endpoint_state
    );
    assert_eq!(
        read_leaf(server_ttbr, PRIVATE_VA).1 & ADDR_MASK,
        server_state
    );
    assert_eq!(
        read_leaf(endpoint_ttbr, PRIVATE_VA + PAGE).1 & ADDR_MASK,
        endpoint_state + PAGE
    );
    assert!(find_leaf(server_ttbr, PRIVATE_VA + PAGE).is_none());

    AddressSpaceKey::from_key(client_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("client Activate failed: {:?}", error.code()));
    assert_eq!(TTBR0_EL1.get(), client_ttbr);

    // ── The endpoint component ──────────────────────────────────────────
    // Its synchronization objects are retyped straight into its own table:
    // only the endpoint ever holds keys to them.
    let doorbell = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &endpoint.table(),
            5,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("doorbell Retype failed: {:?}", error.code()));
    let done = untyped
        .retype(
            ObjectType::EVENT_COUNT,
            0,
            0,
            1,
            &endpoint.table(),
            6,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("done Retype failed: {:?}", error.code()));
    assert_eq!(doorbell, endpoint.local_key(5, doorbell.incarnation()));
    endpoint::init(endpoint_state, doorbell, done);
    // The endpoint is the image's only PPC target: the shared export adapter
    // Returns through its provisioned sentinel.
    export::init_return_key(&ThreadReturnKey::provisioned(
        ENDPOINT_GUARD,
        BOOT_TABLE_SIZE_BITS,
    ));

    // Exports, each Invocation carrying the stack slot its caller uses and
    // installed straight into the caller's table.
    let endpoint_space = AddressSpaceKey::from_key(endpoint.address_space_key);
    let export_to = |entry: u64, table: &KeyTableKey, slot: u32, stack_slot: u64| {
        let (base, end) = endpoint::stack_slot(stack_slot);
        endpoint_space
            .create_invocation(
                entry,
                table,
                KeySlot(slot),
                base,
                end,
                endpoint::MINIMUM_HEADROOM,
            )
            .unwrap_or_else(|error| panic!("endpoint CreateInvocation failed: {:?}", error.code()))
    };
    let send_entry = endpoint::send_entry as *const () as u64;
    let client_send = export_to(send_entry, &self_table, slots.take(1), CLIENT_STACK_SLOT);
    let second_client_send = export_to(send_entry, &self_table, slots.take(1), CLIENT2_STACK_SLOT);
    let server_table = server.table();
    let receive = export_to(
        endpoint::receive_entry as *const () as u64,
        &server_table,
        5,
        server::ENDPOINT_STACK_SLOT,
    );
    let reply_receive = export_to(
        endpoint::reply_receive_entry as *const () as u64,
        &server_table,
        6,
        server::ENDPOINT_STACK_SLOT,
    );
    assert_eq!(receive, server.local_key(5, receive.incarnation()));
    server::init(server_state, receive, reply_receive, SALT);

    // ── Threads ─────────────────────────────────────────────────────────
    let notification = |slot: u32| {
        untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                1,
                &self_table,
                slot,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("client Notification Retype failed: {:?}", error.code()))
    };
    let client2_done = notification(slots.take(1));
    CLIENT2_SEND.store(second_client_send.to_wire(), Ordering::Release);
    CLIENT2_DONE.store(client2_done.to_wire(), Ordering::Release);
    CLIENT2_PARK.store(notification(slots.take(1)).to_wire(), Ordering::Release);
    let client2_stack = builder.execution_stack(8, &mut slots);
    let server_stack = builder.execution_stack(8, &mut slots);
    // Queued in this order: the second client runs first when the boot
    // client blocks, so two requests are queued before the server starts.
    threads::spawn(
        nucleus,
        boot_as_id,
        client2_entry as *const () as u64,
        client2_stack.top,
    );
    let server_thread = threads::spawn(
        nucleus,
        server.address_space,
        server::server_entry as *const () as u64,
        server_stack.top,
    );

    // ── Round 1: two clients queue before the server first runs ─────────
    let request = 0xA1;
    let (ticket, reply) = send(client_send, request, CLIENT_STACK_SLOT);
    assert_eq!((ticket, reply), (1, server::work(request, SALT)));
    assert_eq!(TTBR0_EL1.get(), client_ttbr);
    // Wait for the second client's reply to come back to it.
    NotificationKey::from_key(client2_done)
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("client2 done wait failed: {:?}", error.code()));
    assert_eq!(
        (
            CLIENT2_TICKET.load(Ordering::Acquire),
            CLIENT2_REPLY.load(Ordering::Acquire)
        ),
        (2, server::work(CLIENT2_REQUEST, SALT))
    );
    assert_eq!(endpoint::counters(endpoint_state), (2, 2, 2));
    assert_eq!(server::served(server_state), 2);

    // The server is now parked *inside* the endpoint, in `reply_receive`'s
    // receive half: migrated there, one continuation on its stack.
    let parked = nucleus
        .pools
        .threads
        .get_live(usize::from(server_thread.index))
        .expect("server Thread missing");
    assert_eq!(parked.address_space, endpoint.address_space);
    assert_eq!(parked.invocation_stack.len(), 1);
    assert!(matches!(parked.context, ExecutionContext::Parked { .. }));

    // ── Round 2: a request wakes the server parked inside the endpoint ──
    let request = 0xC3;
    let (ticket, reply) = send(client_send, request, CLIENT_STACK_SLOT);
    assert_eq!((ticket, reply), (3, server::work(request, SALT)));
    assert_eq!(endpoint::counters(endpoint_state), (3, 3, 3));
    assert_eq!(server::served(server_state), 3);
    let client = nucleus
        .pools
        .threads
        .get_live(0)
        .expect("boot client Thread missing");
    assert_eq!(client.address_space, boot_as_id);
    assert!(client.invocation_stack.is_empty());
    assert_eq!(TTBR0_EL1.get(), client_ttbr);

    semi::println!("Endpoint rendezvous across three AddressSpaces passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
