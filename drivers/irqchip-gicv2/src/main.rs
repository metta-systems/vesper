// Host tests (`host-tests`) build this binary natively too; it is only an
// empty `main` there.
#![cfg_attr(not(feature = "host-tests"), no_std, no_main)]

//! Privileged IC component for the Arm `GICv2` (GIC-400: Raspberry Pi 4 and 5).
//!
//! Built position-independent and linked with `drivers/privileged.ld`;
//! kickstart relocates it into the kernel half and the nucleus calls the
//! exported [`libirqchip::IrqChipOps`] table (see `libirqchip`).

use {
    core::cell::UnsafeCell,
    irqchip_gicv2::{Gic400, LINE_COUNT},
    libirqchip::{ABI_VERSION, IrqChipConfig, IrqChipOps, SPURIOUS, irqchip_export},
};

/// The bound controller. The nucleus calls the component on one core with
/// interrupts masked, so accesses never overlap.
struct ChipCell(UnsafeCell<Option<Gic400>>);

// SAFETY: see `ChipCell`: every access is serialized by the nucleus.
unsafe impl Sync for ChipCell {}

static CHIP: ChipCell = ChipCell(UnsafeCell::new(None));

/// The bound controller, if `init` succeeded.
fn chip() -> Option<&'static mut Gic400> {
    // SAFETY: see `ChipCell`.
    unsafe { (*CHIP.0.get()).as_mut() }
}

unsafe extern "C" fn init(config: *const IrqChipConfig) -> i32 {
    // SAFETY: the nucleus passes a valid config whose regions it mapped.
    match unsafe { Gic400::new(&*config) } {
        Ok(bound) => {
            // SAFETY: see `ChipCell`.
            unsafe {
                *CHIP.0.get() = Some(bound);
            }
            0
        }
        Err(_) => -1,
    }
}

unsafe extern "C" fn claim() -> u32 {
    chip().map_or(SPURIOUS, Gic400::claim)
}

unsafe extern "C" fn complete(line: u32) {
    if let Some(chip) = chip() {
        chip.complete(line);
    }
}

unsafe extern "C" fn mask(line: u32) {
    if let Some(chip) = chip() {
        chip.set_enabled(line, false);
    }
}

unsafe extern "C" fn unmask(line: u32) {
    if let Some(chip) = chip() {
        chip.set_enabled(line, true);
    }
}

unsafe extern "C" fn set_trigger(line: u32, level: u32) -> i32 {
    match chip().map(|chip| chip.set_trigger(line, level != 0)) {
        Some(Ok(())) => 0,
        _ => -1,
    }
}

unsafe extern "C" fn xlate(_kind: u32, cells: *const u32, count: u32) -> u32 {
    // SAFETY: the caller passes `count` readable cells.
    let cells = unsafe { core::slice::from_raw_parts(cells, count as usize) };
    Gic400::xlate(cells)
}

irqchip_export!(
    IrqChipOps {
        abi_version: ABI_VERSION,
        line_count: LINE_COUNT,
        init,
        claim,
        complete,
        mask,
        unmask,
        set_trigger,
        xlate,
    },
    compatible = ["arm,gic-400", "arm,cortex-a15-gic"]
);

#[cfg(feature = "host-tests")]
fn main() {}

#[cfg(not(feature = "host-tests"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
