/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 *
 * Based on ideas from Jorge Aparicio, Andre Richter, Phil Oppenheimer, Sergio Benitez.
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Low-level boot of the Raspberry's processor
//! http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf

use {
    crate::endless_sleep,
    cortex_a::{asm, regs::*},
};

//use crate::arch::caps::{CapNode, Capability};

// Stack placed before first executable instruction
const STACK_START: u64 = 0x0008_0000; // Keep in sync with linker script

/// Type check the user-supplied entry function.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        /// # Safety
        /// Only type-checks!
        #[export_name = "main"]
        pub unsafe fn __main() -> ! {
            // type check the given path
            let f: fn() -> ! = $path;

            f()
        }
    };
}

/// Reset function.
///
/// Initializes the bss section before calling into the user's `main()`.
///
/// # Safety
///
/// Totally unsafe! We're in the hardware land.
#[link_section = ".text.boot"]
unsafe fn reset() -> ! {
    extern "C" {
        // Boundaries of the .bss section, provided by the linker script
        static mut __BSS_START: u64;
        static mut __BSS_END: u64;
    }

    // Zeroes the .bss section
    r0::zero_bss(&mut __BSS_START, &mut __BSS_END);

    extern "Rust" {
        fn main() -> !;
    }

    main()
}

// [ARMv6 unaligned data access restrictions](https://developer.arm.com/documentation/ddi0333/h/unaligned-and-mixed-endian-data-access-support/unaligned-access-support/armv6-unaligned-data-access-restrictions?lang=en)
// dictates that compatibility bit U in CP15 must be set to 1 to allow Unaligned accesses while MMU is off.
// (In addition to SCTLR_EL1.A being 0)
// See also [CP15 C1 docs](https://developer.arm.com/documentation/ddi0290/g/system-control-coprocessor/system-control-processor-registers/c1--control-register).
// #[link_section = ".text.boot"]
// #[inline]
// fn enable_armv6_unaligned_access() {
//     unsafe {
//         asm!(
//             "mrc p15, 0, {u}, c1, c0, 0",
//             "or {u}, {u}, {CR_U}",
//             "mcr p15, 0, {u}, c1, c0, 0",
//             u = out(reg) _,
//             CR_U = const 1 << 22
//         );
//     }
// }

#[link_section = ".text.boot"]
#[inline]
fn shared_setup_and_enter_pre() {
    // Enable timer counter registers for EL1
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No virtual offset for reading the counters
    CNTVOFF_EL2.set(0);

    // Set System Control Register (EL1)
    // Make memory non-cacheable and disable MMU mapping.
    // Disable alignment checks, because Rust fmt module uses a little optimization
    // that happily reads and writes half-words (ldrh/strh) from/to unaligned addresses.
    SCTLR_EL1.write(
        SCTLR_EL1::I::NonCacheable
            + SCTLR_EL1::C::NonCacheable
            + SCTLR_EL1::M::Disable
            + SCTLR_EL1::A::Disable
            + SCTLR_EL1::SA::Disable
            + SCTLR_EL1::SA0::Disable,
    );

    // enable_armv6_unaligned_access();

    // Set Hypervisor Configuration Register (EL2)
    // Set EL1 execution state to AArch64
    // @todo Explain the SWIO bit (SWIO hardwired on Pi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);
}

#[link_section = ".text.boot"]
#[inline]
fn shared_setup_and_enter_post() -> ! {
    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    SP_EL1.set(STACK_START);

    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.
    asm::eret()
}

/// Real hardware boot-up sequence.
///
/// Prepare and execute transition from EL2 to EL1.
#[link_section = ".text.boot"]
#[inline]
fn setup_and_enter_el1_from_el2() -> ! {
    // Set Saved Program Status Register (EL2)
    // Set up a simulated exception return.
    //
    // Fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1h, // Use SP_EL1
    );

    // Make the Exception Link Register (EL2) point to reset().
    ELR_EL2.set(reset as *const () as u64);

    shared_setup_and_enter_post()
}

/// QEMU boot-up sequence.
///
/// Processors enter EL3 after reset.
/// ref: http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf
/// section: 5.5.1
/// However, GPU init code must be switching it down to EL2.
/// QEMU can't emulate Raspberry Pi properly (no VC boot code), so it starts in EL3.
///
/// Prepare and execute transition from EL3 to EL1.
/// (from https://github.com/s-matyukevich/raspberry-pi-os/blob/master/docs/lesson02/rpi-os.md)
#[cfg(qemu)]
#[link_section = ".text.boot"]
#[inline]
fn setup_and_enter_el1_from_el3() -> ! {
    // Set Secure Configuration Register (EL3)
    SCR_EL3.write(SCR_EL3::RW::NextELIsAarch64 + SCR_EL3::NS::NonSecure);

    // Set Saved Program Status Register (EL3)
    // Set up a simulated exception return.
    //
    // Fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    SPSR_EL3.write(
        SPSR_EL3::D::Masked
            + SPSR_EL3::A::Masked
            + SPSR_EL3::I::Masked
            + SPSR_EL3::F::Masked
            + SPSR_EL3::M::EL1h, // Use SP_EL1
    );

    // Make the Exception Link Register (EL3) point to reset().
    ELR_EL3.set(reset as *const () as u64);

    shared_setup_and_enter_post()
}

