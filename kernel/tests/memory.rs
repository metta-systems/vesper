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
    liblocking::interface::Mutex,
    liblog::println,
    libmemory::{
        arch::mmu::translation_table::{PageDescriptor, TableDescriptor},
        mm::{align_up, BumpAllocator},
        mmu::{
            kernel_map_at, page_alloc,
            translation_table::{interface::TranslationTable, FixedSizeTranslationTable},
            AccessPermissions, AttributeFields, MemAttributes, MemoryRegion, PageAddress,
        },
        phys_addr::{PhysAddr, PhysAddrNotValid},
        platform::memory::mmu::{
            virt_boot_core_stack_region, virt_code_region, virt_data_region, KERNEL_TABLES,
        },
        platform::KernelGranule, //memory::mmu::KernelGranule},
        Address,
        Physical,
        Virtual,
    },
};

mod common;

pub type MinSizeTranslationTable = FixedSizeTranslationTable<1>;

/// Check if the size of `struct TableDescriptor` is as expected.
#[test_case]
fn size_of_tabledescriptor_equals_64_bit() {
    assert_eq!(
        core::mem::size_of::<TableDescriptor>(),
        core::mem::size_of::<u64>()
    );
}

/// Check if the size of `struct PageDescriptor` is as expected.
#[test_case]
fn size_of_pagedescriptor_equals_64_bit() {
    assert_eq!(
        core::mem::size_of::<PageDescriptor>(),
        core::mem::size_of::<u64>()
    );
}

/// Sanity of [Address] methods.
#[test_case]
fn address_type_method_sanity() {
    let addr = Address::<Virtual>::new(KernelGranule::SIZE + 100);

    assert_eq!(addr.align_down_page().as_usize(), KernelGranule::SIZE);

    assert_eq!(addr.align_up_page().as_usize(), KernelGranule::SIZE * 2);

    assert!(!addr.is_page_aligned());

    assert_eq!(addr.offset_into_page(), 100);
}

// Validate allocator allocates from the provided address range
// Validate allocation fails when range is exhausted
#[test_case]
fn test_allocates_within_init_range() {
    let allocator = BumpAllocator::new(256, 512, "Test allocator 1");
    let result1 = allocator.allocate(unsafe { Layout::from_size_align_unchecked(128, 1) });
    assert!(result1.is_ok());
    let result2 = allocator.allocate(unsafe { Layout::from_size_align_unchecked(128, 32) });
    println!("{:?}", result2);
    assert!(result2.is_ok());
    let result3 = allocator.allocate(unsafe { Layout::from_size_align_unchecked(1, 1) });
    assert!(result3.is_err());
}

// Creating with end <= start sshould fail
// @todo return Result<> from new?
#[test_case]
fn test_bad_allocator() {
    let bad_allocator = BumpAllocator::new(512, 256, "Test allocator 2");
    let result1 = bad_allocator.allocate(unsafe { Layout::from_size_align_unchecked(1, 1) });
    assert!(result1.is_err());
}

#[test_case]
pub fn test_align_up() {
    // align 1
    assert_eq!(align_up(0, 1), 0);
    assert_eq!(align_up(1234, 1), 1234);
    assert_eq!(align_up(0xffff_ffff_ffff_ffff, 1), 0xffff_ffff_ffff_ffff);
    // align 2
    assert_eq!(align_up(0, 2), 0);
    assert_eq!(align_up(1233, 2), 1234);
    assert_eq!(align_up(0xffff_ffff_ffff_fffe, 2), 0xffff_ffff_ffff_fffe);
    // address 0
    assert_eq!(align_up(0, 128), 0);
    assert_eq!(align_up(0, 1), 0);
    assert_eq!(align_up(0, 2), 0);
    assert_eq!(align_up(0, 0x8000_0000_0000_0000), 0);
}

//==============================================================================
//==============================================================================
//==============================================================================

/// Check that you cannot map into the MMIO VA range from kernel_map_at().
/*#[test_case]
fn no_manual_mmio_map() {
    let allocator_region =
        MemoryRegion::new(PageAddress::from(0xab0000), PageAddress::from(0xab00000));
    page_alloc::kernel_mmio_va_allocator().lock(|allocator| allocator.init(allocator_region));

    let phys_start_page_addr: PageAddress<Physical> = PageAddress::from(0);
    let phys_end_exclusive_page_addr: PageAddress<Physical> =
        phys_start_page_addr.checked_offset(5).unwrap();
    let phys_region = MemoryRegion::new(phys_start_page_addr, phys_end_exclusive_page_addr);

    let num_pages = NonZeroUsize::new(phys_region.num_pages()).unwrap();
    let virt_region = page_alloc::kernel_mmio_va_allocator()
        .lock(|allocator| allocator.alloc(num_pages))
        .unwrap();

    let attr = AttributeFields {
        mem_attributes: MemAttributes::CacheableDRAM,
        acc_perms: AccessPermissions::ReadWrite,
        execute_never: true,
    };

    unsafe {
        assert_eq!(
            kernel_map_at("test", &virt_region, &phys_region, &attr),
            Err("Attempt to manually map into MMIO region")
        )
    };
}*/
//==============================================================================
//==============================================================================
//==============================================================================

