//! JTAG helper functions.

use {
    core::ptr::{read_volatile, write_volatile},
    libcpu::nop,
};

#[unsafe(no_mangle)]
static mut WAIT_FLAG: bool = true;

/// Wait for debugger to attach.
/// Then in gdb issue `> set var *(&WAIT_FLAG) = 0`
/// from inside this function's frame to continue running.
pub fn wait_debugger() {
    // SAFETY: We're in a single core boot, only us and debugger can touch this flag.
    while unsafe { read_volatile(&raw const WAIT_FLAG) } {
        nop();
    }
    // Reset the flag so that next jtag::wait_debugger() would block again.
    // SAFETY: We're in a single core boot, only us and debugger can touch this flag.
    unsafe { write_volatile(&raw mut WAIT_FLAG, true) }
}
