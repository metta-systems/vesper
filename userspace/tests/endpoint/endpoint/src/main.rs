#![no_std]
#![no_main]

//! The rendezvous endpoint: a passive third-party component that neither the
//! clients nor the server own. It has no Thread of its own; clients and the
//! server reach it only through `Invocation`s, and their Threads meet — and
//! block — inside it.
//!
//! - `send(request)` (client): queue the request, ring the doorbell, block
//!   inside the endpoint until the server has replied, return
//!   `(ticket, reply)`.
//! - `receive()` (server): block inside the endpoint until a request is
//!   queued, return `(ticket, request)`.
//! - `reply_receive(ticket, reply)` (server): publish the reply, wake its
//!   client, then continue exactly as `receive`.
//!
//! The queue is this component's own statics, private to its image and
//! `AddressSpace`. The doorbell `Notification` and `done` `EventCount` keys
//! live only in its table. Replies are FIFO, so `done` counts replies and
//! ticket `t` is answered once `done >= t`.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    endpoint_protocol::{BAD_REPLY, DOORBELL_BIT, EndpointInit, QUEUE_FULL, init},
    libobject::{
        EventCountKey, NotificationKey, RawKey,
        export::{self, PpcResult},
        ppc_export,
        thread::ThreadReturnKey,
    },
    libuser as component,
};

component::passive!();

const QUEUE_SLOTS: u64 = 8;

/// Last ticket issued, last handed to the server, last answered.
static SUBMITTED: AtomicU64 = AtomicU64::new(0);
static TAKEN: AtomicU64 = AtomicU64::new(0);
static REPLIED: AtomicU64 = AtomicU64::new(0);
static REQUESTS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
static REPLIES: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

fn slot(ticket: u64) -> usize {
    usize::try_from((ticket - 1) % QUEUE_SLOTS).unwrap_or(0)
}

/// This component's init page; also records the export adapter's Return
/// key (idempotent, so every procedure can call it on entry).
fn setup() -> &'static EndpointInit {
    // SAFETY: the builder fills this component's init page with an
    // `EndpointInit` before any Invocation can reach it.
    let init: &EndpointInit = unsafe { init() };
    export::init_return_key(&ThreadReturnKey::provisioned(
        u32::try_from(init.guard).unwrap_or(0),
        u8::try_from(init.size_bits).unwrap_or(0),
    ));
    init
}

fn doorbell(init: &EndpointInit) -> NotificationKey {
    NotificationKey::from_key(RawKey::from_wire(init.doorbell))
}

fn done(init: &EndpointInit) -> EventCountKey {
    EventCountKey::from_key(RawKey::from_wire(init.done))
}

/// Hand the server the next queued request, blocking on the doorbell inside
/// the endpoint while the queue is empty. A pending doorbell bit may wake it
/// with nothing to take; it then simply waits again.
fn receive(init: &EndpointInit) -> PpcResult {
    loop {
        let taken = TAKEN.load(Ordering::Acquire);
        if taken < SUBMITTED.load(Ordering::Acquire) {
            let ticket = taken + 1;
            TAKEN.store(ticket, Ordering::Release);
            return PpcResult {
                r0: ticket,
                r1: REQUESTS[slot(ticket)].load(Ordering::Acquire),
            };
        }
        doorbell(init)
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
    let init = setup();
    let ticket = SUBMITTED.load(Ordering::Acquire) + 1;
    if ticket - REPLIED.load(Ordering::Acquire) > QUEUE_SLOTS {
        return PpcResult {
            r0: 0,
            r1: QUEUE_FULL,
        };
    }
    REQUESTS[slot(ticket)].store(request, Ordering::Release);
    SUBMITTED.store(ticket, Ordering::Release);
    doorbell(init)
        .signal(DOORBELL_BIT)
        .unwrap_or_else(|error| panic!("endpoint: doorbell signal failed: {:?}", error.code()));
    // Block here, inside the endpoint, until the server has replied to us.
    done(init)
        .await_ge(ticket, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("endpoint: done await failed: {:?}", error.code()));
    PpcResult {
        r0: ticket,
        r1: REPLIES[slot(ticket)].load(Ordering::Acquire),
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
    receive(setup())
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
    let init = setup();
    let replied = REPLIED.load(Ordering::Acquire);
    if ticket != replied + 1 || ticket > TAKEN.load(Ordering::Acquire) {
        return PpcResult {
            r0: 0,
            r1: BAD_REPLY,
        };
    }
    REPLIES[slot(ticket)].store(reply, Ordering::Release);
    REPLIED.store(ticket, Ordering::Release);
    // Wakes the client blocked in `send`; it runs once this Thread blocks.
    done(init)
        .advance(1)
        .unwrap_or_else(|error| panic!("endpoint: done advance failed: {:?}", error.code()));
    receive(init)
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
