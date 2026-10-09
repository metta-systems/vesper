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
        memory::{Alloc, MemoryPermissions},
        paging::{self, MmuSetup},
    },
    core::cell::UnsafeCell,
    libaddress::{PhysAddr, VirtAddr},
    libdevicetree::{
        DeviceTree, Node, find_compatible, interrupt_cells, interrupt_parent, interrupt_specifier,
        is_compatible, is_enabled, is_interrupt_controller, phandle, regions,
    },
    libimage::{PAGE_BYTES, PAGE_SIZE, PrivilegedImage},
    libirqchip::{
        Controller, IrqChipConfig, MAX_CONTROLLERS, MAX_REGIONS, MAX_SPECIFIER_CELLS, MmioRegion,
        PlatformHandoff, Specifier,
    },
    libqemu::semihosting as semi,
};

/// Where the privileged component is linked into the kernel half: the
/// otherwise unused TTBR1 L0 slot 257.
pub const PRIVILEGED_BASE: u64 = 0xFFFF_8080_0000_0000;

/// Generic timer nodes (device tree `arm,armv7-timer`/`arm,armv8-timer` binding).
const TIMERS: &[&str] = &["arm,armv8-timer", "arm,armv7-timer"];

/// Index of the non-secure EL1 physical timer in a timer node's `interrupts`
/// (binding order: secure physical, non-secure physical, virtual, hypervisor).
const NONSECURE_PHYSICAL_TIMER: usize = 1;

/// A controller the component will drive: its kind and CPU physical MMIO.
#[derive(Clone, Copy, Default)]
struct PlannedController {
    kind: u32,
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
    let (timer, _) = find_compatible(tree, TIMERS).ok_or("no generic timer node")?;
    let root = interrupt_parent(tree, &timer).ok_or("the timer has no interrupt-parent")?;
    let (image, root_kind) = images
        .iter()
        .find_map(|image| kind_in(image, &root).map(|kind| (*image, kind)))
        .ok_or("no bundled component drives the root interrupt controller")?;

    let mut plan = IrqChipPlan {
        image,
        controller_count: 1,
        controllers: [PlannedController::default(); MAX_CONTROLLERS],
        physical_timer: Specifier::default(),
    };
    plan.controllers[0] = plan_controller(root_kind, &root);
    for node in tree.nodes() {
        if plan.controller_count == MAX_CONTROLLERS {
            break;
        }
        let is_root = phandle(&node).is_some() && phandle(&node) == phandle(&root);
        if is_root || !is_interrupt_controller(&node) || !is_enabled(&node) {
            continue;
        }
        if let Some(kind) = kind_in(image, &node) {
            plan.controllers[plan.controller_count] = plan_controller(kind, &node);
            plan.controller_count += 1;
        }
    }

    let cell_count = interrupt_cells(&root).ok_or("the root controller has no #interrupt-cells")?;
    let mut cells = [0_u32; MAX_SPECIFIER_CELLS];
    let specifier = interrupt_specifier(
        &timer,
        NONSECURE_PHYSICAL_TIMER,
        cell_count as usize,
        &mut cells,
    )
    .ok_or("cannot read the physical timer interrupt")?;
    plan.physical_timer = Specifier {
        kind: root_kind,
        cell_count,
        cells: [0; MAX_SPECIFIER_CELLS],
    };
    plan.physical_timer.cells[..specifier.len()].copy_from_slice(specifier);

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
