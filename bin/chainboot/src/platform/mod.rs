/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The hardware chainboot needs to talk to its host: the PL011 UART the device
//! tree names, polled, the firmware mailbox that tells the UART's clock and
//! switches the LED, and a few facts about the board. Nothing here is a driver
//! framework: [`init`] does what bring-up takes, once. The UART, its pins and the
//! mailbox are `libboot_uart`'s, which Kickstart's early console shares.
//!
//! Every address comes from the device tree the firmware passes, so one
//! chainboot runs on every board it is built for.

pub mod led;
#[cfg(feature = "mmu")]
pub mod mmu;

pub use libboot_uart::{gpio, mailbox, pl011};
use {
    core::cell::UnsafeCell,
    libboot_uart::{
        ClockRate, ClockState, attach, guess_boot_hardware, route_header_pins, start_uart_clock,
    },
    libdevicetree::{Board, DeviceTree, INDEX_BUFFER_BYTES},
};

/// What the counter frequency register holds, in Hz. The firmware's stub is meant
/// to set it; a program that times anything by it fails without a sound when it
/// is zero.
#[must_use]
pub fn counter_frequency() -> u64 {
    use {aarch64_cpu::registers::CNTFRQ_EL0, tock_registers::interfaces::Readable};
    CNTFRQ_EL0.get()
}

/// Whether the MMU is on, as a word.
#[must_use]
pub fn mmu_state() -> &'static str {
    #[cfg(feature = "mmu")]
    {
        if mmu::enabled() {
            "on"
        } else {
            "off, it did not come on"
        }
    }
    #[cfg(not(feature = "mmu"))]
    {
        "off, not built in"
    }
}

/// Where the firmware loads chainboot's kernel to be run from: the kernel is
/// received here and jumped to.
pub const KERNEL_LOAD_ADDRESS: u64 = 0x8_0000;

/// The console UART, attached in [`init`] at the address the device tree gives:
/// the MMU is off, or the map is flat, so the address is the physical one, and
/// nothing else uses the UART.
static UART: pl011::Pl011 = pl011::Pl011::unattached();

/// The buffer the device tree's index lives in, for the rest of chainboot's run.
#[repr(C, align(16))]
struct IndexBuffer(UnsafeCell<[u8; INDEX_BUFFER_BYTES]>);

// SAFETY: handed out once, by `index_device_tree` on the one core chainboot
// runs on.
unsafe impl Sync for IndexBuffer {}

static INDEX_BUFFER: IndexBuffer = IndexBuffer(UnsafeCell::new([0; INDEX_BUFFER_BYTES]));

/// Index the device tree at `dtb`.
///
/// # Safety
///
/// Called once: the index buffer is handed out for good. `dtb` is the device
/// tree the firmware passed, intact for as long as chainboot runs.
unsafe fn index_device_tree(dtb: u32) -> Result<DeviceTree<'static, 'static>, &'static str> {
    let pointer = core::ptr::with_exposed_provenance::<u8>(dtb as usize);
    // SAFETY: the caller's contract.
    let blob = unsafe { DeviceTree::blob_from_raw(pointer) }
        .map_err(|_error| "no device tree where the firmware said")?;
    // SAFETY: the caller's contract: nothing else holds the buffer.
    let buffer = unsafe { &mut *INDEX_BUFFER.0.get() };
    DeviceTree::new(blob, buffer).map_err(|_error| "the device tree does not index")
}

/// What bring-up found out.
#[derive(Clone, Copy, Debug)]
pub struct Bringup {
    /// The board the device tree describes, or why the tree could not be read
    /// (the console is then the one of the board the core's type points to).
    pub board: Result<Board<'static>, &'static str>,
    pub uart_clock: ClockRate,
    pub clock_state: ClockState,
    /// The board's revision code, if the firmware said.
    pub board_revision: Option<u32>,
}

/// Find the board's hardware in the device tree at `dtb` (or, when the tree
/// cannot be read, take the board the core's type points to), route the PL011
/// to the header pins where it has them, get its clock running, set it up for
/// that clock and make it the console. Without the tree there is no LED and no
/// MMU: the memory nodes say where RAM is.
///
/// There is no console to say what happens, so the LED does. After starting the
/// UART clock it flashes once when the clock runs, twice when the firmware says
/// it is stopped, and three times when the firmware does not say. It stays off
/// while the UART is set up and goes on when that is done. A stopped clock
/// leaves the UART alone: touching a peripheral without its clock may hang the
/// core, and then nothing would tell what went wrong.
///
/// # Safety
///
/// Called once, with the device tree pointer the firmware passed.
///
/// # Errors
///
/// If neither the device tree nor the core's type names a PL011, or the UART
/// cannot be set up.
pub unsafe fn init(dtb: u32) -> Result<Bringup, &'static str> {
    // SAFETY: the caller's contract.
    let device_tree = unsafe { index_device_tree(dtb) };
    let hardware = match &device_tree {
        Ok(tree) => tree.boot_hardware(),
        Err(_why) => None,
    }
    .or_else(|| guess_boot_hardware().map(|(_, hardware)| hardware))
    .ok_or("neither the device tree nor the core's type names a PL011")?;

    // SAFETY: `hardware` gives the board's registers, the MMU is off, and
    // chainboot runs on one core; as for `UART`.
    unsafe { attach(&hardware, &UART) }?;
    // The pins go first: the UART should not drive them before they are its.
    // SAFETY: as for `attach`.
    unsafe {
        route_header_pins(&hardware);
    }
    let board_revision = mailbox::board_revision().ok();
    led::init(device_tree.as_ref().ok().and_then(DeviceTree::activity_led));

    // The LED is on while the MMU comes on, and stays on if it does not.
    #[cfg(feature = "mmu")]
    if let Ok(tree) = &device_tree {
        let _lit = led::set(true);
        mmu::enable(tree);
    }

    let (clock_state, uart_clock) = start_uart_clock(hardware.uart.clock);
    led::flash(match clock_state {
        ClockState::Running => 1,
        ClockState::Stopped => 2,
        ClockState::Unknown => 3,
    });
    let bringup = Bringup {
        board: device_tree.map(|tree| tree.board()),
        uart_clock,
        clock_state,
        board_revision,
    };
    if clock_state == ClockState::Stopped {
        return Ok(bringup);
    }

    UART.init(uart_clock.rate)?;
    let _lit = led::set(true);
    libconsole::console::register_console(&UART);
    Ok(bringup)
}

/// Switch the UART, running at `clock` Hz, to `baud_rate` once what was written
/// has left the line.
///
/// # Errors
///
/// If the rate is out of range or the transmitter does not drain; the UART then
/// stays at its rate.
pub fn set_uart_rate(clock: u32, baud_rate: u32) -> Result<(), &'static str> {
    UART.set_baud_rate(clock, baud_rate)
}
