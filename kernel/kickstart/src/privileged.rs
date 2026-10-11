//! Select, load and map the privileged interrupt-controller component.
//!
//! The root interrupt controller is the one the CPU's generic timer is wired
//! to: the (inherited) `interrupt-parent` of the `arm,armv7-timer` /
//! `arm,armv8-timer` node. The bundled [`PrivilegedImage`] whose
//! `compatible` list names it is loaded, together with every other enabled
//! interrupt controller node it drives (cascaded controllers).
//!
//! The component is relocated to [`PRIVILEGED_BASE`] in the kernel half and
//! its controllers' MMIO is mapped after it, each region behind a guard page.
//! Kickstart runs at EL2 here, where the kernel half is not live, so it only
//! records a [`PlatformHandoff`]; the nucleus calls the component once the
//! boot Thread runs at EL1 (see `bootstrap`).

use {
    crate::{
        boot_info::{BOOT_INFO, InterruptControllers},
        memory::{Alloc, MemoryPermissions},
        paging::{self, MmuSetup},
    },
    core::cell::UnsafeCell,
    libaddress::{PhysAddr, VirtAddr},
    libdevicetree::{
        DeviceTree, Node,
        interrupts::{
            MAX_INTERRUPT_CELLS, TimerInterrupt, interrupt_controllers, root_interrupt_controller,
            timer_interrupt,
        },
        is_compatible, phandle, regions,
    },
    libimage::{PAGE_BYTES, PAGE_SIZE, PrivilegedImage},
    libirqchip::{
        Controller, IrqChipConfig, MAX_CONTROLLERS, MAX_REGIONS, MAX_SPECIFIER_CELLS, MmioRegion,
        PlatformHandoff, Specifier,
    },
    liblocking::interface::Mutex,
    libqemu::semihosting as semi,
};

/// Where the privileged component is linked into the kernel half: the
/// otherwise unused TTBR1 L0 slot 257.
pub const PRIVILEGED_BASE: u64 = 0xFFFF_8080_0000_0000;

// A timer specifier from the device tree fits the component ABI unchanged.
const _: () = assert!(MAX_INTERRUPT_CELLS == MAX_SPECIFIER_CELLS);

/// A controller the component will drive: its kind, phandle and CPU physical
/// MMIO.
#[derive(Clone, Copy, Default)]
struct PlannedController {
    kind: u32,
    /// The node's device tree phandle, zero if it has none
    phandle: u32,
    region_count: usize,
    /// `(physical start, size)` of each `reg` entry
    regions: [(u64, u64); MAX_REGIONS],
}

/// What the device tree asks for, decided before any memory is committed.
pub struct IrqChipPlan {
    image: &'static PrivilegedImage,
    controller_count: usize,
    controllers: [PlannedController; MAX_CONTROLLERS],
    physical_timer: Specifier,
}

/// The first of `node`'s `compatible` strings `image` drives, as a kind.
fn kind_in(image: &PrivilegedImage, node: &Node<'_, '_, '_>) -> Option<u32> {
    image
        .compatible
        .iter()
        .find(|compatible| is_compatible(node, compatible))
        .and_then(|compatible| image.kind_of(compatible))
}

fn plan_controller(kind: u32, node: &Node<'_, '_, '_>) -> PlannedController {
    let mut planned = PlannedController {
        kind,
        phandle: phandle(node).unwrap_or_default(),
        ..PlannedController::default()
    };
    for region in regions(node).take(MAX_REGIONS) {
        planned.regions[planned.region_count] = (region.start, region.size);
        planned.region_count += 1;
    }
    planned
}

