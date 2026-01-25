#![no_std]
#![no_main]
#![allow(unused)]
#![feature(format_args_nl)]

mod boot;
mod el_switch;
mod embed;
mod loader;
mod memory;
mod paging;
mod syscall_test;

use {
    core::panic::PanicInfo,
    libcpu::endless_sleep,
    libqemu::semi_println,
    memory::{BootAllocator, PhysAddr},
    syscall_test::protected_call6,
};

unsafe extern "C" {
    static __init_start: u8;
    static __init_end: u8;
    static __free_memory_start: u8;
}

#[unsafe(no_mangle)]
pub extern "C" fn init_main(_dtb_ptr: *const u8) -> ! {
    semi_println!("init_main started");

    let init_start = unsafe { &__init_start as *const u8 as u64 };
    let init_end = unsafe { &__init_end as *const u8 as u64 };
    let free_start = unsafe { &__free_memory_start as *const u8 as u64 };

    let memory_size = 256 * 1024 * 1024;
    let mut allocator = BootAllocator::new(PhysAddr::new(free_start), memory_size);
    let memory_end = allocator.end();
    semi_println!(
        "init_main: Created BootAllocator {memory_size} @ {:#016X}",
        free_start
    );

    // ═══════════════════════════════════════════════════════════════
    // PHASE 1: Load kernel
    // ═══════════════════════════════════════════════════════════════

    let kernel_layout = loader::load_kernel(&mut allocator).expect("Failed to load nucleus");
    semi_println!("init_main: Loaded nucleus image");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 2: Set up page tables
    // ═══════════════════════════════════════════════════════════════

    let mut mmu_setup = paging::MmuSetup::new(&mut allocator).expect("Failed to create MMU setup");
    semi_println!("init_main: Created MmuSetup");

    // Identity map init_thread
    paging::create_identity_mapping(&mut mmu_setup, PhysAddr::new(init_start), memory_end)
        .expect("Failed to create identity mapping");
    semi_println!("init_main: Identity mapped the Init_Thread");

    // Create kernel mapping with per-section permissions
    paging::create_kernel_mapping(&mut mmu_setup, &kernel_layout)
        .expect("Failed to create kernel mapping");
    semi_println!("init_main: Higher-half mapped the nucleus");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 3: Prepare for EL1
    // ═══════════════════════════════════════════════════════════════

    let ttbr0 = mmu_setup.ttbr0();
    let ttbr1 = mmu_setup.ttbr1();
    semi_println!("init_main: TTBR0_EL1 at {ttbr0:#016X}, TTBR1_EL1 at {ttbr1:#016X}");

    // Get vector table virtual address for VBAR_EL1
    // VBAR is only used after MMU is enabled, so we set the virtual address directly
    let vbar = kernel_layout.vbar_el1_virt();

    // Allocate EL1 stack
    let el1_stack = allocator
        .alloc_pages(16)
        .expect("Failed to allocate EL1 stack");
    let el1_stack_top = el1_stack.as_u64() + 16 * 4096;
    // FIXME: stack must be identity-mapped!
    semi_println!("init_main: EL1 stack at {el1_stack_top:#016X}, vbar {vbar:#016X}");

    // ═══════════════════════════════════════════════════════════════
    // PHASE 4: Enable MMU and drop to EL1
    // ═══════════════════════════════════════════════════════════════

    unsafe {
        el_switch::enable_mmu_and_drop_to_el1(
            ttbr0,
            ttbr1,
            vbar,
            init_thread_run as *const () as u64,
            el1_stack_top,
        );
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    semi_println!("PANICKED: {info}");
    endless_sleep()
}

#[unsafe(no_mangle)]
pub extern "C" fn init_thread_run(_dtb_ptr: *const u8) -> ! {
    // Run initial thread further in EL1, seting up the capDL etc.
    semi_println!("init_main_run dropped to EL1");
    unsafe {
        protected_call6(0, 0, 0, 0, 0, 0, 0, 0);
    }
    endless_sleep()
}
