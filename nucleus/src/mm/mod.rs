/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

pub mod bump_allocator;
pub use bump_allocator::BumpAllocator;

/// Align address downwards.
///
/// Returns the greatest x with alignment `align` so that x <= addr.
/// The alignment must be a power of 2.
pub fn align_down(addr: u64, align: usize) -> u64 {
    assert!(align.is_power_of_two(), "`align` must be a power of two");
    addr & !(align as u64 - 1)
}

/// Align address upwards.
///
/// Returns the smallest x with alignment `align` so that x >= addr.
/// The alignment must be a power of 2.
pub fn align_up(addr: u64, align: usize) -> u64 {
    assert!(align.is_power_of_two(), "`align` must be a power of two");
    let align_mask = align as u64 - 1;
    if addr & align_mask == 0 {
        addr // already aligned
    } else {
        (addr | align_mask) + 1
    }
}

/// Calculate the next possible aligned address without sanity checking the
/// input parameters.
// u64 for return and addr?
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
