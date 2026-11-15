//! The board's console UART, polled, at the kernel-half address Kickstart
//! mapped it at (the nucleus binary's `nucleus_set_board_console`).
//!
//! The few lines that say how the components fare, a nucleus panic and what
//! components write to a `DebugConsole` go there, beside the `uart-pl011`
//! driver component that owns the UART. On hardware only: under QEMU the
//! lines are on semihosting already, and on the UART they would cut into the
//! log server's lines and the loopback tests' bytes, which the tests read
//! back.

use {
    core::sync::atomic::{AtomicBool, Ordering},
    libconsole::console::interface::{ConsoleOps, Write},
};

/// The UART. Unattached until [`attach`].
pub static BOARD_UART: libboot_uart::pl011::Pl011 = libboot_uart::pl011::Pl011::unattached();

/// Whether [`BOARD_UART`] is attached. Only loaded and stored.
static ATTACHED: AtomicBool = AtomicBool::new(false);

/// Use the console UART whose registers are mapped at `base` in the kernel
/// half.
///
/// # Safety
/// Boot-only, from the single boot core with interrupts masked. `base` is the
/// kernel-half address of a PL011's registers, mapped as device memory for as
/// long as the nucleus runs.
pub unsafe fn attach(base: usize) {
    // SAFETY: the caller's contract.
    unsafe {
        BOARD_UART.attach(base);
    }
    ATTACHED.store(true, Ordering::Relaxed);
}

/// Whether there is a board console to print on.
pub fn attached() -> bool {
    ATTACHED.load(Ordering::Relaxed)
}

/// Whether lines go to the board console: one is attached, outside QEMU.
fn printing() -> bool {
    !cfg!(feature = "qemu") && attached()
}

/// Print `line` and a newline on the board console.
pub fn line(line: core::fmt::Arguments) {
    if printing() {
        let _ignored = BOARD_UART.write_fmt(format_args!("{line}\n"));
    }
}

/// Print `bytes` on the board console as they are, each malformed UTF-8
/// sequence as U+FFFD.
pub fn bytes(bytes: &[u8]) {
    if printing() {
        for chunk in bytes.utf8_chunks() {
            BOARD_UART.write_string(chunk.valid());
            if !chunk.invalid().is_empty() {
                BOARD_UART.write_string("\u{FFFD}");
            }
        }
    }
}