/// Pick the IC component for the device tree's root interrupt controller.
///
/// # Errors
///
/// If the tree has no generic timer, its controller is unknown to every
/// bundled component, or the timer interrupt cannot be read.
pub fn plan(
    device_tree: &DeviceTree<'_, '_>,
    images: &[&'static PrivilegedImage],
) -> Result<IrqChipPlan, &'static str> {
    let tree = device_tree.index();
    let root =
        root_interrupt_controller(tree).ok_or("no generic timer with an interrupt-parent")?;
    let (image, root_kind) = images
        .iter()
        .find_map(|image| kind_in(image, &root).map(|kind| (*image, kind)))
        .ok_or("no bundled component drives the root interrupt controller")?;

    let mut plan = IrqChipPlan {
        image,
        controller_count: 0,
        controllers: [PlannedController::default(); MAX_CONTROLLERS],
        physical_timer: Specifier::default(),
    };
    // The root first, then the cascaded controllers the component drives:
    // the order `IRQControl.Issue` numbers them by.
    for node in
        interrupt_controllers(tree, |node| kind_in(image, node).is_some()).take(MAX_CONTROLLERS)
    {
        let kind = kind_in(image, &node).unwrap_or(root_kind);
        plan.controllers[plan.controller_count] = plan_controller(kind, &node);
        plan.controller_count += 1;
    }

    let timer = timer_interrupt(tree, TimerInterrupt::NonSecurePhysical)
        .ok_or("cannot read the physical timer interrupt")?;
    if Some(timer.controller) != phandle(&root) {
        return Err("the physical timer is not routed to the root controller");
    }
    plan.physical_timer = Specifier {
        kind: root_kind,
        cell_count: timer.cell_count,
        cells: timer.cells,
    };

    semi::println!(
        "🥾 Interrupt controller: {} ({} controllers), physical timer {:?}",
        image.name,
        plan.controller_count,
        plan.physical_timer.cells()
    );
    Ok(plan)
}

/// Load the planned component at [`PRIVILEGED_BASE`], map it and its MMIO.
///
/// # Errors
///
/// If memory cannot be allocated or a mapping fails.
pub fn load(plan: &IrqChipPlan, setup: &mut MmuSetup) -> Result<PlatformHandoff, &'static str> {
    let image = plan.image;
    let size = image.total_size();
    let phys = setup
        .allocator()
        .alloc_pages(
            size / PAGE_BYTES,
            ("⚡ Interrupt controller", Alloc::Persistent),
        )
        .ok_or("cannot allocate the interrupt controller component")?;
    // SAFETY: kickstart runs with the MMU off at EL2; the freshly allocated
    // pages are identity-accessible and exclusively ours.
    let memory = unsafe { core::slice::from_raw_parts_mut(phys.as_mut_ptr::<u8>(), size) };
    image
        .load_into(memory, PRIVILEGED_BASE)
        .map_err(|_load_error| "cannot lay out the interrupt controller component")?;

    for segment in image.segments {
        let pages = segment.meta.size.div_ceil(PAGE_BYTES) as u64;
        for page in 0..pages {
            let offset = segment.meta.virt_addr + page * PAGE_SIZE;
            setup.map_privileged_page(
                VirtAddr::new(PRIVILEGED_BASE + offset),
                PhysAddr::new(phys.as_u64() + offset),
                segment.meta.permissions,
                ("⚡ Interrupt controller mapping", Alloc::Persistent),
            )?;
        }
    }

    // MMIO after the image, each region page-granular behind a guard page.
    let mut next_virt = PRIVILEGED_BASE + size as u64 + PAGE_SIZE;
    let mut config = IrqChipConfig {
        controller_count: u32::try_from(plan.controller_count).expect("at most MAX_CONTROLLERS"),
        ..IrqChipConfig::default()
    };
    for (planned, controller) in plan.controllers[..plan.controller_count]
        .iter()
        .zip(config.controllers.iter_mut())
    {
        *controller = Controller {
            kind: planned.kind,
            region_count: u32::try_from(planned.region_count).expect("at most MAX_REGIONS"),
            ..Controller::default()
        };
        for (&(start, region_size), mapped) in planned.regions[..planned.region_count]
            .iter()
            .zip(controller.regions.iter_mut())
        {
            let page_start = start & !(PAGE_SIZE - 1);
            let span = (start - page_start + region_size).next_multiple_of(PAGE_SIZE);
            paging::create_device_mapping(
                setup,
                PhysAddr::new(page_start),
                VirtAddr::new(next_virt),
                usize::try_from(span).map_err(|_overflow| "MMIO region too large")?,
            )?;
            *mapped = MmioRegion {
                virt: next_virt + (start - page_start),
                size: region_size,
            };
            next_virt += span + PAGE_SIZE;
        }
    }

    semi::println!(
        "🥾 Loaded {} at {:#x}: {} KiB, ops at {:#x}",
        image.name,
        PRIVILEGED_BASE,
        size / 1024,
        PRIVILEGED_BASE + image.ops_offset
    );
    record_kernel_owned(plan);
    record_controller_order(plan);
    Ok(PlatformHandoff {
        ops: PRIVILEGED_BASE + image.ops_offset,
        config,
        physical_timer: plan.physical_timer,
    })
}

