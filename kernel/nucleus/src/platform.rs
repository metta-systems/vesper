//! The privileged interrupt-controller component, as the nucleus sees it.
//!
//! Kickstart loads the component matching the device tree's root interrupt
//! controller into the kernel half and hands its [`PlatformHandoff`] to
//! [`install`] once, from the boot Thread at EL1 (see `libirqchip`). From then
//! on the nucleus calls the component directly, on the single boot core with
//! interrupts masked.

use {
    core::cell::UnsafeCell,
    libirqchip::{ABI_VERSION, INVALID_LINE, IrqChipConfig, IrqChipOps, PlatformHandoff, SPURIOUS},
};

/// Why [`install`] refused a handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallError {
    /// A component is already installed
    AlreadyInstalled,
    /// The component was built for another ABI version
    AbiMismatch,
    /// The component's `init` rejected the configuration
    InitFailed,
    /// The physical timer specifier does not name a line of the component
    NoTimerLine,
}

/// The installed component.
struct Platform {
    ops: &'static IrqChipOps,
    /// The nucleus's own copy: the component keeps pointers derived from it
    config: IrqChipConfig,
    /// The line of the CPU's non-secure EL1 physical timer (`CNTP`)
    physical_timer_line: u32,
}

/// Written once by [`install`] at boot, read-only afterwards.
struct PlatformCell(UnsafeCell<Option<Platform>>);

// SAFETY: see `PlatformCell`; the nucleus runs on one core with interrupts
// masked, and boot installs before any interrupt can be taken.
unsafe impl Sync for PlatformCell {}

static PLATFORM: PlatformCell = PlatformCell(UnsafeCell::new(None));

fn platform() -> Option<&'static Platform> {
    // SAFETY: see `PlatformCell`.
    unsafe { (*PLATFORM.0.get()).as_ref() }
}

/// Install the component described by `handoff`: check its ABI, let it take
/// the controllers and decode the physical timer line, which is returned.
///
/// # Safety
///
/// Boot-only, once, on the single boot core with interrupts masked, after
/// kickstart mapped the component and its MMIO as described by `handoff`.
pub unsafe fn install(handoff: &PlatformHandoff) -> Result<u32, InstallError> {
    // SAFETY: see `PlatformCell`: boot is the only writer.
    let slot = unsafe { &mut *PLATFORM.0.get() };
    if slot.is_some() {
        return Err(InstallError::AlreadyInstalled);
    }
    // SAFETY: kickstart mapped the relocated operation table at `ops`.
    let ops = unsafe { &*(handoff.ops as *const IrqChipOps) };
    if ops.abi_version != ABI_VERSION {
        return Err(InstallError::AbiMismatch);
    }
    let platform = slot.insert(Platform {
        ops,
        config: handoff.config,
        physical_timer_line: INVALID_LINE,
    });
    // SAFETY: the component's contract; the config lives in the static.
    if unsafe { (ops.init)(&raw const platform.config) } != 0 {
        *slot = None;
        return Err(InstallError::InitFailed);
    }
    let timer = handoff.physical_timer;
    let cells = timer.cells();
    // `cells()` is clamped to the array, so the length always fits.
    let count = u32::try_from(cells.len()).unwrap_or(0);
    // SAFETY: `cells` is valid for `count` reads for the duration of the call.
    let line = unsafe { (ops.xlate)(timer.kind, cells.as_ptr(), count) };
    if line >= ops.line_count {
        *slot = None;
        return Err(InstallError::NoTimerLine);
    }
    platform.physical_timer_line = line;
    Ok(line)
}

/// Whether a component is installed.
pub fn is_installed() -> bool {
    platform().is_some()
}

/// The physical timer's line, if a component is installed.
pub fn physical_timer_line() -> Option<u32> {
    platform().map(|platform| platform.physical_timer_line)
}

/// The highest-priority pending line, if any.
pub fn claim() -> Option<u32> {
    let platform = platform()?;
    // SAFETY: the component's contract (interrupts masked, single core).
    let line = unsafe { (platform.ops.claim)() };
    (line != SPURIOUS).then_some(line)
}

/// End-of-interrupt for a claimed `line`.
pub fn complete(line: u32) {
    if let Some(platform) = platform() {
        // SAFETY: the component's contract.
        unsafe {
            (platform.ops.complete)(line);
        }
    }
}

/// Stop delivering `line`.
pub fn mask(line: u32) {
    if let Some(platform) = platform() {
        // SAFETY: the component's contract.
        unsafe {
            (platform.ops.mask)(line);
        }
    }
}

/// Allow delivering `line`.
pub fn unmask(line: u32) {
    if let Some(platform) = platform() {
        // SAFETY: the component's contract.
        unsafe {
            (platform.ops.unmask)(line);
        }
    }
}
