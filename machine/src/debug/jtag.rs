// SPDX-FileCopyrightText: 2024 Metta Systems OÜ
// SPDX-FileContributor: Berkus

//! JTAG helper functions.

use {
    crate::cpu::nop,
    core::ptr::{read_volatile, write_volatile},
};

#[unsafe(no_mangle)]
static mut WAIT_FLAG: bool = true;

/// Wait for debugger to attach.
/// Then in gdb issue `> set var *(&WAIT_FLAG) = 0`
/// from inside this function's frame to continue running.
pub fn wait_debugger() {
    while unsafe { read_volatile(&raw const WAIT_FLAG) } {
        nop();
    }
    // Reset the flag so that next jtag::wait_debugger() would block again.
    unsafe { write_volatile(&raw mut WAIT_FLAG, true) }
}
