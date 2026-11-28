//! Select, load and initialise the start-method component, which starts the secondary
//! cores (SMP design: start-method components, SMP-2 and SMP-3).
//!
//! The device tree names the method in every `/cpus` node's `enable-method`. When all the
//! nodes agree, the bundled component (`CPU_START`) whose `enable-method` list holds that
//! value is the one loaded; the others are not. A tree whose nodes disagree, or name a
//! method no bundled component implements, gets no component, and the system runs on the
//! boot core alone.
//!
//! Kickstart runs at EL2 with its MMU and caches off, and calls the component there, so the
//! component is relocated to its own physical address. Its memory is Kickstart's and is
//! reclaimed with it; the nucleus never gets the table.

use {
    crate::memory::{Alloc, BootAllocator},
    libcpu_start::{ABI_VERSION, Conduit, CpuEntry, CpuStartConfig, CpuStartOps, StartStatus},
    libdevicetree::{
        DeviceTree,
        cpus::{PsciConduit, common_enable_method, cpus, psci},
    },
    libimage::{PAGE_BYTES, PrivilegedImage},
    libqemu::semihosting as semi,
};

/// What the device tree asks for, decided while its index is live.
pub struct CpuStartPlan {
    image: &'static PrivilegedImage,
    config: CpuStartConfig,
}

/// Pick the start-method component for the device tree's `/cpus`.
///
/// # Errors
///
/// If the `/cpus` nodes name no common `enable-method`, no bundled component implements
/// it, or the tree lists more CPUs than [`libcpu_start::MAX_CPUS`].
pub fn plan(
    device_tree: &DeviceTree<'_, '_>,
    images: &[&'static PrivilegedImage],
) -> Result<CpuStartPlan, &'static str> {
    let tree = device_tree.index();
    let wanted =
        common_enable_method(cpus(tree)).ok_or("the /cpus nodes name no common enable-method")?;
    let (image, method) = images
        .iter()
        .find_map(|image| {
            image
                .compatible
                .iter()
                .find(|method| **method == wanted)
                .map(|method| (*image, *method))
        })
        .ok_or("no bundled component implements the enable-method")?;

    let mut config = CpuStartConfig::default();
    for cpu in cpus(tree) {
        let entry = config
            .cpus
            .get_mut(cpu.index)
            .ok_or("more CPUs than the start-method ABI describes")?;
        *entry = CpuEntry {
            affinity: cpu.affinity,
            release_address: cpu.release_address.unwrap_or(0),
        };
        config.cpu_count += 1;
    }
    if let Some(found) = psci(tree) {
        config.conduit = match found.conduit {
            PsciConduit::Smc => Conduit::Smc,
            PsciConduit::Hvc => Conduit::Hvc,
        };
        config.psci_cpu_on = found.cpu_on;
    }

    semi::println!(
        "🥾 Start method: {method} ({}), {} CPUs",
        image.name,
        config.cpu_count
    );
    Ok(CpuStartPlan { image, config })
}

/// The loaded component: its table and the configuration it accepted.
pub struct LoadedCpuStart {
    ops: *const CpuStartOps,
    config: CpuStartConfig,
}

impl LoadedCpuStart {
    /// Release core `cpu` (its logical index) to start at the physical address `entry`,
    /// with `context` the physical address of its boot parameters.
    pub fn start(&self, cpu: u32, entry: u64, context: u64) -> StartStatus {
        let Some(found) = self.config.cpu(cpu) else {
            return StartStatus::NoSuchCpu;
        };
        // SAFETY: `ops` is the relocated table `load` checked; Kickstart calls it on the
        // boot core at EL2, as the ABI requires.
        let raw = unsafe { ((*self.ops).start)(cpu, found.affinity, entry, context) };
        StartStatus::from_raw(raw)
    }
}

/// Load the planned component at a physical address of its own and initialise it.
///
/// # Errors
///
/// If memory cannot be allocated, the image does not lay out, its table has another ABI
/// version, or the component refuses the configuration.
pub fn load(
    plan: &CpuStartPlan,
    allocator: &mut BootAllocator,
) -> Result<LoadedCpuStart, &'static str> {
    let image = plan.image;
    let size = image.total_size();
    let phys = allocator
        .alloc_pages(
            size / PAGE_BYTES,
            ("⚡ Start-method component", Alloc::Droppable),
        )
        .ok_or("cannot allocate the start-method component")?;
    // SAFETY: Kickstart runs with the MMU off at EL2; the freshly allocated pages are
    // identity-accessible and exclusively ours.
    let memory = unsafe { core::slice::from_raw_parts_mut(phys.as_mut_ptr::<u8>(), size) };
    image
        .load_into(memory, phys.as_u64())
        .map_err(|_load_error| "cannot lay out the start-method component")?;
    // The image is code that runs at its physical address: make the instruction fetch
    // see what was just written.
    // SAFETY: the range is the component's memory, reached as is with the MMU off.
    unsafe {
        libcpu::cache::unify_instruction_range(phys.as_u64(), phys.as_u64() + size as u64);
    }

    let ops = (phys.as_u64() + image.ops_offset) as *const CpuStartOps;
    // SAFETY: `ops_offset` names the component's table, relocated just above.
    let table = unsafe { &*ops };
    if table.abi_version != ABI_VERSION {
        return Err("the start-method component has another ABI version");
    }
    // SAFETY: the table is relocated and the configuration outlives the call.
    let status = StartStatus::from_raw(unsafe { (table.init)(&raw const plan.config) });
    if status != StartStatus::Success {
        semi::println!("🥾 {} refused its configuration: {status:?}", image.name);
        return Err("the start-method component refused its configuration");
    }
    semi::println!(
        "🥾 Loaded {} at {:#x}: {} KiB, ops at {:#x}",
        image.name,
        phys.as_u64(),
        size / 1024,
        ops as u64
    );
    Ok(LoadedCpuStart {
        ops,
        config: plan.config,
    })
}
