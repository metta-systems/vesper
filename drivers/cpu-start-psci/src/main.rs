// Host tests (`host-tests`) build this binary natively too; it is only an
// empty `main` there.
#![cfg_attr(not(feature = "host-tests"), no_std, no_main)]

//! Start-method component for PSCI (`enable-method = "psci"`).
//!
//! Built position-independent and linked with `linker/cpu-start.ld`; Kickstart relocates
//! it and calls the exported [`libcpu_start::CpuStartOps`] table at EL2 with its MMU off
//! (see `libcpu_start`).

use {
    core::{arch::asm, cell::UnsafeCell},
    cpu_start_psci::{Firmware, Psci},
    libcpu_start::{
        ABI_VERSION, Conduit, CpuStartConfig, CpuStartOps, StartStatus, cpu_start_export,
    },
};

/// The configuration `init` accepted. Kickstart calls the component on the boot core
/// alone, so accesses never overlap.
struct PsciCell(UnsafeCell<Option<Psci>>);

// SAFETY: see `PsciCell`: one core calls the component.
unsafe impl Sync for PsciCell {}

static PSCI: PsciCell = PsciCell(UnsafeCell::new(None));

/// The firmware behind `SMC` or `HVC`.
struct Machine;

impl Firmware for Machine {
    unsafe fn call(&mut self, conduit: Conduit, function: u32, x1: u64, x2: u64, x3: u64) -> i64 {
        let mut x0 = u64::from(function);
        // SMC Calling Convention: the call takes x0..x3 and may change x0..x17.
        match conduit {
            // SAFETY: the caller passes a PSCI call the firmware implements.
            Conduit::Smc => unsafe {
                asm!(
                    "smc #0",
                    inout("x0") x0,
                    inout("x1") x1 => _, inout("x2") x2 => _, inout("x3") x3 => _,
                    out("x4") _, out("x5") _, out("x6") _, out("x7") _,
                    out("x8") _, out("x9") _, out("x10") _, out("x11") _,
                    out("x12") _, out("x13") _, out("x14") _, out("x15") _,
                    out("x16") _, out("x17") _,
                    options(nostack),
                );
            },
            // SAFETY: as above.
            Conduit::Hvc => unsafe {
                asm!(
                    "hvc #0",
                    inout("x0") x0,
                    inout("x1") x1 => _, inout("x2") x2 => _, inout("x3") x3 => _,
                    out("x4") _, out("x5") _, out("x6") _, out("x7") _,
                    out("x8") _, out("x9") _, out("x10") _, out("x11") _,
                    out("x12") _, out("x13") _, out("x14") _, out("x15") _,
                    out("x16") _, out("x17") _,
                    options(nostack),
                );
            },
            Conduit::None => return cpu_start_psci::psci_return::NOT_SUPPORTED,
        }
        x0.cast_signed()
    }
}

unsafe extern "C" fn init(config: *const CpuStartConfig) -> i32 {
    // SAFETY: Kickstart passes a valid configuration.
    match Psci::new(unsafe { &*config }) {
        Ok(psci) => {
            // SAFETY: see `PsciCell`.
            unsafe {
                *PSCI.0.get() = Some(psci);
            }
            StartStatus::Success as i32
        }
        Err(status) => status as i32,
    }
}

unsafe extern "C" fn start(cpu: u32, mpidr: u64, entry: u64, context: u64) -> i32 {
    // SAFETY: see `PsciCell`.
    let status = match unsafe { &*PSCI.0.get() } {
        Some(psci) => psci.start(cpu, mpidr, entry, context, &mut Machine),
        None => StartStatus::InvalidConfig,
    };
    status as i32
}

cpu_start_export!(
    CpuStartOps {
        abi_version: ABI_VERSION,
        reserved: 0,
        init,
        start,
    },
    enable_method = ["psci"]
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
