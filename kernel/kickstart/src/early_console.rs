//! Kickstart's early console: its own boot lines on the PL011, before any driver
//! runs, so that a hang early in the boot shows on the board.
//!
//! [`start`] runs first thing at EL2, with the MMU off (`libboot` turns it off
//! for EL1 and EL2), where a store-exclusive never completes on the board: nothing
//! here or in `libboot_uart` uses an atomic read-modify-write.
//!
//! - **Started by chainboot.** `x1` and `x2` at entry hold chainboot's handoff
//!   (`libchainboot_protocol::kept_rate`): chainboot routed the pins, set the UART
//!   up and negotiated its rate with the host. The UART is used as it is.
//! - **Started by the firmware** (from the SD card). The pins are routed to the
//!   PL011 (GPIO 14 and 15 to ALT0, BCM2837 ARM Peripherals sections 6.1 and 6.2),
//!   the firmware is asked to run the UART clock and to say its rate, and the
//!   UART is set up at 115200 8N1 with FIFOs, every interrupt masked (PL011 TRM,
//!   DDI 0183G, section 3.3: UARTIBRD, UARTFBRD, `UARTLCR_H`, UARTCR, UARTIMSC).
//!   This is done even when the firmware
//!   enabled it (`enable_uart=1`): its rate is never the one to keep. Then the
//!   console negotiation of the protocol crate runs, the same rate request and
//!   answers as chainboot's: a host that answers within 1.5 s gets its rate.
//!
//! [`hand_over`] ends the early console just before the MMU goes on and Kickstart
//! drops to EL1, where the UART is not mapped: from there on Kickstart prints on
//! semihosting only, and the `uart-pl011` driver component is the UART's one
//! owner. The driver keeps the divisors it finds, so the rate carries over to the
//! log server's output.

use {
    core::{
        fmt,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    },
    libboot_uart::{DEFAULT_UART_CLOCK, GPIO_BASE, PL011_BASE, gpio, mailbox, pl011::Pl011},
    libchainboot_protocol::{
        BOOT_BAUD_RATE, CONSOLE_ACK, CONSOLE_BEACON, CONSOLE_CONFIRM_WAIT, CONSOLE_REQUEST_GAP,
        CONSOLE_WAIT, Opening, RATE_ACCEPTED, RATE_REFUSED, RATE_REQUEST_BODY_LENGTH,
        RATE_REQUEST_MAGIC, RateRefusal, Run, SWITCH_SETTLE, decode_rate_request_body, kept_rate,
        opening, validate_rate,
    },
    libconsole::{SerialOps, console::interface::Write},
    libqemu::semihosting as semi,
    libtime::time::time_manager,
};

// SAFETY: the PL011 is at this address on every board Kickstart is built for, the
// MMU is off while the early console is open so the address is the physical one,
// and nothing else uses the UART until `hand_over`.
static UART: Pl011 = unsafe { Pl011::new(PL011_BASE) };

/// Whether the early lines go to the UART: from [`start`] to [`hand_over`].
/// Only loaded and stored.
static OPEN: AtomicBool = AtomicBool::new(false);

/// How the early console came to its rate.
#[derive(Clone, Copy, Debug)]
pub enum Console {
    /// Chainboot left the UART at this rate and negotiated it with the host.
    Kept { rate: u32 },
    /// The host asked for this rate, and confirmed it.
    Negotiated { rate: u32 },
    /// Nobody asked for another rate within the window.
    NoHost,
    /// The host asked for a rate the UART cannot make.
    Refused(RateRefusal),
    /// The host asked for this rate but did not confirm it there.
    NotConfirmed { rate: u32 },
    /// The UART did not drain to switch.
    NotSwitched { rate: u32, why: &'static str },
    /// The UART is left alone: the firmware says its clock is stopped (touching
    /// a peripheral without its clock may hang the core), or the clock makes no
    /// divisors for the boot rate.
    NoUart { why: &'static str },
}

impl fmt::Display for Console {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kept { rate } => write!(formatter, "{rate} baud, as chainboot left it"),
            Self::Negotiated { rate } => write!(formatter, "{rate} baud, as the host asked"),
            Self::NoHost => write!(
                formatter,
                "{BOOT_BAUD_RATE} baud, no host asked for another"
            ),
            Self::Refused(refusal) => {
                write!(formatter, "{BOOT_BAUD_RATE} baud, refused: {refusal}")
            }
            Self::NotConfirmed { rate } => write!(
                formatter,
                "{BOOT_BAUD_RATE} baud, the host did not confirm {rate} baud"
            ),
            Self::NotSwitched { rate, why } => write!(
                formatter,
                "{BOOT_BAUD_RATE} baud, could not switch to {rate} baud: {why}"
            ),
            Self::NoUart { why } => write!(formatter, "none: {why}"),
        }
    }
}

/// Open the early console: keep chainboot's UART when `loader_registers` (`x1`
/// and `x2` at entry) hold its handoff, otherwise set the UART up and negotiate
/// its rate with the host. Say how it went.
pub fn start(loader_registers: [u64; 2]) -> Console {
    if let Some(rate) = kept_rate(loader_registers) {
        OPEN.store(true, Ordering::Relaxed);
        return Console::Kept { rate };
    }

    // The pins go first: the UART should not drive them before they are its.
    // SAFETY: the GPIO registers are at `GPIO_BASE`, and nothing else uses them
    // this early.
    unsafe {
        gpio::route_uart(GPIO_BASE);
    }
    let _started = mailbox::start_uart_clock();
    if mailbox::uart_clock_running() == Ok(false) {
        return Console::NoUart {
            why: "the firmware says the UART clock is stopped",
        };
    }
    let clock = mailbox::uart_clock_rate().unwrap_or(DEFAULT_UART_CLOCK);
    if let Err(why) = UART.init(clock) {
        return Console::NoUart { why };
    }
    OPEN.store(true, Ordering::Relaxed);
    let console = negotiate(clock);
    // The beacons and answers end, the boot lines start on a line of their own.
    print_line(format_args!(""));
    console
}

