#![no_std]
#![no_main]

//! log-test's program: the last words of a component reach the UART through
//! the log server.
//!
//! - The first Thread logs that it is ready, waits for the second to fault,
//!   then panics on purpose: the runtime's panic handler has `liblog_client`
//!   log `PANICKED: log_probe: … at <file>:<line>`.
//! - The second Thread recurses until it runs into the guard page below its
//!   stack: the kernel delivers the fault to the handler `liblog_client`
//!   exports, which logs `FAULT: log_probe thread 1: stack overflow (guard
//!   page) …` and stops the Thread.
//!
//! Under QEMU the panic ends the run (semihosting exit); `just test-log`
//! checks the UART file for both lines.

use {
    libdevice_driver::{driver_init, driver_threads},
    liblog_client::log_println,
    libobject::{EventCountKey, RawKey},
};

libuser::entry!(main);

driver_threads!(overflow);

/// Bytes of each frame the overflowing Thread pushes: well below a guard
/// page, so the recursion meets the guard rather than jumping over it.
const FRAME_BYTES: usize = 512;

/// How long the first Thread gives the second to fault and report.
const FAULT_WAIT_NS: u64 = 500_000_000;

/// One level of an unbounded recursion with a frame of [`FRAME_BYTES`].
#[inline(never)]
fn descend(depth: u64) -> u64 {
    let mut frame = [0_u8; FRAME_BYTES];
    frame[0] = depth.to_le_bytes()[0];
    core::hint::black_box(&mut frame);
    if depth == u64::MAX {
        return 0;
    }
    descend(depth + 1).wrapping_add(u64::from(core::hint::black_box(frame[0])))
}

/// The second Thread: overflows its stack. The fault handler stops it.
fn overflow(_init_va: u64) -> ! {
    log_println!("log-probe: thread 1 overflows its stack");
    let depth = descend(0);
    panic!("log-probe: the recursion returned {depth}")
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    log_println!("log-probe: ready");
    // Nobody advances the component's own EventCount: the wait is a sleep.
    let _slept = EventCountKey::from_key(RawKey::from_wire(init.event_count)).sleep(FAULT_WAIT_NS);
    panic!("made to panic on purpose")
}
