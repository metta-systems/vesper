//! The privileged interrupt-controller component, as the nucleus sees it.
//!
//! Kickstart loads the component matching the device tree's root interrupt
//! controller into the kernel half and hands its [`PlatformHandoff`] to
//! [`install`] once, from the boot Thread at EL1 (see `libirqchip`). From then
//! on the nucleus calls the component directly, with interrupts masked, from
//! any core under the kernel lock. The secondary cores find the installed
//! component through [`is_installed`] and then each run [`init_cpu`].

use {
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering},
    },
    libirqchip::{
        ABI_VERSION, INVALID_LINE, IrqChipConfig, IrqChipOps, PlatformHandoff, SPURIOUS,
        TRIGGER_EDGE, TRIGGER_FIXED, TRIGGER_LEVEL,
    },
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

// SAFETY: see `PlatformCell`; boot installs before any interrupt can be taken,
// and another core reads it only after `INSTALLED` publishes it.
unsafe impl Sync for PlatformCell {}

static PLATFORM: PlatformCell = PlatformCell(UnsafeCell::new(None));

/// Set with release once [`PLATFORM`] holds an initialised component; read
/// with acquire by the cores that did not install it.
static INSTALLED: AtomicBool = AtomicBool::new(false);

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
    let Some((line, _trigger)) = translate(platform, timer.kind, timer.cells()) else {
        *slot = None;
        return Err(InstallError::NoTimerLine);
    };
    platform.physical_timer_line = line;
    INSTALLED.store(true, Ordering::Release);
    Ok(line)
}

/// Whether a component is installed.
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire) && platform().is_some()
}

/// The line an inter-processor interrupt arrives on, if a component is
/// installed.
pub fn ipi_line() -> Option<u32> {
    is_installed()
        .then(platform)
        .flatten()
        .map(|platform| platform.ops.ipi_line)
}

/// Prepare the calling core, logical CPU `cpu`, to take interrupts; whether
/// the component accepted it.
pub fn init_cpu(cpu: u32) -> bool {
    is_installed()
        .then(platform)
        .flatten()
        // SAFETY: the component's contract; called once per core, under the
        // kernel lock or on the boot core before any other core enters.
        .is_some_and(|platform| unsafe { (platform.ops.init_cpu)(cpu) } == 0)
}

/// Raise the IPI line on every logical CPU of `cpu_mask`; whether the
/// component sent it.
pub fn send_ipi(cpu_mask: u32) -> bool {
    is_installed()
        .then(platform)
        .flatten()
        // SAFETY: the component's contract; called under the kernel lock.
        .is_some_and(|platform| unsafe { (platform.ops.send_ipi)(cpu_mask) } == 0)
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

/// How a line is triggered, as an interrupt specifier asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The specifier leaves the trigger alone, or the line's is fixed.
    Fixed,
    Edge,
    Level,
}

/// Number of lines the installed component numbers, zero without one.
pub fn line_count() -> u32 {
    platform().map_or(0, |platform| platform.ops.line_count)
}

/// Decode `cells`, a device tree interrupt specifier relative to the
/// controller at `controller` in the component's list (the root first), into
/// a line below [`line_count`] and the trigger it asks for.
pub fn xlate(controller: usize, cells: &[u32]) -> Option<(u32, Trigger)> {
    let platform = platform()?;
    let kind = platform.config.controllers().get(controller)?.kind;
    translate(platform, kind, cells)
}

/// Configure `line` as level (`Trigger::Level`) or edge triggered; a
/// `Trigger::Fixed` request leaves it alone. Whether the component accepted.
pub fn set_trigger(line: u32, trigger: Trigger) -> bool {
    let level = match trigger {
        Trigger::Fixed => return true,
        Trigger::Edge => 0,
        Trigger::Level => 1,
    };
    platform().is_some_and(|platform| {
        // SAFETY: the component's contract.
        unsafe { (platform.ops.set_trigger)(line, level) == 0 }
    })
}

/// `xlate` through `platform`'s component for a controller of `kind`.
fn translate(platform: &Platform, kind: u32, cells: &[u32]) -> Option<(u32, Trigger)> {
    let count = u32::try_from(cells.len()).ok()?;
    let mut trigger = TRIGGER_FIXED;
    // SAFETY: `cells` is valid for `count` reads and `trigger` for one write
    // for the duration of the call.
    let line = unsafe { (platform.ops.xlate)(kind, cells.as_ptr(), count, &raw mut trigger) };
    if line == INVALID_LINE || line >= platform.ops.line_count {
        return None;
    }
    let trigger = match trigger {
        TRIGGER_EDGE => Trigger::Edge,
        TRIGGER_LEVEL => Trigger::Level,
        _ => Trigger::Fixed,
    };
    Some((line, trigger))
}
