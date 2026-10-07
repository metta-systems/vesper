//! The server party: a Thread living in its own `AddressSpace`. It only
//! calls the endpoint — `receive` once, then `reply_receive` per request —
//! and does the work in its own `AddressSpace` with server-private data.

use {
    crate::{PRIVATE_VA, call, endpoint},
    core::sync::atomic::{AtomicU64, Ordering},
    libaddress::PhysAddr,
    libobject::RawKey,
};

/// The server's Invocation stack slot inside the endpoint.
pub const ENDPOINT_STACK_SLOT: u64 = 2;

/// Server-private page at [`PRIVATE_VA`], mapped only in the server's root.
#[repr(C)]
struct ServerState {
    /// Server-table Invocation keys, written by the builder at init.
    receive: AtomicU64,
    reply_receive: AtomicU64,
    /// Data only the server's `AddressSpace` can read.
    salt: AtomicU64,
    /// Requests served, for the builder's checks.
    served: AtomicU64,
}

/// The server's work on one request.
pub const fn work(request: u64, salt: u64) -> u64 {
    request.wrapping_mul(3) ^ salt
}

fn direct(state_paddr: u64) -> &'static ServerState {
    // SAFETY: the builder owns the sanitized server state Frame and reaches
    // it through the invariant direct map; every field is atomic.
    unsafe { &*(PhysAddr::new(state_paddr).user_to_kernel().as_u64() as *const ServerState) }
}

/// Component init: record the server's keys and private salt.
pub fn init(state_paddr: u64, receive: RawKey, reply_receive: RawKey, salt: u64) {
    let state = direct(state_paddr);
    state.receive.store(receive.to_wire(), Ordering::Release);
    state
        .reply_receive
        .store(reply_receive.to_wire(), Ordering::Release);
    state.salt.store(salt, Ordering::Release);
}

pub fn served(state_paddr: u64) -> u64 {
    direct(state_paddr).served.load(Ordering::Acquire)
}

/// The server Thread. Never returns: it ends parked inside the endpoint,
/// waiting in `reply_receive` for a request that never comes.
pub extern "C" fn server_entry() -> ! {
    // SAFETY: the server state page is mapped RW at PRIVATE_VA in the
    // server's root only, and this Thread starts in the server.
    let state = unsafe { &*(PRIVATE_VA as *const ServerState) };
    let salt = state.salt.load(Ordering::Acquire);
    let receive = RawKey::from_wire(state.receive.load(Ordering::Acquire));
    let reply_receive = RawKey::from_wire(state.reply_receive.load(Ordering::Acquire));
    let (_, stack_end) = endpoint::stack_slot(ENDPOINT_STACK_SLOT);
    let (mut ticket, mut request) = call(receive, [0; 6], stack_end);
    loop {
        // Back in the server's own AddressSpace: only it can read `salt`.
        let reply = work(request, salt);
        state.served.fetch_add(1, Ordering::AcqRel);
        (ticket, request) = call(reply_receive, [ticket, reply, 0, 0, 0, 0], stack_end);
        assert_ne!(ticket, 0, "endpoint rejected the server's reply");
    }
}
