//! JTAG helper functions.

use {
    aarch64_cpu::asm,
    core::ptr::{read_volatile, write_volatile},
};

#[no_mangle]
static mut WAIT_FLAG: bool = true;

/// Wait for debugger to attach.
/// Then in gdb issue `> set var *(&WAIT_FLAG) = 0`
/// from inside this function's frame to continue running.
pub fn wait_debugger() {
    while unsafe { read_volatile(&WAIT_FLAG) } {
        asm::nop();
    }
    // Reset the flag so that next jtag::wait_debugger() would block again.
    unsafe { write_volatile(&mut WAIT_FLAG, true) }
}
