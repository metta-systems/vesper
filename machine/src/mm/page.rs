use core::{
    fmt::{Debug, Display},
    ops::Range,
};

/// Represents a page size supported by the architecture
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PageSize {
    /// Size in bytes
    size: usize,
    /// Bits needed to represent the offset within a page
    offset_bits: u8,
    /// Page table level where this size is used
    level: u8,
}

impl PageSize {
    /// Create a new page size
    pub const fn new(size: usize, level: u8) -> Self {
        // Size must be power of 2
        assert!(size.is_power_of_two());

        Self {
            size,
            offset_bits: size.trailing_zeros() as u8,
            level,
        }
    }

    /// Get the page mask for this page size
    pub const fn mask(&self) -> usize {
        self.size - 1
    }

    /// Get the inverse mask for page alignment
    pub const fn inverse_mask(&self) -> usize {
        !self.mask()
    }

    /// Check if an address is aligned to this page size
    pub fn is_aligned(&self, addr: usize) -> bool {
        (addr & self.mask()) == 0
    }
}

/// Collection of page sizes supported by an architecture
pub trait PageSizeSupport {
    /// Get all supported page sizes
    fn supported_sizes(&self) -> &[PageSize];

    /// Get the smallest supported page size
    fn base_page_size(&self) -> PageSize {
        *self.supported_sizes().first().unwrap()
    }

    /// Get the largest supported page size
    fn max_page_size(&self) -> PageSize {
        *self.supported_sizes().last().unwrap()
    }
}
