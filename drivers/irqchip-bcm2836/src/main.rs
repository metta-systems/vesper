// Host tests (`host-tests`) build this binary natively too; it is only an
// empty `main` there.
#![cfg_attr(not(feature = "host-tests"), no_std, no_main)]

//! Privileged IC component for the Raspberry Pi 3 interrupt controllers.
//!
//! Built position-independent and linked with `drivers/privileged.ld`;
//! kickstart relocates it into the kernel half and the nucleus calls the
//! exported [`libirqchip::IrqChipOps`] table (see `libirqchip`).

use {
    core::cell::UnsafeCell,
    irqchip_bcm2836::{Bcm2836, LINE_COUNT},
    libirqchip::{ABI_VERSION, IrqChipConfig, IrqChipOps, SPURIOUS, irqchip_export},
};

/// The bound controllers. The nucleus calls the component on one core with
/// interrupts masked, so accesses never overlap.
struct ChipCell(UnsafeCell<Option<Bcm2836>>);

// SAFETY: see `ChipCell`: every access is serialized by the nucleus.
unsafe impl Sync for ChipCell {}

static CHIP: ChipCell = ChipCell(UnsafeCell::new(None));

/// The bound controllers, if `init` succeeded.
fn chip() -> Option<&'static mut Bcm2836> {
    // SAFETY: see `ChipCell`.
    unsafe { (*CHIP.0.get()).as_mut() }
}

unsafe extern "C" fn init(config: *const IrqChipConfig) -> i32 {
    // SAFETY: the nucleus passes a valid config whose regions it mapped.
    match unsafe { Bcm2836::new(&*config) } {
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
    chip().map_or(SPURIOUS, |chip| chip.claim())
}

/// The BCM controllers have no end-of-interrupt: a line stays pending until
/// its source is quiesced or the line is masked.
unsafe extern "C" fn complete(_line: u32) {}

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

/// Every BCM line is level triggered.
unsafe extern "C" fn set_trigger(_line: u32, level: u32) -> i32 {
    if level != 0 { 0 } else { -1 }
}

unsafe extern "C" fn xlate(kind: u32, cells: *const u32, count: u32) -> u32 {
    // SAFETY: the caller passes `count` readable cells.
    let cells = unsafe { core::slice::from_raw_parts(cells, count as usize) };
    Bcm2836::xlate(kind, cells)
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
    compatible = [
        // Kind 0, `irqchip_bcm2836::KIND_LOCAL`: the root controller.
        "brcm,bcm2836-l1-intc",
        // Kind 1, `irqchip_bcm2836::KIND_ARMCTRL`: cascaded on local line 8.
        "brcm,bcm2836-armctrl-ic",
    ]
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