/// The loaded component's handoff, read by `bootstrap` at EL1.
struct HandoffCell(UnsafeCell<Option<PlatformHandoff>>);

// SAFETY: written once at EL2 before the boot Thread runs, read-only afterwards,
// on the single boot core.
unsafe impl Sync for HandoffCell {}

static HANDOFF: HandoffCell = HandoffCell(UnsafeCell::new(None));

/// Record the handoff for `bootstrap`.
pub fn record(handoff: &PlatformHandoff) {
    // SAFETY: see `HandoffCell`: the only write, before any reader.
    unsafe {
        *HANDOFF.0.get() = Some(*handoff);
    }
}

/// The recorded handoff, if a component was loaded.
pub fn handoff() -> Option<&'static PlatformHandoff> {
    // SAFETY: see `HandoffCell`: no writer remains.
    unsafe { (*HANDOFF.0.get()).as_ref() }
}

/// Most physical MMIO ranges the kernel drives through privileged components.
const MAX_KERNEL_OWNED: usize = MAX_CONTROLLERS * MAX_REGIONS + MAX_WITHHELD;

/// Most extra ranges withheld by compatible string (see [`WITHHELD`]).
const MAX_WITHHELD: usize = 4;

/// Interrupt controllers never handed out as device memory even when no
/// loaded component drives them: the ARM local interrupt block also holds
/// the core timer controls.
pub const WITHHELD: &[&str] = &["brcm,bcm2836-l1-intc"];

/// The physical MMIO ranges of the loaded component's controllers: hardware
/// the kernel drives itself, never handed out as device memory.
struct KernelOwnedCell(UnsafeCell<([(u64, u64); MAX_KERNEL_OWNED], usize)>);

// SAFETY: written once at EL2 before the boot Thread runs, read-only afterwards,
// on the single boot core.
unsafe impl Sync for KernelOwnedCell {}

static KERNEL_OWNED: KernelOwnedCell =
    KernelOwnedCell(UnsafeCell::new(([(0, 0); MAX_KERNEL_OWNED], 0)));

/// Record the physical MMIO ranges `plan` gives the component.
fn record_kernel_owned(plan: &IrqChipPlan) {
    // SAFETY: see `KernelOwnedCell`: the only write, before any reader.
    let (ranges, count) = unsafe { &mut *KERNEL_OWNED.0.get() };
    for planned in &plan.controllers[..plan.controller_count] {
        for &range in &planned.regions[..planned.region_count] {
            ranges[*count] = range;
            *count += 1;
        }
    }
}

/// `(physical start, size)` of every MMIO range the kernel drives, empty if
/// no component was loaded.
pub fn kernel_owned_ranges() -> &'static [(u64, u64)] {
    // SAFETY: see `KernelOwnedCell`: no writer remains.
    let (ranges, count) = unsafe { &*KERNEL_OWNED.0.get() };
    &ranges[..*count]
}

/// Withhold `[start, start + size)` from device memory: a node matching
/// [`WITHHELD`]. Called while kickstart walks the device tree at EL2.
pub fn withhold(start: u64, size: u64) {
    // SAFETY: see `KernelOwnedCell`: written only at EL2, before any reader.
    let (ranges, count) = unsafe { &mut *KERNEL_OWNED.0.get() };
    assert!(*count < MAX_KERNEL_OWNED, "too many withheld device ranges");
    ranges[*count] = (start, size);
    *count += 1;
}

/// Record in `BOOT_INFO` the phandles of `plan`'s controllers, in the order
/// `IRQControl.Issue` numbers them.
fn record_controller_order(plan: &IrqChipPlan) {
    let mut controllers = InterruptControllers {
        count: u32::try_from(plan.controller_count).expect("at most MAX_CONTROLLERS"),
        ..InterruptControllers::default()
    };
    for (recorded, planned) in controllers
        .phandles
        .iter_mut()
        .zip(&plan.controllers[..plan.controller_count])
    {
        *recorded = planned.phandle;
    }
    BOOT_INFO.lock(|boot_info| boot_info.interrupt_controllers = controllers);
}