/// Entrypoint of the processor.
///
/// Parks all cores except core0 and checks if we started in EL2/EL3. If
/// so, proceeds with setting up EL1.
///
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel boot function reset().
///
/// Dissection of various RPi core boot stubs is available
/// [here](https://leiradel.github.io/2019/01/20/Raspberry-Pi-Stubs.html).
///
#[no_mangle]
#[link_section = ".text.boot.entry"]
pub unsafe extern "C" fn _boot_cores() -> ! {
    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;
    // Can't match values with dots in match, so use intermediate consts.
    #[cfg(qemu)]
    const EL3: u64 = CurrentEL::EL::EL3.value;
    const EL2: u64 = CurrentEL::EL::EL2.value;
    const EL1: u64 = CurrentEL::EL::EL1.value;

    // Set stack pointer. Used in case we started in EL1.
    SP.set(STACK_START);

    shared_setup_and_enter_pre();

    if CORE_0 == MPIDR_EL1.get() & CORE_MASK {
        match CurrentEL.get() {
            #[cfg(qemu)]
            EL3 => setup_and_enter_el1_from_el3(),
            EL2 => setup_and_enter_el1_from_el2(),
            EL1 => reset(),
            _ => endless_sleep(),
        }
    }

    // if not core0 or not EL3/EL2/EL1, infinitely wait for events
    endless_sleep()
}

/*
// caps and mem regions init

enum KernelInitError {}

fn map_kernel_window() {}

fn init_cpu() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn init_plat() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn arch_init_freemem() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn create_domain_cap() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn init_irqs() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn create_bootinfo_cap() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn create_asid_pool_for_initial_thread() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn create_idle_thread() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn clean_invalidate_l1_caches() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn create_initial_thread() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn init_core_state(_: Result<(), KernelInitError>) -> Result<(), KernelInitError> {
    unimplemented!();
}

fn create_untypeds() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn finalise_bootinfo() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn invalidate_local_tlb() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn lock_kernel_node() -> Result<(), KernelInitError> {
    unimplemented!();
}

fn schedule() {
    unimplemented!();
}

fn activate_thread() {
    unimplemented!();
}

#[link_section = ".text.boot"]
// #[used]
fn try_init_kernel() -> Result<(), KernelInitError> {
    map_kernel_window();
    init_cpu()?;
    init_plat()?;
    arch_init_freemem()?;

    let root_capnode_cap = create_root_capnode();
    create_domain_cap(root_capnode_cap);
    init_irqs(root_capnode_cap);

    //fill in boot info and
    create_bootinfo_cap();

    let it_asid_pool_cap = create_asid_pool_for_initial_thread(root_capnode_cap);
    create_idle_thread();

    /* Before creating the initial thread (which also switches to it)
     * we clean the cache so that any page table information written
     * as a result of calling create_frames_of_region will be correctly
     * read by the hardware page table walker */
    clean_invalidate_l1_caches();

    let it = create_initial_thread(root_capnode_cap);

    init_core_state(it);

    create_untypeds(root_capnode_cap);

    finalise_bootinfo();

    clean_invalidate_l1_caches();
    invalidate_local_tlb();

    // grab kernel lock before returning
    lock_kernel_node();

    Ok(())
}

fn try_init_kernel_secondary_core() -> Result<(), KernelInitError>
{
    init_cpu();

    /* Enable per-CPU timer interrupts */
    maskInterrupt(false, KERNEL_TIMER_IRQ);

    lock_kernel_node;

    ksNumCPUs++; // increase global cpu counter - this should be done differently?

    init_core_state(SchedulerAction_ResumeCurrentThread);

    Ok(())
}

fn init_kernel() {
    try_init_kernel()?;
    // or for AP:
    //    try_init_kernel_secondary_core();
    schedule();
    activate_thread();
}

const CONFIG_ROOT_CAPNODE_SIZE_BITS: usize = 12;
const wordBits: usize = 64;

fn create_root_capnode() -> Capability // Attr(BOOT_CODE)
{
    // write the number of root CNode slots to global state
    boot_info.max_slot_pos = 1 << CONFIG_ROOT_CAPNODE_SIZE_BITS; // 12 bits => 4096 slots

    // seL4_SlotBits = 32 bytes per entry, 4096 entries =>
    // create an empty root CapNode
    // this goes into the kernel startup/heap memory (one of the few items that kernel DOES allocate).
    let region_size = core::mem::size_of::<Capability> * boot_info.max_slot_pos; // 12 + 5 => 131072 (128Kb)
    let pptr = alloc_region(region_size); // GlobalAllocator::alloc_zeroed instead?
    if pptr.is_none() {
        println!("Kernel init failing: could not create root capnode");
        return Capability(NullCap::Type::value);
    }
    let Some(pptr) = pptr;
    memzero(pptr, region_size); // CTE_PTR(pptr) ?

    // transmute into a type? (you can use ptr.write() to just write a type into memory location)

    let cap = CapNode::new_root(pptr);

    // this cnode contains a cap to itself...
    /* write the root CNode cap into the root CNode */
    // @todo rootCapNode.write_slot(CapInitThreadCNode, cap); -- where cap and rootCapNode are synonyms!
    write_slot(SLOT_PTR(pptr, seL4_CapInitThreadCNode), cap);

    cap // reference to pptr is here
}
*/
