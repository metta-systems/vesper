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