/// Print `line` and a newline on the early console, while it is open.
pub fn print_line(line: fmt::Arguments) {
    if OPEN.load(Ordering::Relaxed) {
        let _ignored = UART.write_fmt(format_args!("{line}\n"));
    }
}

/// Close the early console before the MMU goes on: what was written leaves the
/// line, and the UART is the `uart-pl011` driver's from now on.
pub fn hand_over() {
    if OPEN.load(Ordering::Relaxed) {
        UART.flush();
        OPEN.store(false, Ordering::Relaxed);
    }
}

/// Print a boot line on semihosting (under `qemu`) and on the early console.
#[macro_export]
macro_rules! early_println {
    ($($arg:tt)+) => {{
        ::libqemu::semihosting::println!($($arg)+);
        $crate::early_console::print_line(::core::format_args!($($arg)+));
    }};
}

/// How long it is since `start`, in uptime.
fn since(start: Duration) -> Duration {
    time_manager().uptime().saturating_sub(start)
}

fn send(bytes: &[u8]) {
    for byte in bytes {
        UART.write_byte(*byte);
    }
    UART.flush();
}

/// Offer the console (the protocol crate's "The console negotiation"), and switch
/// to the rate the host asks for when it confirms it.
fn negotiate(clock: u32) -> Console {
    UART.clear_rx();
    let Some(request) = wait_for_rate_request() else {
        return Console::NoHost;
    };
    let rate = match request.and_then(|rate| validate_rate(clock, rate).map(|_divisors| rate)) {
        Ok(rate) => rate,
        Err(refusal) => {
            print_line(format_args!(
                "{RATE_REFUSED}\n🥾 Staying at the rate: {refusal}"
            ));
            return Console::Refused(refusal);
        }
    };
    let _ignored = UART.write_fmt(format_args!("{RATE_ACCEPTED}"));
    // The answer leaves the line at the old rate before the switch.
    if let Err(why) = UART.set_baud_rate(clock, rate) {
        return Console::NotSwitched { rate, why };
    }
    time_manager().spin_for(SWITCH_SETTLE);
    if confirmed() {
        semi::println!("🥾 Console at {rate} baud, the host confirmed it");
        return Console::Negotiated { rate };
    }
    if let Err(why) = UART.set_baud_rate(clock, BOOT_BAUD_RATE) {
        // Only the transmitter not draining fails here: nobody hears it.
        semi::println!("🥾 Could not go back to {BOOT_BAUD_RATE} baud: {why}");
    }
    time_manager().spin_for(SWITCH_SETTLE);
    Console::NotConfirmed { rate }
}

/// Beacon as [`CONSOLE_WAIT`] says until a rate request arrives: its rate, or
/// why it is no good. `None` when the window closes first. Anything else that
/// arrives is dropped.
fn wait_for_rate_request() -> Option<Result<u32, RateRefusal>> {
    let started = time_manager().uptime();
    loop {
        if CONSOLE_WAIT.gives_up(since(started)) {
            return None;
        }
        send(&CONSOLE_BEACON);
        let beaconed = time_manager().uptime();
        while since(beaconed) < CONSOLE_WAIT.beacon_period {
            let Some(first) = UART.read_byte_nonblocking() else {
                continue;
            };
            if first != RATE_REQUEST_MAGIC[0] {
                continue;
            }
            let Some(magic) = read_request::<8>(Some(first)) else {
                continue;
            };
            if opening(magic) != Opening::RateRequest {
                continue;
            }
            let Some(body) = read_request::<RATE_REQUEST_BODY_LENGTH>(None) else {
                continue;
            };
            semi::println!("🥾 Console rate request after {:?}", since(started));
            return Some(decode_rate_request_body(body).ok_or(RateRefusal::Garbled));
        }
    }
}

/// The next `N` bytes of a rate request, the first `first` when it has been read
/// already; `None` when a gap between two is longer than [`CONSOLE_REQUEST_GAP`].
fn read_request<const N: usize>(first: Option<u8>) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = match (index, first) {
            (0, Some(first)) => first,
            _ => read_within(CONSOLE_REQUEST_GAP)?,
        };
    }
    Some(bytes)
}

/// The next byte, if it arrives within `limit`.
fn read_within(limit: Duration) -> Option<u8> {
    let started = time_manager().uptime();
    while since(started) < limit {
        if let Some(byte) = UART.read_byte_nonblocking() {
            return Some(byte);
        }
    }
    None
}

/// Beacon at the new rate as [`CONSOLE_CONFIRM_WAIT`] says; whether the host
/// answers with [`CONSOLE_ACK`].
fn confirmed() -> bool {
    let started = time_manager().uptime();
    let mut ack = Run::of(CONSOLE_ACK);
    loop {
        if CONSOLE_CONFIRM_WAIT.gives_up(since(started)) {
            return false;
        }
        send(&CONSOLE_BEACON);
        let beaconed = time_manager().uptime();
        while since(beaconed) < CONSOLE_CONFIRM_WAIT.beacon_period {
            if UART
                .read_byte_nonblocking()
                .is_some_and(|byte| ack.push(byte))
            {
                return true;
            }
        }
    }
}
