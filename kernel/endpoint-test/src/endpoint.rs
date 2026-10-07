//! The rendezvous endpoint: a third-party component that neither the clients
//! nor the server own. Both reach it only through `Invocation`s, and their
//! Threads meet inside it.
//!
//! - `send(request)` (client): queue the request, ring the doorbell, then
//!   block *inside the endpoint* until the server has replied, and return
//!   `(ticket, reply)`.
//! - `receive()` (server): block inside the endpoint until a request is
//!   queued, and return `(ticket, request)`.
//! - `reply_receive(ticket, reply)` (server): publish the reply, wake its
//!   client, then continue exactly as `receive` — one Call per request in
//!   the server's steady state.
//!
//! The queue lives in a page mapped only in the endpoint's root, and the
//! doorbell `Notification` and `done` `EventCount` keys live only in the
//! endpoint's table. Replies are FIFO, so `done` counts replies and a
//! client's ticket `t` is answered once `done >= t`.

use {
    crate::PRIVATE_VA,
    core::sync::atomic::{AtomicU64, Ordering},
    libaddress::PhysAddr,
    libkicktest::paging::PAGE,
    libobject::{EventCountKey, NotificationKey, RawKey, export::PpcResult, ppc_export},
};

/// Request slots; a client may have at most this many requests outstanding
/// beyond the last reply.
const QUEUE_SLOTS: u64 = 8;
const DOORBELL_BIT: u64 = 1;
/// `send` result word 1 when the queue is full (no ticket issued).
pub const QUEUE_FULL: u64 = u64::MAX;
/// `reply_receive` result word 1 when the reply is not for the next
/// unanswered ticket; nothing is published and no request is received.
pub const BAD_REPLY: u64 = u64::MAX - 1;

/// Endpoint-private page layout at [`PRIVATE_VA`]: the state page, then
/// [`STACK_SLOTS`] Invocation stacks of [`STACK_SLOT_PAGES`] pages each.
pub const STACK_SLOTS: u64 = 3;
pub const STACK_SLOT_PAGES: u64 = 2;
pub const PRIVATE_PAGES: u64 = 1 + STACK_SLOTS * STACK_SLOT_PAGES;
pub const MINIMUM_HEADROOM: u64 = 0x400;

/// The `[base, end)` stack extent of Invocation stack slot `index`. Threads
/// that may be inside the endpoint at the same time use distinct slots.
pub const fn stack_slot(index: u64) -> (u64, u64) {
    let base = PRIVATE_VA + PAGE * (1 + index * STACK_SLOT_PAGES);
    (base, base + PAGE * STACK_SLOT_PAGES)
}

#[repr(C)]
struct EndpointState {
    /// Endpoint-table keys, written by the builder at component init.
    doorbell: AtomicU64,
    done: AtomicU64,
    /// Last ticket issued, last ticket handed to the server, last ticket
    /// answered.
    submitted: AtomicU64,
    taken: AtomicU64,
    replied: AtomicU64,
    requests: [AtomicU64; 8],
    replies: [AtomicU64; 8],
}

const _: () = assert!(size_of::<EndpointState>() <= 4096);

fn state() -> &'static EndpointState {
    // SAFETY: the endpoint's state page is mapped RW at PRIVATE_VA in the
    // endpoint's root only, and these bodies run only migrated into it.
    // Every field is atomic, so concurrent Threads in the endpoint share it.
    unsafe { &*(PRIVATE_VA as *const EndpointState) }
}

fn slot(ticket: u64) -> usize {
    usize::try_from((ticket - 1) % QUEUE_SLOTS).unwrap_or(0)
}

fn doorbell(state: &EndpointState) -> NotificationKey {
    NotificationKey::from_key(RawKey::from_wire(state.doorbell.load(Ordering::Acquire)))
}

fn done(state: &EndpointState) -> EventCountKey {
    EventCountKey::from_key(RawKey::from_wire(state.done.load(Ordering::Acquire)))
}

