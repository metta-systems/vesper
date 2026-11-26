// Host tests (`host-tests`) build this binary natively too; it is only an
// empty `main` there.
#![cfg_attr(not(feature = "host-tests"), no_std, no_main)]

//! Start-method component for the spin table (`enable-method = "spin-table"`).
//!
//! Built position-independent and linked with `linker/cpu-start.ld`; Kickstart relocates
//! it and calls the exported [`libcpu_start::CpuStartOps`] table at EL2 with its MMU off
//! (see `libcpu_start`).

use {
    core::cell::UnsafeCell,
    cpu_start_spin_table::{ReleaseWord, SpinTable},
    libcpu_start::{ABI_VERSION, CpuStartConfig, CpuStartOps, StartStatus, cpu_start_export},
};

/// The table `init` accepted. Kickstart calls the component on the boot core alone, so
/// accesses never overlap.
struct TableCell(UnsafeCell<Option<SpinTable>>);

// SAFETY: see `TableCell`: one core calls the component.
unsafe impl Sync for TableCell {}

static TABLE: TableCell = TableCell(UnsafeCell::new(None));

/// The release words, as the hardware sees them.
struct Machine;

impl ReleaseWord for Machine {
    unsafe fn release(&mut self, word: u64, entry: u64) {
        // A plain store: with the MMU off an atomic read-modify-write never completes.
        // SAFETY: the caller passes a release word, a physical address the MMU-off
        // component reaches as is.
        unsafe {
            (word as *mut u64).write_volatile(entry);
        }
        // SAFETY: the word was just written through the same address.
        unsafe {
            libcpu::cache::clean_data_range(word, word + 8);
        }
        aarch64_cpu::asm::sev();
    }
}

unsafe extern "C" fn init(config: *const CpuStartConfig) -> i32 {
    // SAFETY: Kickstart passes a valid configuration.
    match SpinTable::new(unsafe { &*config }) {
        Ok(table) => {
            // SAFETY: see `TableCell`.
            unsafe {
                *TABLE.0.get() = Some(table);
            }
            StartStatus::Success as i32
        }
        Err(status) => status as i32,
    }
}

unsafe extern "C" fn start(cpu: u32, mpidr: u64, entry: u64, _context: u64) -> i32 {
    // SAFETY: see `TableCell`.
    let status = match unsafe { &*TABLE.0.get() } {
        Some(table) => table.start(cpu, mpidr, entry, &mut Machine),
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
    enable_method = ["spin-table"]
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