/// Sanity checks for the TranslationTable implementation.
#[test_case]
fn translation_table_implementation_sanity() {
    // This will occupy a lot of space on the stack.
    let mut tables = MinSizeTranslationTable::new();

    tables.init().unwrap();

    let virt_start_page_addr: PageAddress<Virtual> = PageAddress::from(0);
    let virt_end_exclusive_page_addr: PageAddress<Virtual> =
        virt_start_page_addr.checked_offset(5).unwrap();

    let phys_start_page_addr: PageAddress<Physical> = PageAddress::from(0);
    let phys_end_exclusive_page_addr: PageAddress<Physical> =
        phys_start_page_addr.checked_offset(5).unwrap();

    let virt_region = MemoryRegion::new(virt_start_page_addr, virt_end_exclusive_page_addr);
    let phys_region = MemoryRegion::new(phys_start_page_addr, phys_end_exclusive_page_addr);

    let attr = AttributeFields {
        mem_attributes: MemAttributes::CacheableDRAM,
        acc_perms: AccessPermissions::ReadWrite,
        execute_never: true,
    };

    unsafe { assert_eq!(tables.map_at(&virt_region, &phys_region, attr), Ok(())) };
}

/// Sanity of [PageAddress] methods.
#[test_case]
fn pageaddress_type_method_sanity() {
    let page_addr: PageAddress<Virtual> = PageAddress::from(KernelGranule::SIZE * 2);

    assert_eq!(
        page_addr.checked_offset(-2),
        Some(PageAddress::<Virtual>::from(0))
    );

    assert_eq!(
        page_addr.checked_offset(2),
        Some(PageAddress::<Virtual>::from(KernelGranule::SIZE * 4))
    );

    assert_eq!(
        PageAddress::<Virtual>::from(0).checked_offset(0),
        Some(PageAddress::<Virtual>::from(0))
    );
    assert_eq!(PageAddress::<Virtual>::from(0).checked_offset(-1), None);

    let max_page_addr = Address::<Virtual>::new(usize::MAX).align_down_page();
    assert_eq!(
        PageAddress::<Virtual>::from(max_page_addr).checked_offset(1),
        None
    );

    let zero = PageAddress::<Virtual>::from(0);
    let three = PageAddress::<Virtual>::from(KernelGranule::SIZE * 3);
    assert_eq!(PageAddress::steps_between(&zero, &three), (3, Some(3)));
}

/// Sanity of [MemoryRegion] methods.
#[test_case]
fn memoryregion_type_method_sanity() {
    let zero = PageAddress::<Virtual>::from(0);
    let zero_region = MemoryRegion::new(zero, zero);
    assert_eq!(zero_region.num_pages(), 0);
    assert_eq!(zero_region.size(), 0);

    let one = PageAddress::<Virtual>::from(KernelGranule::SIZE);
    let one_region = MemoryRegion::new(zero, one);
    assert_eq!(one_region.num_pages(), 1);
    assert_eq!(one_region.size(), KernelGranule::SIZE);

    let three = PageAddress::<Virtual>::from(KernelGranule::SIZE * 3);
    let mut three_region = MemoryRegion::new(zero, three);
    assert!(three_region.contains(zero.into_inner()));
    assert!(!three_region.contains(three.into_inner()));
    assert!(three_region.overlaps(&one_region));

    let allocation = three_region
        .take_first_n_pages(NonZeroUsize::new(2).unwrap())
        .unwrap();
    assert_eq!(allocation.num_pages(), 2);
    assert_eq!(three_region.num_pages(), 1);

    for (i, alloc) in allocation.into_iter().enumerate() {
        assert_eq!(alloc.into_inner().as_usize(), i * KernelGranule::SIZE);
    }
}

#[test_case]
pub fn test_invalid_phys_addr() {
    let result = PhysAddr::try_new(0xfafa_0123_3210_3210);
    if let Err(e) = result {
        assert_eq!(e, PhysAddrNotValid(0xfafa_0123_3210_3210));
    } else {
        assert!(false)
    }
}

/// Check alignment of the kernel's virtual memory layout sections.
#[test_case]
fn virt_mem_layout_sections_are_64kib_aligned() {
    for i in [
        virt_boot_core_stack_region,
        virt_code_region,
        virt_data_region,
    ]
    .iter()
    {
        let start = i().start_page_addr().into_inner();
        let end_exclusive = i().end_exclusive_page_addr().into_inner();

        assert!(start.is_page_aligned());
        assert!(end_exclusive.is_page_aligned());
        assert!(end_exclusive >= start);
    }
}

/// Ensure the kernel's virtual memory layout is free of overlaps.
#[test_case]
fn virt_mem_layout_has_no_overlaps() {
    let layout = [
        virt_boot_core_stack_region(),
        virt_code_region(),
        virt_data_region(),
    ];

    for (i, first_range) in layout.iter().enumerate() {
        for second_range in layout.iter().skip(i + 1) {
            assert!(!first_range.overlaps(second_range))
        }
    }
}
