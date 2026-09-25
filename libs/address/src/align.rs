/// Align address downwards.
///
/// Returns the greatest x with alignment `align` so that x <= addr.
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn align_down_bits(addr: u64, alignment_bits: u64) -> u64 {
    addr & !((1 << alignment_bits) - 1)
}

/// Align address downwards.
///
/// Returns the greatest x with alignment `align` so that x <= addr.
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn align_down(addr: u64, alignment: u64) -> u64 {
    assert!(
        alignment.is_power_of_two(),
        "`alignment` must be a power of two"
    );
    addr & !(alignment - 1)
}

/// Align address upwards.
///
/// Returns the smallest x with alignment `align` so that x >= addr.
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn align_up_bits(value: u64, alignment_bits: u64) -> u64 {
    let align_mask = (1 << alignment_bits) - 1;
    if value & align_mask == 0 {
        value // already aligned
    } else {
        (value | align_mask) + 1
    }
}

/// Align address upwards.
///
/// Returns the smallest x with alignment `align` so that x >= addr.
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn align_up(value: u64, alignment: u64) -> u64 {
    assert!(
        alignment.is_power_of_two(),
        "`alignment` must be a power of two"
    );

    let align_mask = alignment - 1;
    if value & align_mask == 0 {
        value // already aligned
    } else {
        (value | align_mask) + 1
    }
}

/// Check if a value is aligned to a given alignment.
#[inline(always)]
pub const fn is_aligned_bits(value: u64, alignment_bits: u64) -> bool {
    (value & ((1 << alignment_bits) - 1)) == 0
}

/// Check if a value is aligned to a given alignment.
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn is_aligned(value: u64, alignment: u64) -> bool {
    debug_assert!(
        alignment.is_power_of_two(),
        "`alignment` must be a power of two"
    );

    (value & (alignment - 1)) == 0
}

/// Calculate the next possible aligned address without sanity checking the
/// input parameters.
#[inline]
pub fn aligned_addr_unchecked(addr: u64, alignment: u64) -> u64 {
    (addr + (alignment - 1)) & !(alignment - 1)
}