/// Component init, run by the builder before any Invocation: record the
/// endpoint-table keys in the state page through the direct map.
pub fn init(state_paddr: u64, doorbell_key: RawKey, done_key: RawKey) {
    // SAFETY: the builder owns the freshly sanitized state Frame and reaches
    // it through the invariant direct map; no endpoint code runs yet.
    let state =
        unsafe { &*(PhysAddr::new(state_paddr).user_to_kernel().as_u64() as *const EndpointState) };
    state
        .doorbell
        .store(doorbell_key.to_wire(), Ordering::Release);
    state.done.store(done_key.to_wire(), Ordering::Release);
}

/// Read the endpoint's `(submitted, taken, replied)` counters through the
/// direct map, for the builder's checks.
pub fn counters(state_paddr: u64) -> (u64, u64, u64) {
    // SAFETY: as in `init`; read-only atomic loads.
    let state =
        unsafe { &*(PhysAddr::new(state_paddr).user_to_kernel().as_u64() as *const EndpointState) };
    (
        state.submitted.load(Ordering::Acquire),
        state.taken.load(Ordering::Acquire),
        state.replied.load(Ordering::Acquire),
    )
}

/// Hand the server the next queued request, blocking on the doorbell inside
/// the endpoint while the queue is empty. A pending doorbell bit may wake it
/// with nothing to take; it then simply waits again.
fn receive(state: &EndpointState) -> PpcResult {
    loop {
        let taken = state.taken.load(Ordering::Acquire);
        if taken < state.submitted.load(Ordering::Acquire) {
            let ticket = taken + 1;
            state.taken.store(ticket, Ordering::Release);
            return PpcResult {
                r0: ticket,
                r1: state.requests[slot(ticket)].load(Ordering::Acquire),
            };
        }
        doorbell(state)
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("endpoint: doorbell wait failed: {:?}", error.code()));
    }
}

extern "C" fn send_body(
    _dummy0: u64,
    _dummy1: u64,
    request: u64,
    _input1: u64,
    _input2: u64,
    _input3: u64,
    _input4: u64,
    _input5: u64,
) -> PpcResult {
    let state = state();
    let ticket = state.submitted.load(Ordering::Acquire) + 1;
    if ticket - state.replied.load(Ordering::Acquire) > QUEUE_SLOTS {
        return PpcResult {
            r0: 0,
            r1: QUEUE_FULL,
        };
    }
    state.requests[slot(ticket)].store(request, Ordering::Release);
    state.submitted.store(ticket, Ordering::Release);
    doorbell(state)
        .signal(DOORBELL_BIT)
        .unwrap_or_else(|error| panic!("endpoint: doorbell signal failed: {:?}", error.code()));
    // Block here, inside the endpoint, until the server has replied to us.
    done(state)
        .await_ge(ticket, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("endpoint: done await failed: {:?}", error.code()));
    PpcResult {
        r0: ticket,
        r1: state.replies[slot(ticket)].load(Ordering::Acquire),
    }
}

extern "C" fn receive_body(
    _dummy0: u64,
    _dummy1: u64,
    _input0: u64,
    _input1: u64,
    _input2: u64,
    _input3: u64,
    _input4: u64,
    _input5: u64,
) -> PpcResult {
    receive(state())
}

extern "C" fn reply_receive_body(
    _dummy0: u64,
    _dummy1: u64,
    ticket: u64,
    reply: u64,
    _input2: u64,
    _input3: u64,
    _input4: u64,
    _input5: u64,
) -> PpcResult {
    let state = state();
    let replied = state.replied.load(Ordering::Acquire);
    if ticket != replied + 1 || ticket > state.taken.load(Ordering::Acquire) {
        return PpcResult {
            r0: 0,
            r1: BAD_REPLY,
        };
    }
    state.replies[slot(ticket)].store(reply, Ordering::Release);
    state.replied.store(ticket, Ordering::Release);
    // Wakes the client blocked in `send`; it runs once this Thread blocks.
    done(state)
        .advance(1)
        .unwrap_or_else(|error| panic!("endpoint: done advance failed: {:?}", error.code()));
    receive(state)
}

ppc_export!(
    /// `send(request) -> (ticket, reply)`.
    pub fn send_entry => send_body
);

ppc_export!(
    /// `receive() -> (ticket, request)`.
    pub fn receive_entry => receive_body
);

ppc_export!(
    /// `reply_receive(ticket, reply) -> (next ticket, next request)`.
    pub fn reply_receive_entry => reply_receive_body
);
