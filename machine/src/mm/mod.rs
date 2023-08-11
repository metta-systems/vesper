/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

mod bump_allocator;
pub use bump_allocator::BumpAllocator;

/// Align address downwards.
///
/// Returns the greatest x with alignment `align` so that x <= addr.
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn align_down(addr: usize, alignment: usize) -> usize {
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
pub const fn align_up(value: usize, alignment: usize) -> usize {
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
/// The alignment must be a power of 2.
#[inline(always)]
pub const fn is_aligned(value: usize, alignment: usize) -> bool {
    assert!(
        alignment.is_power_of_two(),
        "`alignment` must be a power of two"
    );

    (value & (alignment - 1)) == 0
}

/// Convert a size into human readable format.
pub const fn size_human_readable_ceil(size: usize) -> (usize, &'static str) {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * 1024;
    const GIB: usize = 1024 * 1024 * 1024;

    if (size / GIB) > 0 {
        (size.div_ceil(GIB), "GiB")
    } else if (size / MIB) > 0 {
        (size.div_ceil(MIB), "MiB")
    } else if (size / KIB) > 0 {
        (size.div_ceil(KIB), "KiB")
    } else {
        (size, "Byte")
    }
}

/// Calculate the next possible aligned address without sanity checking the
/// input parameters.
#[inline]
fn aligned_addr_unchecked(addr: usize, alignment: usize) -> usize {
    (addr + (alignment - 1)) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
