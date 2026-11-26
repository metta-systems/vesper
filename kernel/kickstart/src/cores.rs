//! The per-core records and trap stacks (SMP design, "Per-core record", C3(a)).
//!
//! One record and one `SP_EL1` trap stack per core, sized from the device tree's `/cpus`
//! and carved from accounted boot memory. The trap stacks are mapped in the kernel half in
//! slots from the nucleus's `__STACK_VIRT_BOTTOM`, by logical index, each behind a guard
//! page; the records are reached through the nucleus's linear map of RAM. Each core's
//! `TPIDR_EL1` names its own record.

use {
    crate::memory::{Alloc, BootAllocator},
    libaddress::{PHYSICAL_KERNEL_WINDOW, PhysAddr},
    libcpu::smp::CoreAffinity,
    libcpu_start::MAX_CPUS,
    libdevicetree::{DeviceTree, cpus::cpus},
    libimage::PAGE_BYTES,
    libqemu::semihosting as semi,
    nucleus::core_record::CoreRecord,
};

/// Pages of one core's trap stack: 512 KiB.
pub const TRAP_STACK_PAGES: usize = 128;
/// Bytes of one core's trap stack.
pub const TRAP_STACK_BYTES: usize = TRAP_STACK_PAGES * PAGE_BYTES;

/// The cores the device tree lists, decided while its index is live.
#[derive(Debug, Clone, Copy)]
pub struct CorePlan {
    count: usize,
    boot_index: usize,
    affinities: [u64; MAX_CPUS],
}

impl CorePlan {
    /// The boot core's logical index.
    pub fn boot_index(&self) -> usize {
        self.boot_index
    }
}

/// The cores of `/cpus`, at most [`MAX_CPUS`], by logical index. A tree that lists no
/// CPU, or not the boot core among the first [`MAX_CPUS`], gets one core: the boot core.
pub fn plan(device_tree: &DeviceTree<'_, '_>) -> CorePlan {
    let boot_affinity = CoreAffinity::current().bits();
    let mut plan = CorePlan {
        count: 0,
        boot_index: 0,
        affinities: [0; MAX_CPUS],
    };
    for cpu in cpus(device_tree.index()).take(MAX_CPUS) {
        plan.affinities[cpu.index] = cpu.affinity;
        plan.count = cpu.index + 1;
    }
    match plan.affinities[..plan.count]
        .iter()
        .position(|&affinity| affinity == boot_affinity)
    {
        Some(boot_index) => plan.boot_index = boot_index,
        None => {
            semi::println!("🥾 /cpus does not list the boot core: running it alone");
            plan.count = 1;
            plan.affinities[0] = boot_affinity;
        }
    }
    semi::println!(
        "🥾 {} cores, the boot core is {} (affinity {:#x})",
        plan.count,
        plan.boot_index,
        boot_affinity
    );
    plan
}

/// The carved records and trap stacks.
#[derive(Debug, Clone, Copy)]
pub struct Cores {
    plan: CorePlan,
    records: PhysAddr,
    stacks: [u64; MAX_CPUS],
}

impl Cores {
    /// The trap stacks' physical bases, by logical index.
    pub fn stacks(&self) -> &[u64] {
        &self.stacks[..self.plan.count]
    }

    /// The boot core's logical index.
    pub fn boot_index(&self) -> usize {
        self.plan.boot_index
    }

    /// The planned cores' affinities, by logical index.
    pub fn affinities(&self) -> &[u64] {
        &self.plan.affinities[..self.plan.count]
    }

    /// Physical top of core `index`'s trap stack, which its secondary entry runs on at
    /// EL2 before the stack is used at its kernel virtual address.
    pub fn stack_physical_top(&self, index: usize) -> u64 {
        self.stacks[index] + TRAP_STACK_BYTES as u64
    }

    /// Kernel virtual address of core `index`'s record.
    pub fn record_virt(&self, index: usize) -> u64 {
        PHYSICAL_KERNEL_WINDOW + self.records.as_u64() + (index * size_of::<CoreRecord>()) as u64
    }

    /// Write every core's record, with `trap_stack_top(index)` the kernel virtual top of
    /// its trap stack. Kickstart runs with its MMU off: the records are written at their
    /// physical addresses.
    pub fn write_records(&self, trap_stack_top: impl Fn(usize) -> u64) {
        let base = self.records.as_u64() as *mut CoreRecord;
        for (index, &affinity) in self.plan.affinities[..self.plan.count].iter().enumerate() {
            let record = CoreRecord::new(
                u32::try_from(index).expect("at most MAX_CPUS cores"),
                affinity,
                trap_stack_top(index),
            );
            // SAFETY: the record run was allocated for `count` records and is ours; the
            // MMU is off, so its physical address is its address.
            unsafe {
                base.add(index).write(record);
            }
        }
    }
}

/// Carve a trap stack per planned core and the run of records.
///
/// # Errors
///
/// If boot memory runs out.
pub fn allocate(plan: CorePlan, allocator: &mut BootAllocator) -> Result<Cores, &'static str> {
    let mut stacks = [0; MAX_CPUS];
    for stack in &mut stacks[..plan.count] {
        *stack = allocator
            .alloc_pages(
                TRAP_STACK_PAGES,
                ("🧠 Nucleus trap stack", Alloc::Persistent),
            )
            .ok_or("cannot allocate a trap stack")?
            .as_u64();
    }
    let record_pages = (plan.count * size_of::<CoreRecord>()).div_ceil(PAGE_BYTES);
    let records = allocator
        .alloc_pages(record_pages, ("🧠 Per-core records", Alloc::Persistent))
        .ok_or("cannot allocate the per-core records")?;
    Ok(Cores {
        plan,
        records,
        stacks,
    })
}
