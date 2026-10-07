#![no_std]

//! What the endpoint-test parties agree on: the per-component init page the
//! builder writes, the client report page, the requests and the server's work.
//!
//! The init page is a fixture convention standing in for the open
//! init-handoff design: the builder writes each component's keys and
//! parameters into a page mapped at [`INIT_VA`] in that component only.

use core::sync::atomic::AtomicU64;

/// Each component's init page (written by the builder before the component
/// runs).
pub const INIT_VA: u64 = 0x2000_0000;
/// The client's report page, right after its init page.
pub const REPORT_VA: u64 = INIT_VA + 0x1000;
/// The 2 MiB span holding each component's guarded stacks.
pub const STACK_REGION: u64 = 0x3000_0000;

/// Doorbell bit the endpoint signals when a request is queued.
pub const DOORBELL_BIT: u64 = 1;
/// `send` result word 1 when the queue is full (no ticket issued).
pub const QUEUE_FULL: u64 = u64::MAX;
/// `reply_receive` result word 1 when the reply is not for the next
/// unanswered ticket.
pub const BAD_REPLY: u64 = u64::MAX - 1;

/// Requests each client Thread sends, in order. Client 0 sends twice, so its
/// second request reaches a server already parked inside the endpoint.
pub const CLIENT_REQUESTS: [&[u64]; 2] = [&[0xA1, 0xC3], &[0xB2]];

/// The server's work on one request.
/// It is exposed from endpoint-protocol so that the main app can assert the calculation results.
pub const fn work(request: u64, salt: u64) -> u64 {
    request.wrapping_mul(3) ^ salt
}

/// The endpoint's init page: its own table's guard and size (for its
/// `Thread.Return` key) and its two synchronization objects.
#[repr(C)]
pub struct EndpointInit {
    pub guard: u64,
    pub size_bits: u64,
    pub doorbell: u64,
    pub done: u64,
}

/// The server's init page: its two endpoint Invocations, the endpoint stack
/// it uses with them, and its private salt.
#[repr(C)]
pub struct ServerInit {
    pub receive: u64,
    pub reply_receive: u64,
    pub stack_end: u64,
    pub salt: u64,
}

/// The client `AddressSpace`'s init page, shared by its Threads: one `send`
/// Invocation and endpoint stack per client Thread, the Notification that
/// tells the builder a client is done, and one to park on.
#[repr(C)]
pub struct ClientInit {
    pub send: [u64; 2],
    pub stack_end: [u64; 2],
    pub done: u64,
    pub park: u64,
}

/// Where client Threads record `(ticket, reply)` per request, for the
/// builder to check.
#[repr(C)]
pub struct ClientReport {
    pub replies: [[[AtomicU64; 2]; 2]; 2],
}

/// This component's init page.
///
/// # Safety
///
/// The calling component's init page holds a `T` at [`INIT_VA`].
pub unsafe fn init<T>() -> &'static T {
    // SAFETY: forwarded caller contract; the builder maps and fills the page
    // before the component runs, and never changes it afterwards.
    unsafe { &*(INIT_VA as *const T) }
}
