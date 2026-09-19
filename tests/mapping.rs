/// Sanity of [PageAddress] methods.
#[test_case]
fn pageaddress_type_method_sanity() {
    let page_addr: PageAddress<Virtual> = PageAddress::from(KernelGranule::SIZE * 2);

    assert_eq!(
        page_addr.checked_page_offset(-2),
        Some(PageAddress::<Virtual>::from(0))
    );

    assert_eq!(
        page_addr.checked_page_offset(2),
        Some(PageAddress::<Virtual>::from(KernelGranule::SIZE * 4))
    );

    assert_eq!(
        PageAddress::<Virtual>::from(0).checked_page_offset(0),
        Some(PageAddress::<Virtual>::from(0))
    );
    assert_eq!(
        PageAddress::<Virtual>::from(0).checked_page_offset(-1),
        None
    );

    let max_page_addr = Address::<Virtual>::new(usize::MAX).align_down_page();
    assert_eq!(
        PageAddress::<Virtual>::from(max_page_addr).checked_page_offset(1),
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
