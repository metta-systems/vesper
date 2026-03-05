#![no_std]
#![no_main]
#![feature(allocator_api)]
#![feature(step_trait)]
#![feature(format_args_nl)]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(unused_imports)] // commented-out tests

use {
    core::{
        alloc::{Allocator, Layout},
        cell::UnsafeCell,
        iter::Step,
        num::NonZeroUsize,
        ops::Range,
    },
    libaddress::{align_up, Address, PhysAddr, PhysAddrNotValid, Physical, Virtual},
    liballoc::BumpAllocator,
    liblocking::interface::Mutex,
    liblog::println,
    libmemory::{
        arch::mmu::translation_table::{PageDescriptor, TableDescriptor},
        mmu::{
            kernel_map_at, page_alloc,
            translation_table::{interface::TranslationTable, FixedSizeTranslationTable},
            AccessPermissions, AttributeFields, MemAttributes, MemoryRegion, PageAddress,
        },
        platform::memory::mmu::{
            virt_boot_core_stack_region, virt_code_region, virt_data_region, KERNEL_TABLES,
        },
    },
    libplatform::KernelGranule,
};

// mod common;

// pub type MinSizeTranslationTable = FixedSizeTranslationTable<1>;

// /// Check if the size of `struct TableDescriptor` is as expected.
// #[test_case]
// fn size_of_tabledescriptor_equals_64_bit() {
//     assert_eq!(
//         core::mem::size_of::<TableDescriptor>(),
//         core::mem::size_of::<u64>()
//     );
// }

// /// Check if the size of `struct PageDescriptor` is as expected.
// #[test_case]
// fn size_of_pagedescriptor_equals_64_bit() {
//     assert_eq!(
//         core::mem::size_of::<PageDescriptor>(),
//         core::mem::size_of::<u64>()
//     );
// }

// /// Check that you cannot map into the MMIO VA range from kernel_map_at().
// /*#[test_case]
// fn no_manual_mmio_map() {
//     let allocator_region =
//         MemoryRegion::new(PageAddress::from(0xab0000), PageAddress::from(0xab00000));
//     page_alloc::kernel_mmio_va_allocator().lock(|allocator| allocator.init(allocator_region));

//     let phys_start_page_addr: PageAddress<Physical> = PageAddress::from(0);
//     let phys_end_exclusive_page_addr: PageAddress<Physical> =
//         phys_start_page_addr.checked_page_offset(5).unwrap();
//     let phys_region = MemoryRegion::new(phys_start_page_addr, phys_end_exclusive_page_addr);

//     let num_pages = NonZeroUsize::new(phys_region.num_pages()).unwrap();
//     let virt_region = page_alloc::kernel_mmio_va_allocator()
//         .lock(|allocator| allocator.alloc(num_pages))
//         .unwrap();

//     let attr = AttributeFields {
//         mem_attributes: MemAttributes::CacheableDRAM,
//         acc_perms: AccessPermissions::ReadWrite,
//         execute_never: true,
//     };

//     unsafe {
//         assert_eq!(
//             kernel_map_at("test", &virt_region, &phys_region, &attr),
//             Err("Attempt to manually map into MMIO region")
//         )
//     };
// }*/
// /// Sanity checks for the TranslationTable implementation.
// #[test_case]
// fn translation_table_implementation_sanity() {
//     // This will occupy a lot of space on the stack.
//     let mut tables = MinSizeTranslationTable::new();

//     tables.init().unwrap();

//     let virt_start_page_addr: PageAddress<Virtual> = PageAddress::from(0);
//     let virt_end_exclusive_page_addr: PageAddress<Virtual> =
//         virt_start_page_addr.checked_page_offset(5).unwrap();

//     let phys_start_page_addr: PageAddress<Physical> = PageAddress::from(0);
//     let phys_end_exclusive_page_addr: PageAddress<Physical> =
//         phys_start_page_addr.checked_page_offset(5).unwrap();

//     let virt_region = MemoryRegion::new(virt_start_page_addr, virt_end_exclusive_page_addr);
//     let phys_region = MemoryRegion::new(phys_start_page_addr, phys_end_exclusive_page_addr);

//     let attr = AttributeFields {
//         mem_attributes: MemAttributes::CacheableDRAM,
//         acc_perms: AccessPermissions::ReadWrite,
//         execute_never: true,
//     };

//     unsafe { assert_eq!(tables.map_at(&virt_region, &phys_region, attr), Ok(())) };
// }

// /// Check alignment of the kernel's virtual memory layout sections.
// #[test_case]
// fn virt_mem_layout_sections_are_64kib_aligned() {
//     for i in [
//         virt_boot_core_stack_region,
//         virt_code_region,
//         virt_data_region,
//     ]
//     .iter()
//     {
//         let start = i().start_page_addr().into_inner();
//         let end_exclusive = i().end_exclusive_page_addr().into_inner();

//         assert!(start.is_page_aligned());
//         assert!(end_exclusive.is_page_aligned());
//         assert!(end_exclusive >= start);
//     }
// }

// /// Ensure the kernel's virtual memory layout is free of overlaps.
// #[test_case]
// fn virt_mem_layout_has_no_overlaps() {
//     let layout = [
//         virt_boot_core_stack_region(),
//         virt_code_region(),
//         virt_data_region(),
//     ];

//     for (i, first_range) in layout.iter().enumerate() {
//         for second_range in layout.iter().skip(i + 1) {
//             assert!(!first_range.overlaps(second_range))
//         }
//     }
// }
